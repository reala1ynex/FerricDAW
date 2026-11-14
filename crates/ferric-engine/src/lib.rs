pub mod graph;
pub mod wav_io;

pub use graph::{AudioGraphCore, TrackMeter, MAX_ENGINE_TRACKS};
pub use wav_io::{
    import_wav_file, render_all_stems_offline, render_master_offline, write_wav_file, BitDepth,
    BounceReport, WavIoError,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ferric_dsp::{
    FxRuntimeInstance, MasterMeterSnapshot, MidiEvent, ModulatorRuntime, TunerReading,
};
use ferric_model::{AudioBuffer, MidiNote, Project, TrackId};
use midir::{Ignore, MidiInput, MidiInputConnection};
use rtrb::{Consumer, Producer, RingBuffer};
use std::time::Instant;

const RETRO_SECONDS: usize = 20;
const RETRO_MIDI_CAP: usize = 2048;

#[derive(Debug, Clone, Copy)]
struct StampedMidi {
    frame_counter: u64,
    beat_estimate: f64,
    note: u8,
    velocity: u8,
    is_note_on: bool,
}

/// Commands sent wait-free from UI thread -> Real-Time Audio Callback.
pub enum EngineCommand {
    Play,
    Stop,
    SetRecord(bool),
    SeekBeat(f64),
    SyncProject(Box<Project>),
    RegisterFx(Box<FxRuntimeInstance>),
    RegisterMod(Box<ModulatorRuntime>),
    LiveMidi {
        target_track: Option<TrackId>,
        note: u8,
        velocity: u8,
        is_note_on: bool,
    },
    CaptureRetroactive {
        seconds_back: f32,
    },
    PanicAllNotesOff,
}

/// Telemetry snapshots sent wait-free from Real-Time Audio Callback -> UI thread (~60 Hz).
#[derive(Debug, Clone, Copy)]
pub struct EngineTelemetry {
    pub is_playing: bool,
    pub is_recording: bool,
    pub playhead_beat: f64,
    pub sample_rate: u32,
    pub cpu_load_percent: f32,
    pub master: MasterMeterSnapshot,
    pub tuner: TunerReading,
    pub track_meters: [TrackMeter; MAX_ENGINE_TRACKS],
    pub track_meter_count: usize,
    pub retro_midi_events_buffered: usize,
}

impl Default for EngineTelemetry {
    fn default() -> Self {
        Self {
            is_playing: false,
            is_recording: false,
            playhead_beat: 0.0,
            sample_rate: 48_000,
            cpu_load_percent: 0.0,
            master: MasterMeterSnapshot::default(),
            tuner: TunerReading::default(),
            track_meters: [TrackMeter::default(); MAX_ENGINE_TRACKS],
            track_meter_count: 0,
            retro_midi_events_buffered: 0,
        }
    }
}

/// Completed live-recorded or retroactively captured take returned from audio engine to UI!
pub enum CapturedTakeEvent {
    MidiTake {
        start_beat: f64,
        length_beats: f64,
        notes: Vec<MidiNote>,
        is_retroactive: bool,
    },
    AudioTake {
        start_beat: f64,
        length_beats: f64,
        sample_rate: u32,
        left: Vec<f32>,
        right: Vec<f32>,
        is_retroactive: bool,
    },
}

struct RtCallbackState {
    project: Box<Project>,
    core: AudioGraphCore,
    is_recording: bool,
    record_start_beat: f64,
    frame_counter: u64,
    cmd_rx: Consumer<EngineCommand>,
    hw_midi_rx: Consumer<(u8, u8, bool)>,
    hw_audio_rx: Consumer<(f32, f32)>,
    telem_tx: Producer<EngineTelemetry>,
    garbage_tx: Producer<Box<Project>>,
    capture_req_tx: Producer<CaptureRequest>,
    // Pre-allocated scratch buffers
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    live_midi_block: Vec<(Option<TrackId>, MidiEvent)>,
    // Always-listening retroactive circular buffers (zero runtime alloc!)
    retro_audio_l: Vec<f32>,
    retro_audio_r: Vec<f32>,
    retro_audio_pos: usize,
    has_hw_audio_signal: bool,
    retro_midi: [StampedMidi; RETRO_MIDI_CAP],
    retro_midi_pos: usize,
    retro_midi_len: usize,
    // Live recording accumulators (pre-allocated capacity!)
    rec_midi: Vec<StampedMidi>,
    rec_audio_l: Vec<f32>,
    rec_audio_r: Vec<f32>,
    samples_since_telem: usize,
    smoothed_cpu: f32,
}

enum CaptureRequest {
    RetroMidi {
        bpm: f64,
        sample_rate: u32,
        current_frame: u64,
        current_beat: f64,
        events: Vec<StampedMidi>,
    },
    RetroAudio {
        bpm: f64,
        sample_rate: u32,
        current_beat: f64,
        left: Vec<f32>,
        right: Vec<f32>,
    },
    FinishedLiveMidi {
        start_beat: f64,
        end_beat: f64,
        events: Vec<StampedMidi>,
    },
    FinishedLiveAudio {
        start_beat: f64,
        end_beat: f64,
        sample_rate: u32,
        left: Vec<f32>,
        right: Vec<f32>,
    },
}

/// Main handle held by the UI application. Keeps `cpal` & `midir` streams alive.
pub struct EngineHandle {
    pub sample_rate: u32,
    pub audio_device_name: String,
    pub midi_port_names: Vec<String>,
    cmd_tx: Producer<EngineCommand>,
    telem_rx: Consumer<EngineTelemetry>,
    garbage_rx: Consumer<Box<Project>>,
    capture_req_rx: Consumer<CaptureRequest>,
    known_fx_ids: Vec<ferric_model::FxId>,
    known_mod_ids: Vec<ferric_model::ModulatorId>,
    pub latest_telemetry: EngineTelemetry,
    _output_stream: Option<cpal::Stream>,
    _input_stream: Option<cpal::Stream>,
    _midi_conns: Vec<MidiInputConnection<()>>,
}

impl EngineHandle {
    pub fn start(initial_project: &Project) -> Self {
        let host = cpal::default_host();
        let out_device = host.default_output_device();
        let (sample_rate, out_config_opt, device_name) = if let Some(ref dev) = out_device {
            let name = dev.name().unwrap_or_else(|_| "Default Audio Output".into());
            match dev.default_output_config() {
                Ok(cfg) => (cfg.sample_rate().0, Some(cfg), name),
                Err(_) => (48_000, None, format!("{} (Offline Fallback)", name)),
            }
        } else {
            (48_000, None, "No Hardware Output (Offline Mode)".into())
        };

        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCommand>::new(512);
        let (telem_tx, telem_rx) = RingBuffer::<EngineTelemetry>::new(64);
        let (garbage_tx, garbage_rx) = RingBuffer::<Box<Project>>::new(64);
        let (hw_midi_tx, hw_midi_rx) = RingBuffer::<(u8, u8, bool)>::new(1024);
        let (hw_audio_tx, hw_audio_rx) = RingBuffer::<(f32, f32)>::new(sample_rate as usize / 2);
        let (capture_req_tx, capture_req_rx) = RingBuffer::<CaptureRequest>::new(16);

        // Connect all available hardware MIDI input controllers automatically!
        let (midi_conns, midi_port_names) = Self::connect_hardware_midi(hw_midi_tx);

        // Optional hardware audio input stream (mic / guitar interface)
        let input_stream = Self::start_hardware_audio_input(&host, sample_rate, hw_audio_tx);

        let max_block = 4096usize;
        let mut core = AudioGraphCore::new(sample_rate as f32, max_block);
        core.sync_processors_from_project(initial_project);

        let mut known_fx_ids = Vec::new();
        let mut known_mod_ids = Vec::new();
        collect_ids(initial_project, &mut known_fx_ids, &mut known_mod_ids);

        let retro_frames = sample_rate as usize * RETRO_SECONDS;
        let mut rt = RtCallbackState {
            project: Box::new(initial_project.clone()),
            core,
            is_recording: false,
            record_start_beat: 0.0,
            frame_counter: 0,
            cmd_rx,
            hw_midi_rx,
            hw_audio_rx,
            telem_tx,
            garbage_tx,
            capture_req_tx,
            out_l: vec![0.0; max_block],
            out_r: vec![0.0; max_block],
            in_l: vec![0.0; max_block],
            in_r: vec![0.0; max_block],
            live_midi_block: Vec::with_capacity(128),
            retro_audio_l: vec![0.0; retro_frames],
            retro_audio_r: vec![0.0; retro_frames],
            retro_audio_pos: 0,
            has_hw_audio_signal: false,
            retro_midi: [StampedMidi {
                frame_counter: 0,
                beat_estimate: 0.0,
                note: 60,
                velocity: 0,
                is_note_on: false,
            }; RETRO_MIDI_CAP],
            retro_midi_pos: 0,
            retro_midi_len: 0,
            rec_midi: Vec::with_capacity(4096),
            rec_audio_l: Vec::with_capacity(sample_rate as usize * 120),
            rec_audio_r: Vec::with_capacity(sample_rate as usize * 120),
            samples_since_telem: 0,
            smoothed_cpu: 0.2,
        };

        let output_stream = if let (Some(dev), Some(supported_cfg)) = (out_device, out_config_opt) {
            let channels = supported_cfg.channels() as usize;
            let stream_cfg: cpal::StreamConfig = supported_cfg.clone().into();
            let err_fn = |err| log::warn!("Audio output stream error: {}", err);

            let stream_res = match supported_cfg.sample_format() {
                cpal::SampleFormat::F32 => dev.build_output_stream(
                    &stream_cfg,
                    move |data: &mut [f32], _| {
                        rt.process_interleaved_f32(data, channels);
                    },
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::I16 => dev.build_output_stream(
                    &stream_cfg,
                    move |data: &mut [i16], _| {
                        let frames = data.len() / channels.max(1);
                        let mut tmp = [0.0_f32; 4096];
                        let n = (frames * 2).min(tmp.len());
                        rt.process_interleaved_f32(&mut tmp[..n], 2);
                        for f in 0..(n / 2) {
                            let l = (tmp[f * 2].clamp(-1.0, 1.0) * 32767.0) as i16;
                            let r = (tmp[f * 2 + 1].clamp(-1.0, 1.0) * 32767.0) as i16;
                            data[f * channels] = l;
                            if channels > 1 {
                                data[f * channels + 1] = r;
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
            };

            match stream_res {
                Ok(s) => {
                    let _ = s.play();
                    Some(s)
                }
                Err(e) => {
                    log::warn!("Could not open audio stream: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let mut latest_telemetry = EngineTelemetry::default();
        latest_telemetry.sample_rate = sample_rate;

        Self {
            sample_rate,
            audio_device_name: device_name,
            midi_port_names,
            cmd_tx,
            telem_rx,
            garbage_rx,
            capture_req_rx,
            known_fx_ids,
            known_mod_ids,
            latest_telemetry,
            _output_stream: output_stream,
            _input_stream: input_stream,
            _midi_conns: midi_conns,
        }
    }

    fn connect_hardware_midi(
        hw_midi_tx: Producer<(u8, u8, bool)>,
    ) -> (Vec<MidiInputConnection<()>>, Vec<String>) {
        let mut conns = Vec::new();
        let mut names = Vec::new();
        let Ok(mut midi_in) = MidiInput::new("FerricDAW MIDI Input") else {
            return (conns, names);
        };
        midi_in.ignore(Ignore::ActiveSense | Ignore::Time);
        let ports = midi_in.ports();
        // Connect first available hardware MIDI port using the lock-free SPSC producer
        if let Some(port) = ports.first() {
            let port_name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| "MIDI Controller".into());
            let mut tx = hw_midi_tx;
            if let Ok(conn) = midi_in.connect(
                port,
                "ferric-midi-in",
                move |_stamp, msg, _| {
                    if msg.len() >= 3 {
                        let status = msg[0] & 0xF0;
                        let note = msg[1] & 0x7F;
                        let vel = msg[2] & 0x7F;
                        if status == 0x90 && vel > 0 {
                            let _ = tx.push((note, vel, true));
                        } else if status == 0x80 || (status == 0x90 && vel == 0) {
                            let _ = tx.push((note, 0, false));
                        }
                    }
                },
                (),
            ) {
                names.push(port_name);
                conns.push(conn);
            }
        }
        (conns, names)
    }

    fn start_hardware_audio_input(
        host: &cpal::Host,
        target_sr: u32,
        mut hw_audio_tx: Producer<(f32, f32)>,
    ) -> Option<cpal::Stream> {
        let dev = host.default_input_device()?;
        let cfg = dev.default_input_config().ok()?;
        if cfg.sample_rate().0 != target_sr || cfg.sample_format() != cpal::SampleFormat::F32 {
            return None;
        }
        let channels = cfg.channels() as usize;
        let stream_cfg: cpal::StreamConfig = cfg.into();
        let stream = dev
            .build_input_stream(
                &stream_cfg,
                move |data: &[f32], _| {
                    let frames = data.len() / channels.max(1);
                    for f in 0..frames {
                        let l = data[f * channels];
                        let r = if channels > 1 {
                            data[f * channels + 1]
                        } else {
                            l
                        };
                        let _ = hw_audio_tx.push((l, r));
                    }
                },
                |err| log::warn!("Audio input error: {}", err),
                None,
            )
            .ok()?;
        let _ = stream.play();
        Some(stream)
    }

    pub fn send(&mut self, cmd: EngineCommand) {
        let _ = self.cmd_tx.push(cmd);
    }

    /// Synchronizes an edited `Project` to the real-time thread, pre-allocating any newly added
    /// `FxRuntimeInstance` or `ModulatorRuntime` on the UI thread BEFORE sending!
    pub fn sync_project(&mut self, project: &Project) {
        let sr = self.sample_rate as f32;
        for track in &project.tracks {
            for fx in &track.fx_chain {
                if !self.known_fx_ids.contains(&fx.id) {
                    self.known_fx_ids.push(fx.id);
                    let inst = FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
                    let _ = self.cmd_tx.push(EngineCommand::RegisterFx(Box::new(inst)));
                }
            }
            for clip in &track.clips {
                for fx in &clip.clip_fx {
                    if !self.known_fx_ids.contains(&fx.id) {
                        self.known_fx_ids.push(fx.id);
                        let inst = FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
                        let _ = self.cmd_tx.push(EngineCommand::RegisterFx(Box::new(inst)));
                    }
                }
            }
            for m in &track.modulators {
                if !self.known_mod_ids.contains(&m.id) {
                    self.known_mod_ids.push(m.id);
                    let inst = ModulatorRuntime::new(m.id);
                    let _ = self.cmd_tx.push(EngineCommand::RegisterMod(Box::new(inst)));
                }
            }
        }
        for fx in &project.master.fx_chain {
            if !self.known_fx_ids.contains(&fx.id) {
                self.known_fx_ids.push(fx.id);
                let inst = FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
                let _ = self.cmd_tx.push(EngineCommand::RegisterFx(Box::new(inst)));
            }
        }
        let _ = self
            .cmd_tx
            .push(EngineCommand::SyncProject(Box::new(project.clone())));
    }

    /// Polls telemetry, drops retired `Project` boxes on the UI thread, and harvests any
    /// newly recorded or retroactively captured takes!
    pub fn poll_ui(&mut self, out_captures: &mut Vec<CapturedTakeEvent>) {
        while let Ok(old_proj) = self.garbage_rx.pop() {
            drop(old_proj);
        }
        while let Ok(telem) = self.telem_rx.pop() {
            self.latest_telemetry = telem;
        }
        while let Ok(req) = self.capture_req_rx.pop() {
            if let Some(ev) = convert_capture_request(req) {
                out_captures.push(ev);
            }
        }
    }
}

impl RtCallbackState {
    fn process_interleaved_f32(&mut self, output: &mut [f32], out_channels: usize) {
        let start_inst = Instant::now();
        let total_frames = output.len() / out_channels.max(1);
        if total_frames == 0 {
            return;
        }

        // Drain UI commands without locking
        self.live_midi_block.clear();
        while let Ok(cmd) = self.cmd_rx.pop() {
            match cmd {
                EngineCommand::Play => {
                    self.core.is_playing = true;
                    if self.is_recording {
                        self.record_start_beat = self.core.playhead_beat;
                        self.rec_midi.clear();
                        self.rec_audio_l.clear();
                        self.rec_audio_r.clear();
                    }
                }
                EngineCommand::Stop => {
                    if self.is_recording && self.core.is_playing {
                        self.flush_live_recording();
                    }
                    self.core.is_playing = false;
                    self.core.all_notes_off();
                }
                EngineCommand::SetRecord(rec) => {
                    if self.is_recording && !rec && self.core.is_playing {
                        self.flush_live_recording();
                    }
                    self.is_recording = rec;
                    if rec {
                        self.record_start_beat = self.core.playhead_beat;
                        self.rec_midi.clear();
                        self.rec_audio_l.clear();
                        self.rec_audio_r.clear();
                    }
                }
                EngineCommand::SeekBeat(beat) => {
                    self.core.playhead_beat = beat.max(0.0);
                    self.core.all_notes_off();
                }
                EngineCommand::SyncProject(new_proj) => {
                    let old = std::mem::replace(&mut self.project, new_proj);
                    let _ = self.garbage_tx.push(old);
                }
                EngineCommand::RegisterFx(inst) => {
                    self.core.register_fx_instance(*inst);
                }
                EngineCommand::RegisterMod(inst) => {
                    self.core.register_mod_instance(*inst);
                }
                EngineCommand::LiveMidi {
                    target_track,
                    note,
                    velocity,
                    is_note_on,
                } => {
                    self.record_stamped_midi(note, velocity, is_note_on);
                    self.live_midi_block.push((
                        target_track,
                        MidiEvent {
                            sample_offset: 0,
                            note,
                            velocity,
                            is_note_on,
                        },
                    ));
                }
                EngineCommand::CaptureRetroactive { seconds_back } => {
                    self.emit_retroactive_capture(seconds_back);
                }
                EngineCommand::PanicAllNotesOff => {
                    self.core.all_notes_off();
                }
            }
        }

        // Drain hardware MIDI controller events
        while let Ok((note, velocity, is_note_on)) = self.hw_midi_rx.pop() {
            self.record_stamped_midi(note, velocity, is_note_on);
            self.live_midi_block.push((
                None,
                MidiEvent {
                    sample_offset: 0,
                    note,
                    velocity,
                    is_note_on,
                },
            ));
        }

        // Process in chunks <= max_block
        let mut offset_frame = 0usize;
        while offset_frame < total_frames {
            let chunk = (total_frames - offset_frame).min(self.core.max_block);

            // Pull hardware audio input & feed Always-Listening Retroactive Ring Buffer
            let retro_cap = self.retro_audio_l.len();
            for i in 0..chunk {
                let (il, ir) = self.hw_audio_rx.pop().unwrap_or((0.0, 0.0));
                self.in_l[i] = il;
                self.in_r[i] = ir;
                if il.abs() > 0.002 || ir.abs() > 0.002 {
                    self.has_hw_audio_signal = true;
                }
                self.retro_audio_l[self.retro_audio_pos] = il;
                self.retro_audio_r[self.retro_audio_pos] = ir;
                self.retro_audio_pos = (self.retro_audio_pos + 1) % retro_cap;

                if self.is_recording
                    && self.core.is_playing
                    && self.rec_audio_l.len() < self.rec_audio_l.capacity()
                {
                    self.rec_audio_l.push(il);
                    self.rec_audio_r.push(ir);
                }
            }

            let prev_beat = self.core.playhead_beat;
            let midi_slice = if offset_frame == 0 {
                self.live_midi_block.as_slice()
            } else {
                &[]
            };

            self.core.render_block(
                &self.project,
                &self.in_l[..chunk],
                &self.in_r[..chunk],
                midi_slice,
                &mut self.out_l[..chunk],
                &mut self.out_r[..chunk],
            );

            // Detect Loop Wrap during active recording -> auto-commit Take Lane (REAPER loop take recording!)
            if self.is_recording
                && self.core.is_playing
                && self.core.playhead_beat < prev_beat - 0.5
            {
                self.flush_live_recording_at(self.project.loop_end_beat);
                self.record_start_beat = self.project.loop_start_beat;
            }

            for i in 0..chunk {
                let base = (offset_frame + i) * out_channels;
                output[base] = self.out_l[i];
                if out_channels > 1 {
                    output[base + 1] = self.out_r[i];
                }
                for ch in 2..out_channels {
                    output[base + ch] = 0.0;
                }
            }

            self.frame_counter += chunk as u64;
            self.samples_since_telem += chunk;
            offset_frame += chunk;
        }

        // Compute real-time callback CPU budget %
        let elapsed_secs = start_inst.elapsed().as_secs_f32();
        let budget_secs = total_frames as f32 / self.core.sample_rate;
        let load = if budget_secs > 1e-6 {
            (elapsed_secs / budget_secs) * 100.0
        } else {
            0.0
        };
        self.smoothed_cpu = self.smoothed_cpu * 0.88 + load * 0.12;

        if self.samples_since_telem >= (self.core.sample_rate as usize / 60).max(256) {
            self.samples_since_telem = 0;
            let _ = self.telem_tx.push(EngineTelemetry {
                is_playing: self.core.is_playing,
                is_recording: self.is_recording,
                playhead_beat: self.core.playhead_beat,
                sample_rate: self.core.sample_rate as u32,
                cpu_load_percent: self.smoothed_cpu,
                master: self.core.last_master_meter,
                tuner: self.core.best_tuner_reading(),
                track_meters: self.core.track_meters,
                track_meter_count: self.core.track_meter_count,
                retro_midi_events_buffered: self.retro_midi_len,
            });
        }
    }

    fn record_stamped_midi(&mut self, note: u8, velocity: u8, is_note_on: bool) {
        let ev = StampedMidi {
            frame_counter: self.frame_counter,
            beat_estimate: self.core.playhead_beat,
            note,
            velocity,
            is_note_on,
        };
        self.retro_midi[self.retro_midi_pos] = ev;
        self.retro_midi_pos = (self.retro_midi_pos + 1) % RETRO_MIDI_CAP;
        self.retro_midi_len = (self.retro_midi_len + 1).min(RETRO_MIDI_CAP);

        if self.is_recording
            && self.core.is_playing
            && self.rec_midi.len() < self.rec_midi.capacity()
        {
            self.rec_midi.push(ev);
        }
    }

    fn flush_live_recording(&mut self) {
        self.flush_live_recording_at(self.core.playhead_beat);
    }

    fn flush_live_recording_at(&mut self, end_beat: f64) {
        if !self.rec_midi.is_empty() {
            let events = self.rec_midi.clone();
            self.rec_midi.clear();
            let _ = self.capture_req_tx.push(CaptureRequest::FinishedLiveMidi {
                start_beat: self.record_start_beat,
                end_beat: end_beat.max(self.record_start_beat + 1.0),
                events,
            });
            self.rec_audio_l.clear();
            self.rec_audio_r.clear();
        } else if self.has_hw_audio_signal && self.rec_audio_l.len() > 2048 {
            let left = self.rec_audio_l.clone();
            let right = self.rec_audio_r.clone();
            self.rec_audio_l.clear();
            self.rec_audio_r.clear();
            let _ = self.capture_req_tx.push(CaptureRequest::FinishedLiveAudio {
                start_beat: self.record_start_beat,
                end_beat: end_beat.max(self.record_start_beat + 1.0),
                sample_rate: self.core.sample_rate as u32,
                left,
                right,
            });
        }
    }

    fn emit_retroactive_capture(&mut self, seconds_back: f32) {
        let sr = self.core.sample_rate as u32;
        let window_frames = (seconds_back.clamp(2.0, RETRO_SECONDS as f32) * sr as f32) as u64;
        let min_frame = self.frame_counter.saturating_sub(window_frames);

        // Prefer retroactive MIDI if notes were jammed recently
        let mut recent_midi = Vec::new();
        let count = self.retro_midi_len;
        let start_idx = (self.retro_midi_pos + RETRO_MIDI_CAP - count) % RETRO_MIDI_CAP;
        for i in 0..count {
            let ev = self.retro_midi[(start_idx + i) % RETRO_MIDI_CAP];
            if ev.frame_counter >= min_frame {
                recent_midi.push(ev);
            }
        }

        if !recent_midi.is_empty() {
            self.retro_midi_len = 0;
            let _ = self.capture_req_tx.push(CaptureRequest::RetroMidi {
                bpm: self.project.bpm,
                sample_rate: sr,
                current_frame: self.frame_counter,
                current_beat: self.core.playhead_beat,
                events: recent_midi,
            });
            return;
        }

        if self.has_hw_audio_signal {
            let n = (window_frames as usize).min(self.retro_audio_l.len());
            let cap = self.retro_audio_l.len();
            let mut left = Vec::with_capacity(n);
            let mut right = Vec::with_capacity(n);
            let start = (self.retro_audio_pos + cap - n) % cap;
            for i in 0..n {
                left.push(self.retro_audio_l[(start + i) % cap]);
                right.push(self.retro_audio_r[(start + i) % cap]);
            }
            let _ = self.capture_req_tx.push(CaptureRequest::RetroAudio {
                bpm: self.project.bpm,
                sample_rate: sr,
                current_beat: self.core.playhead_beat,
                left,
                right,
            });
        }
    }
}

fn convert_capture_request(req: CaptureRequest) -> Option<CapturedTakeEvent> {
    match req {
        CaptureRequest::RetroMidi {
            bpm,
            sample_rate,
            current_frame,
            current_beat,
            events,
        } => {
            let first_frame = events.first()?.frame_counter;
            let span_secs = (current_frame.saturating_sub(first_frame)) as f64 / sample_rate as f64;
            let span_beats = ((span_secs * bpm / 60.0).ceil()).max(4.0);
            let start_beat = (current_beat - span_beats).max(0.0).floor();

            let mut notes = Vec::new();
            let mut active: [Option<(u64, u8)>; 128] = [None; 128];
            for ev in &events {
                let idx = (ev.note as usize) & 127;
                if ev.is_note_on && ev.velocity > 0 {
                    active[idx] = Some((ev.frame_counter, ev.velocity));
                } else if let Some((on_frame, vel)) = active[idx].take() {
                    let s_b = ((on_frame - first_frame) as f64 / sample_rate as f64) * (bpm / 60.0);
                    let e_b = ((ev.frame_counter.saturating_sub(first_frame)) as f64
                        / sample_rate as f64)
                        * (bpm / 60.0);
                    notes.push(MidiNote::new(ev.note, vel, s_b, (e_b - s_b).max(0.125)));
                }
            }
            // Close any still-held notes
            for (pitch, slot) in active.iter().enumerate() {
                if let Some((on_frame, vel)) = slot {
                    let s_b = ((on_frame - first_frame) as f64 / sample_rate as f64) * (bpm / 60.0);
                    notes.push(MidiNote::new(pitch as u8, *vel, s_b, 0.5));
                }
            }
            if notes.is_empty() {
                return None;
            }
            Some(CapturedTakeEvent::MidiTake {
                start_beat,
                length_beats: span_beats,
                notes,
                is_retroactive: true,
            })
        }
        CaptureRequest::RetroAudio {
            bpm,
            sample_rate,
            current_beat,
            left,
            right,
        } => {
            let secs = left.len() as f64 / sample_rate as f64;
            let beats = (secs * bpm / 60.0).max(1.0);
            Some(CapturedTakeEvent::AudioTake {
                start_beat: (current_beat - beats).max(0.0),
                length_beats: beats,
                sample_rate,
                left,
                right,
                is_retroactive: true,
            })
        }
        CaptureRequest::FinishedLiveMidi {
            start_beat,
            end_beat,
            events,
        } => {
            let mut notes = Vec::new();
            let mut active: [Option<(f64, u8)>; 128] = [None; 128];
            for ev in &events {
                let rel_beat = (ev.beat_estimate - start_beat).max(0.0);
                let idx = (ev.note as usize) & 127;
                if ev.is_note_on && ev.velocity > 0 {
                    active[idx] = Some((rel_beat, ev.velocity));
                } else if let Some((on_b, vel)) = active[idx].take() {
                    notes.push(MidiNote::new(ev.note, vel, on_b, (rel_beat - on_b).max(0.125)));
                }
            }
            for (pitch, slot) in active.iter().enumerate() {
                if let Some(&(on_b, vel)) = slot.as_ref() {
                    notes.push(MidiNote::new(
                        pitch as u8,
                        vel,
                        on_b,
                        (end_beat - start_beat - on_b).max(0.25),
                    ));
                }
            }
            if notes.is_empty() {
                return None;
            }
            Some(CapturedTakeEvent::MidiTake {
                start_beat,
                length_beats: (end_beat - start_beat).max(1.0),
                notes,
                is_retroactive: false,
            })
        }
        CaptureRequest::FinishedLiveAudio {
            start_beat,
            end_beat,
            sample_rate,
            left,
            right,
        } => Some(CapturedTakeEvent::AudioTake {
            start_beat,
            length_beats: (end_beat - start_beat).max(1.0),
            sample_rate,
            left,
            right,
            is_retroactive: false,
        }),
    }
}

fn collect_ids(
    project: &Project,
    fx_ids: &mut Vec<ferric_model::FxId>,
    mod_ids: &mut Vec<ferric_model::ModulatorId>,
) {
    for t in &project.tracks {
        for fx in &t.fx_chain {
            fx_ids.push(fx.id);
        }
        for c in &t.clips {
            for fx in &c.clip_fx {
                fx_ids.push(fx.id);
            }
        }
        for m in &t.modulators {
            mod_ids.push(m.id);
        }
    }
    for fx in &project.master.fx_chain {
        fx_ids.push(fx.id);
    }
}

pub fn audio_buffer_from_capture(sr: u32, left: Vec<f32>, right: Vec<f32>) -> AudioBuffer {
    AudioBuffer::from_stereo(sr, left, right, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_model::build_demo_project;

    #[test]
    fn offline_master_bounce_renders_demo_song_cleanly() {
        let proj = build_demo_project(48_000);
        let tmp = std::env::temp_dir().join("ferric_test_bounce.wav");
        let report = render_master_offline(&proj, &tmp, 48_000, BitDepth::Pcm16, 0.0, 4.0)
            .expect("bounce succeeds");
        assert!(report.peak_db > -45.0 && report.peak_db <= 0.1);
        assert!(tmp.exists());
        let imported = import_wav_file(&tmp, 48_000).expect("re-import wav");
        assert!(imported.len_frames() > 48_000);
        let _ = std::fs::remove_file(&tmp);
    }
}
