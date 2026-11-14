pub mod clap_host;
pub mod graph;
pub mod wav_io;

pub use clap_host::{
    inspect_clap_bundle, scan_clap_plugins, DiscoveredClapPlugin, HostedClapInstance,
};
pub use graph::{AudioGraphCore, TrackMeter, MAX_ENGINE_TRACKS};
pub use wav_io::{
    import_audio_file, import_wav_file, render_all_stems_offline, render_master_offline,
    render_track_freeze_buffer, write_wav_file, BitDepth, BounceReport, WavIoError,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ferric_dsp::{
    db_to_lin, lin_to_db, FxRuntimeInstance, MasterMeterSnapshot, MidiEvent, ModulatorRuntime,
    TunerReading, VocalPitchTelemetry,
};
use ferric_model::{AudioBuffer, FxKind, MidiNote, Project, TrackId};
use midir::{Ignore, MidiInput, MidiInputConnection};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const RETRO_SECONDS: usize = 20;
const RETRO_MIDI_CAP: usize = 2048;

#[derive(Debug, Clone, Copy)]
enum HwMidiPacket {
    Note {
        note: u8,
        velocity: u8,
        is_note_on: bool,
    },
    ControlChange {
        channel: u8,
        cc: u8,
        value: u8,
    },
}

#[derive(Debug, Clone, Copy)]
struct StampedMidi {
    frame_counter: u64,
    beat_estimate: f64,
    note: u8,
    velocity: u8,
    is_note_on: bool,
}

pub enum EngineCommand {
    Play,
    Stop,
    SetRecord(bool),
    SeekBeat(f64),
    SetInputGainDb(f32),
    SyncProject(Box<Project>),
    RegisterFx(Box<FxRuntimeInstance>),
    RegisterMod(Box<ModulatorRuntime>),
    PreviewBuffer(AudioBuffer),
    StopPreview,
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

#[derive(Debug, Clone, Copy)]
pub struct EngineTelemetry {
    pub is_playing: bool,
    pub is_recording: bool,
    pub playhead_beat: f64,
    pub sample_rate: u32,
    pub cpu_load_percent: f32,
    pub input_peak_db: f32,
    pub total_pdc_samples: u32,
    pub sustain_down: bool,
    pub last_midi_note: Option<u8>,
    pub last_midi_cc: Option<(u8, u8, u8)>, // (channel, cc, value) for 1-click MIDI Learn!
    pub master: MasterMeterSnapshot,
    pub tuner: TunerReading,
    pub vocal_pitch: VocalPitchTelemetry,
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
            input_peak_db: -96.0,
            total_pdc_samples: 0,
            sustain_down: false,
            last_midi_note: None,
            last_midi_cc: None,
            master: MasterMeterSnapshot::default(),
            tuner: TunerReading::default(),
            vocal_pitch: VocalPitchTelemetry::default(),
            track_meters: [TrackMeter::default(); MAX_ENGINE_TRACKS],
            track_meter_count: 0,
            retro_midi_events_buffered: 0,
        }
    }
}

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
    hw_midi_rx: Consumer<HwMidiPacket>,
    hw_audio_rx: Consumer<(f32, f32)>,
    telem_tx: Producer<EngineTelemetry>,
    garbage_tx: Producer<Box<Project>>,
    capture_req_tx: Producer<CaptureRequest>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    live_midi_block: Vec<(Option<TrackId>, MidiEvent)>,
    sustain_release_scratch: Vec<(Option<TrackId>, MidiEvent)>,
    preview_buf: Option<AudioBuffer>,
    preview_pos_frames: f64,
    retro_audio_l: Vec<f32>,
    retro_audio_r: Vec<f32>,
    retro_audio_pos: usize,
    has_hw_audio_signal: bool,
    retro_midi: [StampedMidi; RETRO_MIDI_CAP],
    retro_midi_pos: usize,
    retro_midi_len: usize,
    rec_midi: Vec<StampedMidi>,
    rec_audio_l: Vec<f32>,
    rec_audio_r: Vec<f32>,
    last_midi_note: Option<u8>,
    last_midi_cc: Option<(u8, u8, u8)>,
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

pub fn list_audio_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

pub fn list_audio_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

pub struct EngineHandle {
    pub sample_rate: u32,
    pub audio_device_name: String,
    pub audio_input_name: String,
    pub midi_port_names: Vec<String>,
    pub input_gain_db: f32,
    cmd_tx: Producer<EngineCommand>,
    telem_rx: Consumer<EngineTelemetry>,
    garbage_rx: Consumer<Box<Project>>,
    capture_req_rx: Consumer<CaptureRequest>,
    shared_midi_tx: Arc<Mutex<Producer<HwMidiPacket>>>,
    known_fx_ids: Vec<ferric_model::FxId>,
    known_mod_ids: Vec<ferric_model::ModulatorId>,
    pub latest_telemetry: EngineTelemetry,
    _output_stream: Option<cpal::Stream>,
    _input_stream: Option<cpal::Stream>,
    _midi_conns: Vec<MidiInputConnection<()>>,
}

impl EngineHandle {
    pub fn start(initial_project: &Project) -> Self {
        Self::start_with_devices(initial_project, None, None)
    }

    pub fn start_with_devices(
        initial_project: &Project,
        preferred_out: Option<&str>,
        preferred_in: Option<&str>,
    ) -> Self {
        let host = cpal::default_host();
        let out_device = preferred_out
            .and_then(|want| {
                host.output_devices()
                    .ok()?
                    .find(|d| d.name().ok().as_deref() == Some(want))
            })
            .or_else(|| host.default_output_device());

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
        let (hw_midi_tx, hw_midi_rx) = RingBuffer::<HwMidiPacket>::new(2048);
        let (hw_audio_tx, hw_audio_rx) = RingBuffer::<(f32, f32)>::new(sample_rate as usize);
        let (capture_req_tx, capture_req_rx) = RingBuffer::<CaptureRequest>::new(16);

        let shared_midi_tx = Arc::new(Mutex::new(hw_midi_tx));
        let (midi_conns, midi_port_names) = Self::connect_all_hardware_midi(shared_midi_tx.clone());

        let (input_stream, audio_input_name) =
            Self::start_hardware_audio_input(&host, preferred_in, sample_rate, hw_audio_tx);

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
            sustain_release_scratch: Vec::with_capacity(64),
            preview_buf: None,
            preview_pos_frames: 0.0,
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
            rec_audio_l: Vec::with_capacity(sample_rate as usize * 180),
            rec_audio_r: Vec::with_capacity(sample_rate as usize * 180),
            last_midi_note: None,
            last_midi_cc: None,
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
            audio_input_name,
            midi_port_names,
            input_gain_db: 0.0,
            cmd_tx,
            telem_rx,
            garbage_rx,
            capture_req_rx,
            shared_midi_tx,
            known_fx_ids,
            known_mod_ids,
            latest_telemetry,
            _output_stream: output_stream,
            _input_stream: input_stream,
            _midi_conns: midi_conns,
        }
    }

    /// Hot-plug rescan for USB MIDI controllers (e.g. plugging in the Akai LPK25 while FerricDAW is open!).
    pub fn rescan_midi_devices(&mut self) {
        self._midi_conns.clear();
        let (conns, names) = Self::connect_all_hardware_midi(self.shared_midi_tx.clone());
        self._midi_conns = conns;
        self.midi_port_names = names;
    }

    pub fn has_akai_lpk25(&self) -> bool {
        self.midi_port_names.iter().any(|n| {
            let up = n.to_uppercase();
            up.contains("LPK25") || up.contains("AKAI")
        })
    }

    /// Connects ALL available hardware USB MIDI controllers simultaneously (including Akai LPK25!).
    fn connect_all_hardware_midi(
        shared_tx: Arc<Mutex<Producer<HwMidiPacket>>>,
    ) -> (Vec<MidiInputConnection<()>>, Vec<String>) {
        let mut conns = Vec::new();
        let mut names = Vec::new();
        let Ok(probe) = MidiInput::new("FerricDAW MIDI Probe") else {
            return (conns, names);
        };
        let port_count = probe.ports().len();
        drop(probe);

        for idx in 0..port_count {
            let Ok(mut midi_in) = MidiInput::new(&format!("FerricDAW MIDI In {}", idx)) else {
                continue;
            };
            midi_in.ignore(Ignore::ActiveSense);
            let ports = midi_in.ports();
            let Some(port) = ports.get(idx) else {
                continue;
            };
            let port_name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| format!("MIDI Port {}", idx + 1));
            let tx_clone = shared_tx.clone();
            if let Ok(conn) = midi_in.connect(
                port,
                &format!("ferric-midi-{}", idx),
                move |_stamp, msg, _| {
                    if msg.len() >= 3 {
                        let status = msg[0] & 0xF0;
                        let ch = msg[0] & 0x0F;
                        let d1 = msg[1] & 0x7F;
                        let d2 = msg[2] & 0x7F;
                        let pkt = if status == 0x90 && d2 > 0 {
                            Some(HwMidiPacket::Note {
                                note: d1,
                                velocity: d2,
                                is_note_on: true,
                            })
                        } else if status == 0x80 || (status == 0x90 && d2 == 0) {
                            Some(HwMidiPacket::Note {
                                note: d1,
                                velocity: 0,
                                is_note_on: false,
                            })
                        } else if status == 0xB0 {
                            Some(HwMidiPacket::ControlChange {
                                channel: ch,
                                cc: d1,
                                value: d2,
                            })
                        } else {
                            None
                        };
                        if let Some(p) = pkt {
                            if let Ok(mut guard) = tx_clone.try_lock() {
                                let _ = guard.push(p);
                            }
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

    /// Universal Hardware Audio Line-In / Mic Input Stream (supports `F32` & `I16`, mono/stereo,
    /// AND automatic real-time sample-rate conversion — ideal for Nintendo DS KORG DS-10 3.5mm Line-In!).
    fn start_hardware_audio_input(
        host: &cpal::Host,
        preferred_in: Option<&str>,
        target_sr: u32,
        mut hw_audio_tx: Producer<(f32, f32)>,
    ) -> (Option<cpal::Stream>, String) {
        let dev_opt = preferred_in
            .and_then(|want| {
                host.input_devices()
                    .ok()?
                    .find(|d| d.name().ok().as_deref() == Some(want))
            })
            .or_else(|| host.default_input_device());

        let Some(dev) = dev_opt else {
            return (None, "No Audio Input Detected".into());
        };
        let in_name = dev.name().unwrap_or_else(|_| "Audio Line/Mic In".into());
        let Ok(cfg) = dev.default_input_config() else {
            return (None, format!("{} (Unavailable)", in_name));
        };

        let in_sr = cfg.sample_rate().0.max(8_000);
        let channels = cfg.channels().max(1) as usize;
        let fmt = cfg.sample_format();
        let stream_cfg: cpal::StreamConfig = cfg.into();
        let step = in_sr as f64 / target_sr.max(8_000) as f64;
        let mut frac_acc = 0.0_f64;
        let mut prev_lr = (0.0_f32, 0.0_f32);

        let stream_res = match fmt {
            cpal::SampleFormat::F32 => dev.build_input_stream(
                &stream_cfg,
                move |data: &[f32], _| {
                    let frames = data.len() / channels;
                    for f in 0..frames {
                        let l = data[f * channels];
                        let r = if channels > 1 {
                            data[f * channels + 1]
                        } else {
                            l
                        };
                        while frac_acc < 1.0 {
                            let t = frac_acc as f32;
                            let ol = prev_lr.0 + (l - prev_lr.0) * t;
                            let or = prev_lr.1 + (r - prev_lr.1) * t;
                            let _ = hw_audio_tx.push((ol, or));
                            frac_acc += step;
                        }
                        frac_acc -= 1.0;
                        prev_lr = (l, r);
                    }
                },
                |err| log::warn!("Audio input error: {}", err),
                None,
            ),
            cpal::SampleFormat::I16 => dev.build_input_stream(
                &stream_cfg,
                move |data: &[i16], _| {
                    let frames = data.len() / channels;
                    for f in 0..frames {
                        let l = data[f * channels] as f32 / 32768.0;
                        let r = if channels > 1 {
                            data[f * channels + 1] as f32 / 32768.0
                        } else {
                            l
                        };
                        while frac_acc < 1.0 {
                            let t = frac_acc as f32;
                            let ol = prev_lr.0 + (l - prev_lr.0) * t;
                            let or = prev_lr.1 + (r - prev_lr.1) * t;
                            let _ = hw_audio_tx.push((ol, or));
                            frac_acc += step;
                        }
                        frac_acc -= 1.0;
                        prev_lr = (l, r);
                    }
                },
                |err| log::warn!("Audio input error: {}", err),
                None,
            ),
            _ => return (None, format!("{} (Unsupported Format)", in_name)),
        };

        match stream_res {
            Ok(s) => {
                let _ = s.play();
                (Some(s), in_name)
            }
            Err(_) => (None, format!("{} (Busy)", in_name)),
        }
    }

    pub fn send(&mut self, cmd: EngineCommand) {
        let _ = self.cmd_tx.push(cmd);
    }

    pub fn set_input_gain_db(&mut self, db: f32) {
        self.input_gain_db = db.clamp(-24.0, 24.0);
        let _ = self.cmd_tx.push(EngineCommand::SetInputGainDb(self.input_gain_db));
    }

    pub fn sync_project(&mut self, project: &Project) {
        let sr = self.sample_rate as f32;
        for track in &project.tracks {
            for fx in &track.fx_chain {
                self.register_if_new(fx, sr);
            }
            for clip in &track.clips {
                for fx in &clip.clip_fx {
                    self.register_if_new(fx, sr);
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
            self.register_if_new(fx, sr);
        }
        let _ = self
            .cmd_tx
            .push(EngineCommand::SyncProject(Box::new(project.clone())));
    }

    fn register_if_new(&mut self, fx: &ferric_model::FxSlot, sr: f32) {
        // Also re-register SamplePlayer when its buffer changes!
        if !self.known_fx_ids.contains(&fx.id) {
            self.known_fx_ids.push(fx.id);
            let inst = if fx.kind == FxKind::ExternalClap {
                if let (Some(path), Some(pid)) = (&fx.clap_plugin_path, &fx.clap_plugin_id) {
                    match HostedClapInstance::load(path, pid, sr as f64, 4096) {
                        Ok(hosted) => FxRuntimeInstance::new_external_clap(
                            fx.id,
                            Box::new(hosted),
                            4096,
                        ),
                        Err(e) => {
                            log::warn!("Failed to load CLAP {}: {}", pid, e);
                            FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096)
                        }
                    }
                } else {
                    FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096)
                }
            } else {
                let mut i = FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
                if fx.kind == FxKind::SamplePlayer {
                    i.set_sampler_data(fx.sampler_buffer.clone(), fx.sampler_slices.clone());
                }
                i
            };
            let _ = self.cmd_tx.push(EngineCommand::RegisterFx(Box::new(inst)));
        }
    }

    pub fn force_reload_sampler(&mut self, fx: &ferric_model::FxSlot) {
        let mut inst =
            FxRuntimeInstance::new(fx.id, FxKind::SamplePlayer, self.sample_rate as f32, 4096);
        inst.set_sampler_data(fx.sampler_buffer.clone(), fx.sampler_slices.clone());
        let _ = self.cmd_tx.push(EngineCommand::RegisterFx(Box::new(inst)));
    }

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

        self.live_midi_block.clear();
        self.last_midi_cc = None;

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
                EngineCommand::SetInputGainDb(db) => {
                    self.core.input_gain_lin = db_to_lin(db);
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
                EngineCommand::PreviewBuffer(buf) => {
                    self.preview_buf = Some(buf);
                    self.preview_pos_frames = 0.0;
                }
                EngineCommand::StopPreview => {
                    self.preview_buf = None;
                }
                EngineCommand::LiveMidi {
                    target_track,
                    note,
                    velocity,
                    is_note_on,
                } => {
                    if is_note_on {
                        self.last_midi_note = Some(note);
                    }
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

        // Drain Hardware USB MIDI Controllers (Akai LPK25 notes, Sustain CC64, CCs!)
        while let Ok(pkt) = self.hw_midi_rx.pop() {
            match pkt {
                HwMidiPacket::Note {
                    note,
                    velocity,
                    is_note_on,
                } => {
                    if is_note_on {
                        self.last_midi_note = Some(note);
                    }
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
                HwMidiPacket::ControlChange { channel, cc, value } => {
                    if cc == 64 {
                        // Akai LPK25 Sustain button!
                        self.sustain_release_scratch.clear();
                        self.core
                            .set_sustain_pedal(value >= 64, &mut self.sustain_release_scratch);
                        self.live_midi_block
                            .extend_from_slice(&self.sustain_release_scratch);
                    } else {
                        self.last_midi_cc = Some((channel, cc, value));
                        let _ = self.project.apply_midi_cc(
                            channel,
                            cc,
                            value,
                            self.core.playhead_beat,
                            self.core.is_playing,
                        );
                    }
                }
            }
        }

        let mut offset_frame = 0usize;
        let mut max_in_peak = 0.0_f32;

        while offset_frame < total_frames {
            let chunk = (total_frames - offset_frame).min(self.core.max_block);
            let retro_cap = self.retro_audio_l.len();

            for i in 0..chunk {
                let (il, ir) = self.hw_audio_rx.pop().unwrap_or((0.0, 0.0));
                self.in_l[i] = il;
                self.in_r[i] = ir;
                let pk = (il.abs().max(ir.abs())) * self.core.input_gain_lin;
                max_in_peak = max_in_peak.max(pk);
                if pk > 0.002 {
                    self.has_hw_audio_signal = true;
                }
                self.retro_audio_l[self.retro_audio_pos] = il * self.core.input_gain_lin;
                self.retro_audio_r[self.retro_audio_pos] = ir * self.core.input_gain_lin;
                self.retro_audio_pos = (self.retro_audio_pos + 1) % retro_cap;

                if self.is_recording
                    && self.core.is_playing
                    && self.rec_audio_l.len() < self.rec_audio_l.capacity()
                {
                    self.rec_audio_l.push(il * self.core.input_gain_lin);
                    self.rec_audio_r.push(ir * self.core.input_gain_lin);
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

            // Mix Browser Sample Audition Preview!
            if let Some(ref pbuf) = self.preview_buf {
                let step = pbuf.sample_rate as f64 / self.core.sample_rate as f64;
                let mut finished = false;
                for i in 0..chunk {
                    if self.preview_pos_frames >= pbuf.len_frames() as f64 {
                        finished = true;
                        break;
                    }
                    let (pl, pr) = pbuf.sample_interpolated(self.preview_pos_frames);
                    self.out_l[i] += pl * 0.7;
                    self.out_r[i] += pr * 0.7;
                    self.preview_pos_frames += step;
                }
                if finished {
                    self.preview_buf = None;
                }
            }

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

        let elapsed_secs = start_inst.elapsed().as_secs_f32();
        let budget_secs = total_frames as f32 / self.core.sample_rate;
        let load = if budget_secs > 1e-6 {
            (elapsed_secs / budget_secs) * 100.0
        } else {
            0.0
        };
        self.smoothed_cpu = self.smoothed_cpu * 0.88 + load * 0.12;

        if self.last_midi_cc.is_some()
            || self.samples_since_telem >= (self.core.sample_rate as usize / 60).max(256)
        {
            self.samples_since_telem = 0;
            let _ = self.telem_tx.push(EngineTelemetry {
                is_playing: self.core.is_playing,
                is_recording: self.is_recording,
                playhead_beat: self.core.playhead_beat,
                sample_rate: self.core.sample_rate as u32,
                cpu_load_percent: self.smoothed_cpu,
                input_peak_db: lin_to_db(max_in_peak),
                total_pdc_samples: self.core.total_pdc_samples as u32,
                sustain_down: self.core.sustain_pedal_down,
                last_midi_note: self.last_midi_note,
                last_midi_cc: self.last_midi_cc,
                master: self.core.last_master_meter,
                tuner: self.core.best_tuner_reading(),
                vocal_pitch: self.core.last_vocal_pitch,
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
            let span_secs =
                (current_frame.saturating_sub(first_frame)) as f64 / sample_rate as f64;
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
    fn offline_master_bounce_and_track_freeze_render_cleanly() {
        let proj = build_demo_project(48_000);
        let tmp = std::env::temp_dir().join("ferric_test_bounce_v2.wav");
        let report = render_master_offline(&proj, &tmp, 48_000, BitDepth::Pcm16, 0.0, 4.0)
            .expect("bounce succeeds");
        assert!(report.peak_db > -45.0 && report.peak_db <= 0.1);
        let decoded = import_audio_file(&tmp, 48_000).expect("symphonia/wav decode");
        assert!(decoded.len_frames() > 48_000);
        let _ = std::fs::remove_file(&tmp);

        // Test 1-click Reversible Track Freeze
        let synth_tid = proj.tracks[4].id;
        let frozen_buf =
            render_track_freeze_buffer(&proj, synth_tid, 48_000).expect("freeze buffer");
        assert!(frozen_buf.len_frames() > 48_000);
    }
}
