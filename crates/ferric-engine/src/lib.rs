pub mod audio_backend;
pub mod clap_host;
pub mod flac_enc;
pub mod graph;
pub mod lossy_enc;
pub mod plugin_bridge;
pub mod plugin_window;
pub mod vst3_host;
pub mod wav_io;

pub use clap_host::{
    inspect_clap_bundle, scan_clap_plugins, ClapGuiAndStateHandle, DiscoveredClapPlugin,
    DiscoveredExternalPlugin, HostedClapInstance, LoadedClapBundle,
};
pub use plugin_bridge::{drain_pending_plugin_quarantine, SandboxedExternalPlugin};
pub use flac_enc::write_flac_stereo_file;
pub use graph::{AudioGraphCore, TrackMeter, MAX_ENGINE_TRACKS};
pub use plugin_window::{FloatingPluginWindow, NativePluginGuiView, PluginQuarantine};
pub use vst3_host::{
    inspect_vst3_binary, scan_vst3_plugins, HostedVst3Instance, LoadedVst3Bundle,
    Vst3GuiAndStateHandle,
};
pub use audio_backend::{
    exclusive_buffer_hint, host_for_driver, list_audio_device_matrix, AudioDeviceInfo,
    AudioDriverKind, HwMultiFrame, ResolvedAudioHost,
};
pub use wav_io::{
    import_audio_file, import_wav_file, load_project_bundle, render_all_stems_offline,
    render_all_stems_with_options, render_master_offline, render_master_with_options,
    render_track_freeze_buffer, save_project_bundle, write_exported_audio_file, write_wav_file,
    write_wav_file_dithered, BitDepth, BounceReport, ExportFormat, MasteringOptions, WavIoError,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ferric_dsp::{
    db_to_lin, lin_to_db, FxRuntimeInstance, MasterMeterSnapshot, MidiEvent, ModulatorRuntime,
    TunerReading, VocalPitchTelemetry,
};
use ferric_model::{
    AudioBuffer, ExternalPluginFormat, FxId, FxKind, InputSource, MidiNote,
    PluginParamDescriptor, Project, TrackId,
};
use midir::{Ignore, MidiInput, MidiInputConnection};
use rtrb::{Consumer, Producer, RingBuffer};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Scans BOTH standard/custom `.clap` AND `.vst3` plugin directories using crash-quarantine protection!
pub fn scan_all_external_plugins(
    extra_dirs: &[PathBuf],
    quarantine: &mut PluginQuarantine,
) -> Vec<DiscoveredExternalPlugin> {
    let mut all = scan_clap_plugins(extra_dirs, quarantine);
    all.extend(scan_vst3_plugins(extra_dirs, quarantine));
    all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    all
}

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
    PitchBend {
        semitones: f32,
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
    SetRecordingOffsetSamples(u32),
    TriggerLoopbackLatencyPing,
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
    pub count_in_beats_remaining: f32,
    pub sample_rate: u32,
    pub buffer_size_frames: u32,
    pub cpu_load_percent: f32,
    pub input_peak_db: f32,
    pub detected_input_bpm: Option<f32>,
    pub measured_loopback_samples: Option<u32>,
    pub ping_in_progress: bool,
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
            count_in_beats_remaining: 0.0,
            sample_rate: 48_000,
            buffer_size_frames: 256,
            cpu_load_percent: 0.0,
            input_peak_db: -96.0,
            detected_input_bpm: None,
            measured_loopback_samples: None,
            ping_in_progress: false,
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
        target_track: Option<TrackId>,
        start_beat: f64,
        length_beats: f64,
        notes: Vec<MidiNote>,
        is_retroactive: bool,
    },
    AudioTake {
        target_track: Option<TrackId>,
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
    hw_audio_rx: Consumer<HwMultiFrame>,
    telem_tx: Producer<EngineTelemetry>,
    garbage_tx: Producer<Box<Project>>,
    capture_req_tx: Producer<CaptureRequest>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    /// Planar multi-channel hardware input (up to 16 ch) for the I/O matrix.
    in_planes: Vec<Vec<f32>>,
    hw_in_channel_count: u8,
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
    rec_resample_track: Option<TrackId>,
    rec_resample_l: Vec<f32>,
    rec_resample_r: Vec<f32>,
    recording_offset_samples: usize,
    ping_state: LoopbackPingDetector,
    actual_buffer_frames: u32,
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
        target_track: Option<TrackId>,
        start_beat: f64,
        end_beat: f64,
        events: Vec<StampedMidi>,
    },
    FinishedLiveAudio {
        target_track: Option<TrackId>,
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

/// Prefer real Speakers over Realtek "Headphones" (combo-jack trap when DS Line-In is plugged).
fn prefer_speakers_output_device(host: &cpal::Host) -> Option<cpal::Device> {
    let devices: Vec<cpal::Device> = host.output_devices().ok()?.collect();
    let score = |name: &str| -> i32 {
        let n = name.to_lowercase();
        if n.contains("headphone") || n.contains("headset") {
            return -100;
        }
        if n.contains("steam") || n.contains("nvidia") || n.contains("broadcast") {
            return -50;
        }
        if n.contains("speaker") {
            return 100;
        }
        if n.contains("realtek") {
            return 40;
        }
        0
    };
    let mut best: Option<(i32, cpal::Device)> = None;
    for d in devices {
        let Ok(name) = d.name() else { continue };
        let s = score(&name);
        match &best {
            None => best = Some((s, d)),
            Some((bs, _)) if s > *bs => best = Some((s, d)),
            _ => {}
        }
    }
    best.map(|(_, d)| d).or_else(|| host.default_output_device())
}

/// True when the active output looks like Headphones (combo-jack mute risk with DS Line-In).
/// Pure string check — never enumerate devices (was called every UI frame and tanked FPS).
pub fn realtek_combo_jack_risk(output_name: &str, _input_name: &str) -> bool {
    output_name.to_lowercase().contains("headphone")
}

/// Best Speakers device name for forcing playback away from the combo-jack Headphones path.
pub fn preferred_speakers_output_name() -> Option<String> {
    let host = cpal::default_host();
    prefer_speakers_output_device(&host).and_then(|d| d.name().ok())
}

/// Prefer External Microphone / Line In for DS-10 over Steam/NVIDIA virtual mics.
pub fn preferred_line_input_name() -> Option<String> {
    let mut ranked: Vec<(i32, String)> = list_audio_input_devices()
        .into_iter()
        .map(|n| {
            let l = n.to_lowercase();
            let score = if l.contains("external microphone") || l.contains("line in") || l.contains("line-in")
            {
                100
            } else if l.contains("realtek") && l.contains("microphone") && !l.contains("array") {
                80
            } else if l.contains("steam") || l.contains("nvidia") || l.contains("broadcast") {
                -50
            } else if l.contains("array") {
                -20
            } else {
                0
            };
            (score, n)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    ranked.into_iter().map(|(_, n)| n).next()
}

/// Zero-allocation Acoustic / Cable Loopback Round-Trip Latency Ping Detector!
/// Emits a sharp 2.5 ms dual-frequency chirp impulse on Output L/R and measures exact samples until arrival on Input L/R!
#[derive(Debug, Clone, Copy, Default)]
pub struct LoopbackPingDetector {
    pub active: bool,
    pub emit_samples_left: u32,
    pub elapsed_samples: u32,
    pub timeout_samples: u32,
    pub baseline_peak: f32,
    pub measured_samples: Option<u32>,
}

impl LoopbackPingDetector {
    pub fn trigger(&mut self, sample_rate: u32, current_noise_floor: f32) {
        self.active = true;
        self.emit_samples_left = (sample_rate / 400).max(48); // ~2.5ms crisp impulse burst
        self.elapsed_samples = 0;
        self.timeout_samples = sample_rate / 2; // 500ms max window
        self.baseline_peak = current_noise_floor.max(0.015);
    }

    #[inline]
    pub fn step(&mut self, in_sample_abs: f32, sample_rate: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let out_sig = if self.emit_samples_left > 0 {
            let phase = (self.elapsed_samples as f32 * 2400.0 / sample_rate) * std::f32::consts::TAU;
            self.emit_samples_left -= 1;
            phase.sin() * 0.75
        } else {
            0.0
        };

        // Ignore first 8 samples of electrical crosstalk, then detect arrival >= 3.5x baseline!
        if self.elapsed_samples > 8 && in_sample_abs > (self.baseline_peak * 3.5).max(0.06) {
            self.measured_samples = Some(self.elapsed_samples);
            self.active = false;
            return 0.0;
        }

        self.elapsed_samples += 1;
        if self.elapsed_samples >= self.timeout_samples {
            self.active = false;
        }
        out_sig
    }
}

pub struct EngineHandle {
    pub sample_rate: u32,
    pub requested_buffer_size: Option<u32>,
    pub recording_offset_samples: u32,
    pub audio_device_name: String,
    pub audio_input_name: String,
    pub midi_port_names: Vec<String>,
    pub input_gain_db: f32,
    pub audio_driver: AudioDriverKind,
    /// Honest note when Exclusive/ASIO couldn't do what the label historically implied.
    pub driver_status_note: Option<&'static str>,
    pub input_channel_count: u16,
    pub output_channel_count: u16,
    cmd_tx: Producer<EngineCommand>,
    telem_rx: Consumer<EngineTelemetry>,
    garbage_rx: Consumer<Box<Project>>,
    capture_req_rx: Consumer<CaptureRequest>,
    shared_midi_tx: Arc<Mutex<Producer<HwMidiPacket>>>,
    known_fx_ids: Vec<ferric_model::FxId>,
    known_mod_ids: Vec<ferric_model::ModulatorId>,
    plugin_windows: Vec<FloatingPluginWindow>,
    newly_discovered_params: Vec<(FxId, Vec<PluginParamDescriptor>)>,
    pub quarantine: PluginQuarantine,
    pub latest_telemetry: EngineTelemetry,
    _output_stream: Option<cpal::Stream>,
    _input_stream: Option<cpal::Stream>,
    _midi_conns: Vec<MidiInputConnection<()>>,
}

impl EngineHandle {
    pub fn start(initial_project: &Project) -> Self {
        Self::start_with_config(
            initial_project,
            None,
            None,
            None,
            0,
            AudioDriverKind::WasapiShared,
        )
    }

    pub fn start_with_devices(
        initial_project: &Project,
        preferred_out: Option<&str>,
        preferred_in: Option<&str>,
    ) -> Self {
        Self::start_with_config(
            initial_project,
            preferred_out,
            preferred_in,
            None,
            0,
            AudioDriverKind::WasapiShared,
        )
    }

    pub fn start_with_config(
        initial_project: &Project,
        preferred_out: Option<&str>,
        preferred_in: Option<&str>,
        preferred_buffer_size: Option<u32>,
        recording_offset_samples: u32,
        audio_driver: AudioDriverKind,
    ) -> Self {
        let ResolvedAudioHost {
            host,
            fallback_note: driver_status_note,
        } = host_for_driver(audio_driver);
        let out_device = preferred_out
            .and_then(|want| {
                host.output_devices()
                    .ok()?
                    .find(|d| d.name().ok().as_deref() == Some(want))
            })
            .or_else(|| prefer_speakers_output_device(&host));

        let (sample_rate, out_config_opt, device_name, output_channel_count) =
            if let Some(ref dev) = out_device {
                let name = dev.name().unwrap_or_else(|_| "Default Audio Output".into());
                match dev.default_output_config() {
                    Ok(cfg) => (cfg.sample_rate().0, Some(cfg.clone()), name, cfg.channels()),
                    Err(_) => (48_000, None, format!("{} (Offline Fallback)", name), 2),
                }
            } else {
                (48_000, None, "No Hardware Output (Offline Mode)".into(), 0)
            };

        let buf_hint = exclusive_buffer_hint(audio_driver, preferred_buffer_size, 64);

        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCommand>::new(512);
        let (telem_tx, telem_rx) = RingBuffer::<EngineTelemetry>::new(64);
        let (garbage_tx, garbage_rx) = RingBuffer::<Box<Project>>::new(64);
        let (hw_midi_tx, hw_midi_rx) = RingBuffer::<HwMidiPacket>::new(2048);
        let (hw_audio_tx, hw_audio_rx) = RingBuffer::<HwMultiFrame>::new(sample_rate as usize);
        let (capture_req_tx, capture_req_rx) = RingBuffer::<CaptureRequest>::new(16);

        let shared_midi_tx = Arc::new(Mutex::new(hw_midi_tx));
        let (midi_conns, midi_port_names) = Self::connect_all_hardware_midi(shared_midi_tx.clone());

        let (input_stream, audio_input_name, input_channel_count) =
            Self::start_hardware_audio_input(
                &host,
                preferred_in,
                sample_rate,
                buf_hint,
                hw_audio_tx,
            );

        let max_block = 4096usize;
        let mut core = AudioGraphCore::new(sample_rate as f32, max_block);
        core.sync_processors_from_project(initial_project);

        // Pre- populate已知 built-in FX IDs; external plugins will be loaded cleanly on first `sync_project`!
        let mut known_fx_ids = Vec::new();
        let mut known_mod_ids = Vec::new();
        collect_builtin_ids(initial_project, &mut known_fx_ids, &mut known_mod_ids);

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
            in_planes: (0..16).map(|_| vec![0.0; max_block]).collect(),
            hw_in_channel_count: input_channel_count.min(16) as u8,
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
            rec_resample_track: None,
            rec_resample_l: Vec::with_capacity(sample_rate as usize * 180),
            rec_resample_r: Vec::with_capacity(sample_rate as usize * 180),
            recording_offset_samples: recording_offset_samples as usize,
            ping_state: LoopbackPingDetector::default(),
            actual_buffer_frames: buf_hint.unwrap_or(256),
            last_midi_note: None,
            last_midi_cc: None,
            samples_since_telem: 0,
            smoothed_cpu: 0.2,
        };

        let output_stream = if let (Some(dev), Some(supported_cfg)) = (out_device, out_config_opt) {
            let channels = supported_cfg.channels() as usize;
            let mut stream_cfg: cpal::StreamConfig = supported_cfg.clone().into();
            if let Some(req_buf) = buf_hint {
                let clamped = match supported_cfg.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => req_buf.clamp(*min, *max),
                    cpal::SupportedBufferSize::Unknown => req_buf,
                };
                stream_cfg.buffer_size = cpal::BufferSize::Fixed(clamped);
            }
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
            requested_buffer_size: buf_hint,
            recording_offset_samples,
            audio_device_name: device_name,
            audio_input_name,
            midi_port_names,
            input_gain_db: 0.0,
            audio_driver,
            driver_status_note,
            input_channel_count,
            output_channel_count,
            cmd_tx,
            telem_rx,
            garbage_rx,
            capture_req_rx,
            shared_midi_tx,
            known_fx_ids,
            known_mod_ids,
            plugin_windows: Vec::new(),
            newly_discovered_params: Vec::new(),
            quarantine: PluginQuarantine::load_default(),
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
                        } else if status == 0xE0 {
                            let raw14 = ((d2 as i32) << 7) | (d1 as i32);
                            let semi = ((raw14 - 8192) as f32 / 8192.0) * 2.0;
                            Some(HwMidiPacket::PitchBend { semitones: semi })
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

    /// Universal Hardware Audio Line-In / Mic Input Stream (supports `F32` & `I16`, mono→16-ch,
    /// AND automatic real-time sample-rate conversion — ideal for Nintendo DS KORG DS-10 3.5mm Line-In!).
    fn start_hardware_audio_input(
        host: &cpal::Host,
        preferred_in: Option<&str>,
        target_sr: u32,
        preferred_buffer_size: Option<u32>,
        mut hw_audio_tx: Producer<HwMultiFrame>,
    ) -> (Option<cpal::Stream>, String, u16) {
        let dev_opt = preferred_in
            .and_then(|want| {
                host.input_devices()
                    .ok()?
                    .find(|d| d.name().ok().as_deref() == Some(want))
            })
            .or_else(|| host.default_input_device());

        let Some(dev) = dev_opt else {
            return (None, "No Audio Input Detected".into(), 0);
        };
        let in_name = dev.name().unwrap_or_else(|_| "Audio Line/Mic In".into());
        let Ok(cfg) = dev.default_input_config() else {
            return (None, format!("{} (Unavailable)", in_name), 0);
        };

        let in_sr = cfg.sample_rate().0.max(8_000);
        let channels = cfg.channels().max(1) as usize;
        let ch_count = channels.min(16) as u16;
        let fmt = cfg.sample_format();
        let buf_range = cfg.buffer_size().clone();
        let mut stream_cfg: cpal::StreamConfig = cfg.into();
        if let Some(req_buf) = preferred_buffer_size {
            let clamped = match buf_range {
                cpal::SupportedBufferSize::Range { min, max } => req_buf.clamp(min, max),
                cpal::SupportedBufferSize::Unknown => req_buf,
            };
            stream_cfg.buffer_size = cpal::BufferSize::Fixed(clamped);
        }
        let step = in_sr as f64 / target_sr.max(8_000) as f64;
        // Same-rate path: push frames directly (fractional SRC at step≈1 caused 1-sample
        // zero-padding pops that got amplified by mic-jack input trim).
        let same_rate = (step - 1.0).abs() < 1.0e-9;

        let stream_res = match fmt {
            cpal::SampleFormat::F32 => {
                let mut frac_acc = 0.0_f64;
                let mut prev = HwMultiFrame::default();
                dev.build_input_stream(
                    &stream_cfg,
                    move |data: &[f32], _| {
                        let frames = data.len() / channels;
                        for f in 0..frames {
                            let mut cur = HwMultiFrame {
                                count: ch_count as u8,
                                channels: [0.0; 16],
                            };
                            for c in 0..ch_count as usize {
                                cur.channels[c] = data[f * channels + c];
                            }
                            if same_rate {
                                let _ = hw_audio_tx.push(cur);
                                continue;
                            }
                            while frac_acc < 1.0 {
                                let t = frac_acc as f32;
                                let mut lerped = HwMultiFrame {
                                    count: cur.count,
                                    channels: [0.0; 16],
                                };
                                for c in 0..cur.count as usize {
                                    lerped.channels[c] = prev.channels[c]
                                        + (cur.channels[c] - prev.channels[c]) * t;
                                }
                                let _ = hw_audio_tx.push(lerped);
                                frac_acc += step;
                            }
                            frac_acc -= 1.0;
                            prev = cur;
                        }
                    },
                    |err| log::warn!("Audio input error: {}", err),
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let mut frac_acc = 0.0_f64;
                let mut prev = HwMultiFrame::default();
                dev.build_input_stream(
                    &stream_cfg,
                    move |data: &[i16], _| {
                        let frames = data.len() / channels;
                        for f in 0..frames {
                            let mut cur = HwMultiFrame {
                                count: ch_count as u8,
                                channels: [0.0; 16],
                            };
                            for c in 0..ch_count as usize {
                                cur.channels[c] = data[f * channels + c] as f32 / 32768.0;
                            }
                            if same_rate {
                                let _ = hw_audio_tx.push(cur);
                                continue;
                            }
                            while frac_acc < 1.0 {
                                let t = frac_acc as f32;
                                let mut lerped = HwMultiFrame {
                                    count: cur.count,
                                    channels: [0.0; 16],
                                };
                                for c in 0..cur.count as usize {
                                    lerped.channels[c] = prev.channels[c]
                                        + (cur.channels[c] - prev.channels[c]) * t;
                                }
                                let _ = hw_audio_tx.push(lerped);
                                frac_acc += step;
                            }
                            frac_acc -= 1.0;
                            prev = cur;
                        }
                    },
                    |err| log::warn!("Audio input error: {}", err),
                    None,
                )
            }
            _ => return (None, format!("{} (Unsupported Format)", in_name), ch_count),
        };

        match stream_res {
            Ok(s) => {
                let _ = s.play();
                (Some(s), in_name, ch_count)
            }
            Err(_) => (None, format!("{} (Busy)", in_name), ch_count),
        }
    }

    pub fn send(&mut self, cmd: EngineCommand) {
        let _ = self.cmd_tx.push(cmd);
    }

    pub fn set_input_gain_db(&mut self, db: f32) {
        self.input_gain_db = db.clamp(-24.0, 36.0);
        let _ = self.cmd_tx.push(EngineCommand::SetInputGainDb(self.input_gain_db));
    }

    pub fn set_recording_offset_samples(&mut self, samples: u32) {
        self.recording_offset_samples = samples.min(48_000);
        let _ = self
            .cmd_tx
            .push(EngineCommand::SetRecordingOffsetSamples(self.recording_offset_samples));
    }

    pub fn trigger_loopback_ping(&mut self) {
        let _ = self.cmd_tx.push(EngineCommand::TriggerLoopbackLatencyPing);
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
        if !self.known_fx_ids.contains(&fx.id) {
            self.known_fx_ids.push(fx.id);
            let inst = if fx.kind == FxKind::ExternalClap {
                self.load_external_plugin_instance(fx, sr)
            } else {
                let mut i = FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
                if fx.sampler_buffer.is_some() {
                    i.set_sampler_data(fx.sampler_buffer.clone(), fx.sampler_slices.clone());
                }
                i
            };
            let _ = self.cmd_tx.push(EngineCommand::RegisterFx(Box::new(inst)));
        }
    }

    fn load_external_plugin_instance(
        &mut self,
        fx: &ferric_model::FxSlot,
        sr: f32,
    ) -> FxRuntimeInstance {
        let Some(ref path) = fx.clap_plugin_path else {
            return FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
        };
        let Some(ref pid) = fx.clap_plugin_id else {
            return FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
        };
        if self.quarantine.is_quarantined(path) {
            log::warn!("Skipping quarantined plugin: {}", path.display());
            return FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096);
        }

        let initial_bytes = fx.decode_plugin_state();
        let title = fx
            .clap_plugin_name
            .clone()
            .unwrap_or_else(|| format!("FerricDAW — {}", pid));

        if fx.sandbox_out_of_process {
            match SandboxedExternalPlugin::spawn(
                path.clone(),
                fx.plugin_format,
                pid,
                fx.plugin_state_b64.as_deref(),
                sr as f64,
                4096,
            ) {
                Ok(sandbox) => {
                    self.quarantine.unmark_clean(path);
                    log::info!(
                        "Loaded {} in out-of-process sandbox",
                        fx.clap_plugin_name.as_deref().unwrap_or(pid)
                    );
                    return FxRuntimeInstance::new_external_clap(
                        fx.id,
                        Box::new(sandbox),
                        4096,
                    );
                }
                Err(e) => {
                    log::warn!(
                        "Out-of-process sandbox failed for {}: {} — trying in-process host",
                        path.display(),
                        e
                    );
                }
            }
        }

        self.quarantine.mark_suspect(path);
        match fx.plugin_format {
            ExternalPluginFormat::Clap => {
                match HostedClapInstance::load_full(
                    path,
                    pid,
                    sr as f64,
                    4096,
                    initial_bytes.as_deref(),
                ) {
                    Ok(bundle) => {
                        self.quarantine.unmark_clean(path);
                        if !bundle.discovered_params.is_empty() {
                            self.newly_discovered_params
                                .push((fx.id, bundle.discovered_params));
                        }
                        self.plugin_windows.retain(|w| w.fx_id != fx.id);
                        self.plugin_windows.push(FloatingPluginWindow::new_headless(
                            fx.id,
                            format!("{} [CLAP]", title),
                            Box::new(bundle.gui_and_state),
                        ));
                        FxRuntimeInstance::new_external_clap(
                            fx.id,
                            Box::new(bundle.audio_processor),
                            4096,
                        )
                    }
                    Err(e) => {
                        log::warn!("Failed to load CLAP {}: {}", pid, e);
                        FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096)
                    }
                }
            }
            ExternalPluginFormat::Vst3 => {
                match HostedVst3Instance::load_full(
                    path,
                    pid,
                    sr as f64,
                    4096,
                    initial_bytes.as_deref(),
                ) {
                    Ok(bundle) => {
                        self.quarantine.unmark_clean(path);
                        if !bundle.discovered_params.is_empty() {
                            self.newly_discovered_params
                                .push((fx.id, bundle.discovered_params));
                        }
                        self.plugin_windows.retain(|w| w.fx_id != fx.id);
                        self.plugin_windows.push(FloatingPluginWindow::new_headless(
                            fx.id,
                            format!("{} [VST3]", title),
                            Box::new(bundle.gui_and_state),
                        ));
                        FxRuntimeInstance::new_external_clap(
                            fx.id,
                            Box::new(bundle.audio_processor),
                            4096,
                        )
                    }
                    Err(e) => {
                        log::warn!("Failed to load VST3 {}: {}", pid, e);
                        FxRuntimeInstance::new(fx.id, fx.kind, sr, 4096)
                    }
                }
            }
        }
    }

    pub fn open_plugin_editor(&mut self, fx_id: FxId) -> Result<(), String> {
        let Some(win) = self.plugin_windows.iter_mut().find(|w| w.fx_id == fx_id) else {
            return Err("Plugin instance is not loaded yet".into());
        };
        win.open_window()
    }

    pub fn close_plugin_editor(&mut self, fx_id: FxId) {
        if let Some(win) = self.plugin_windows.iter_mut().find(|w| w.fx_id == fx_id) {
            win.close_window();
        }
    }

    pub fn is_plugin_editor_open(&self, fx_id: FxId) -> bool {
        self.plugin_windows
            .iter()
            .find(|w| w.fx_id == fx_id)
            .map(|w| w.is_editor_open)
            .unwrap_or(false)
    }

    /// Snapshots live binary patch states (`clap.state` / `IBStream`) from all hosted plugins into `project`!
    pub fn harvest_plugin_states_into_project(&mut self, project: &mut Project) {
        for win in &mut self.plugin_windows {
            if let Some(bytes) = win.view.save_state_bytes() {
                for trk in &mut project.tracks {
                    if let Some(fx) = trk.fx_chain.iter_mut().find(|f| f.id == win.fx_id) {
                        fx.encode_plugin_state(&bytes);
                    }
                }
                if let Some(fx) = project
                    .master
                    .fx_chain
                    .iter_mut()
                    .find(|f| f.id == win.fx_id)
                {
                    fx.encode_plugin_state(&bytes);
                }
            }
        }
    }

    /// Applies any newly discovered `clap.params` / `IEditController` parameter lists onto `project`!
    pub fn apply_discovered_plugin_params(&mut self, project: &mut Project) -> bool {
        if self.newly_discovered_params.is_empty() {
            return false;
        }
        let batch: Vec<_> = self.newly_discovered_params.drain(..).collect();
        let mut changed = false;
        for (fx_id, specs) in batch {
            for trk in &mut project.tracks {
                if let Some(fx) = trk.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                    if fx.dynamic_param_specs.is_empty() {
                        fx.apply_discovered_params(specs.clone());
                        changed = true;
                    }
                }
            }
            if let Some(fx) = project.master.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                if fx.dynamic_param_specs.is_empty() {
                    fx.apply_discovered_params(specs.clone());
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn force_reload_sampler(&mut self, fx: &ferric_model::FxSlot) {
        let mut inst = FxRuntimeInstance::new(fx.id, fx.kind, self.sample_rate as f32, 4096);
        inst.set_sampler_data(fx.sampler_buffer.clone(), fx.sampler_slices.clone());
        let _ = self.cmd_tx.push(EngineCommand::RegisterFx(Box::new(inst)));
    }

    pub fn poll_ui(&mut self, out_captures: &mut Vec<CapturedTakeEvent>) {
        drain_pending_plugin_quarantine(&mut self.quarantine);
        for win in &mut self.plugin_windows {
            win.poll();
        }
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
                    if !self.core.is_playing && self.is_recording && self.project.count_in_bars > 0
                    {
                        self.core.count_in_beats_remaining = (self.project.count_in_bars as f64)
                            * self.project.time_signature.beats_per_bar();
                    }
                    self.core.is_playing = true;
                    if self.is_recording {
                        self.record_start_beat = if self.project.punch_in_enabled {
                            self.project.punch_start_beat.max(self.core.playhead_beat)
                        } else {
                            self.core.playhead_beat
                        };
                        self.rec_midi.clear();
                        self.rec_audio_l.clear();
                        self.rec_audio_r.clear();
                        self.rec_resample_l.clear();
                        self.rec_resample_r.clear();
                        self.rec_resample_track = None;
                    }
                }
                EngineCommand::Stop => {
                    if self.is_recording && self.core.is_playing {
                        self.flush_live_recording();
                    }
                    self.core.is_playing = false;
                    self.core.count_in_beats_remaining = 0.0;
                    self.core.all_notes_off();
                }
                EngineCommand::SetRecord(rec) => {
                    if self.is_recording && !rec && self.core.is_playing {
                        self.flush_live_recording();
                    }
                    self.is_recording = rec;
                    if rec {
                        if !self.core.is_playing && self.project.count_in_bars > 0 {
                            self.core.count_in_beats_remaining = (self.project.count_in_bars as f64)
                                * self.project.time_signature.beats_per_bar();
                        }
                        self.record_start_beat = if self.project.punch_in_enabled {
                            self.project.punch_start_beat.max(self.core.playhead_beat)
                        } else {
                            self.core.playhead_beat
                        };
                        self.rec_midi.clear();
                        self.rec_audio_l.clear();
                        self.rec_audio_r.clear();
                        self.rec_resample_l.clear();
                        self.rec_resample_r.clear();
                        self.rec_resample_track = None;
                    }
                }
                EngineCommand::SeekBeat(beat) => {
                    self.core.playhead_beat = beat.max(0.0);
                    self.core.all_notes_off();
                }
                EngineCommand::SetInputGainDb(db) => {
                    self.core.input_gain_lin = db_to_lin(db);
                }
                EngineCommand::SetRecordingOffsetSamples(samples) => {
                    self.recording_offset_samples = samples as usize;
                }
                EngineCommand::TriggerLoopbackLatencyPing => {
                    self.ping_state.trigger(self.core.sample_rate as u32, 0.02);
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
                        let norm = (value as f32 / 127.0).clamp(0.0, 1.0);
                        match cc {
                            1 => self.core.live_mod_wheel = norm,
                            11 => self.core.live_expression = norm,
                            74 => self.core.live_brightness = (norm - 0.5) * 2.0,
                            _ => {}
                        }
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
                HwMidiPacket::PitchBend { semitones } => {
                    self.core.live_pitch_bend_semi = semitones;
                }
            }
        }

        self.actual_buffer_frames = total_frames as u32;
        let mut offset_frame = 0usize;
        let mut max_in_peak = 0.0_f32;

        while offset_frame < total_frames {
            let chunk = (total_frames - offset_frame).min(self.core.max_block);
            let retro_cap = self.retro_audio_l.len();

            for i in 0..chunk {
                let frame = self.hw_audio_rx.pop().unwrap_or_default();
                let nch = frame.count.max(self.hw_in_channel_count).min(16);
                for c in 0..nch as usize {
                    self.in_planes[c][i] = frame.channels[c];
                }
                let il = frame.get(0);
                let ir = frame.get(1);
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

                if self.is_actively_recording_now()
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

            // Zero-alloc planar HW matrix (up to 8 stereo pairs / 16 ch exposed to tracks).
            let hw_slices: [&[f32]; 16] = [
                &self.in_planes[0][..chunk],
                &self.in_planes[1][..chunk],
                &self.in_planes[2][..chunk],
                &self.in_planes[3][..chunk],
                &self.in_planes[4][..chunk],
                &self.in_planes[5][..chunk],
                &self.in_planes[6][..chunk],
                &self.in_planes[7][..chunk],
                &self.in_planes[8][..chunk],
                &self.in_planes[9][..chunk],
                &self.in_planes[10][..chunk],
                &self.in_planes[11][..chunk],
                &self.in_planes[12][..chunk],
                &self.in_planes[13][..chunk],
                &self.in_planes[14][..chunk],
                &self.in_planes[15][..chunk],
            ];
            let n_planes = self.hw_in_channel_count.max(2).min(16) as usize;

            self.core.render_block(
                &self.project,
                &hw_slices[..n_planes],
                midi_slice,
                &mut self.out_l[..chunk],
                &mut self.out_r[..chunk],
            );

            // Capture live ResampleTrackOutput post-fader audio if any armed track selects it!
            if self.is_actively_recording_now()
                && self.rec_resample_l.len() + chunk <= self.rec_resample_l.capacity()
            {
                if let Some((t_idx, trk)) = self
                    .project
                    .tracks
                    .iter()
                    .take(MAX_ENGINE_TRACKS)
                    .enumerate()
                    .find(|(_, t)| {
                        t.record_arm && t.input_source == InputSource::ResampleTrackOutput
                    })
                {
                    self.rec_resample_track = Some(trk.id);
                    let (rl, rr) = self.core.track_post_fader_slices(t_idx, chunk);
                    self.rec_resample_l.extend_from_slice(rl);
                    self.rec_resample_r.extend_from_slice(rr);
                }
            }

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

            // Auto-Punch-Out commit when crossing punch_end_beat!
            if self.is_recording
                && self.core.is_playing
                && self.project.punch_in_enabled
                && prev_beat < self.project.punch_end_beat
                && self.core.playhead_beat >= self.project.punch_end_beat
            {
                self.flush_live_recording_at(self.project.punch_end_beat);
                self.is_recording = false;
            } else if self.is_recording
                && self.core.is_playing
                && self.core.playhead_beat < prev_beat - 0.5
            {
                self.flush_live_recording_at(self.project.loop_end_beat);
                self.record_start_beat = if self.project.punch_in_enabled {
                    self.project.punch_start_beat
                } else {
                    self.project.loop_start_beat
                };
            }

            for i in 0..chunk {
                let in_abs = self.in_l[i].abs().max(self.in_r[i].abs()) * self.core.input_gain_lin;
                let ping_sig = self.ping_state.step(in_abs, self.core.sample_rate);
                let base = (offset_frame + i) * out_channels;
                output[base] = self.out_l[i] + ping_sig;
                if out_channels > 1 {
                    output[base + 1] = self.out_r[i] + ping_sig;
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
                count_in_beats_remaining: self.core.count_in_beats_remaining as f32,
                sample_rate: self.core.sample_rate as u32,
                buffer_size_frames: self.actual_buffer_frames,
                cpu_load_percent: self.smoothed_cpu,
                input_peak_db: lin_to_db(max_in_peak),
                detected_input_bpm: self.core.input_bpm_detector.detected_bpm,
                measured_loopback_samples: self.ping_state.measured_samples,
                ping_in_progress: self.ping_state.active,
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

    #[inline]
    fn is_actively_recording_now(&self) -> bool {
        if !self.is_recording || !self.core.is_playing || self.core.count_in_beats_remaining > 1e-5
        {
            return false;
        }
        if self.project.punch_in_enabled {
            let b = self.core.playhead_beat;
            b >= self.project.punch_start_beat && b <= self.project.punch_end_beat
        } else {
            true
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

        if self.is_actively_recording_now() && self.rec_midi.len() < self.rec_midi.capacity() {
            self.rec_midi.push(ev);
        }
    }

    fn flush_live_recording(&mut self) {
        self.flush_live_recording_at(self.core.playhead_beat);
    }

    fn flush_live_recording_at(&mut self, end_beat: f64) {
        let safe_end = end_beat.max(self.record_start_beat + 1.0);

        // 1. Flush MIDI Take (simultaneously alongside Audio if both LPK25 & DS-10 were played!)
        if !self.rec_midi.is_empty() {
            let midi_target = self
                .project
                .tracks
                .iter()
                .find(|t| t.record_arm && t.input_source == InputSource::MidiAll)
                .or_else(|| {
                    self.project.tracks.iter().find(|t| {
                        t.record_arm && matches!(t.input_source, InputSource::AudioAndMidi { .. })
                    })
                })
                .map(|t| t.id);

            let events = self.rec_midi.clone();
            self.rec_midi.clear();
            let _ = self.capture_req_tx.push(CaptureRequest::FinishedLiveMidi {
                target_track: midi_target,
                start_beat: self.record_start_beat,
                end_beat: safe_end,
                events,
            });
        }

        // 2. Flush Hardware Line-In / Mic Audio Take (e.g., Nintendo DS KORG DS-10!)
        let audio_target = self
            .project
            .tracks
            .iter()
            .find(|t| {
                t.record_arm
                    && matches!(
                        t.input_source,
                        InputSource::AudioMono { .. } | InputSource::AudioAndMidi { .. }
                    )
            })
            .map(|t| t.id);

        if self.has_hw_audio_signal && audio_target.is_some() && self.rec_audio_l.len() > 2048 {
            // Trim leading round-trip hardware latency (`recording_offset_samples`) so overdubs lock sample-tight to the grid!
            let trim = self
                .recording_offset_samples
                .min(self.rec_audio_l.len().saturating_sub(1024));
            let left = self.rec_audio_l[trim..].to_vec();
            let right = self.rec_audio_r[trim..].to_vec();
            let _ = self.capture_req_tx.push(CaptureRequest::FinishedLiveAudio {
                target_track: audio_target,
                start_beat: self.record_start_beat,
                end_beat: safe_end,
                sample_rate: self.core.sample_rate as u32,
                left,
                right,
            });
        }
        self.rec_audio_l.clear();
        self.rec_audio_r.clear();

        // 3. Flush ResampleTrackOutput Take (internal synth/arp/KAOSS resample!)
        if let Some(res_tid) = self.rec_resample_track.take() {
            if self.rec_resample_l.len() > 2048 {
                let left = self.rec_resample_l.clone();
                let right = self.rec_resample_r.clone();
                let _ = self.capture_req_tx.push(CaptureRequest::FinishedLiveAudio {
                    target_track: Some(res_tid),
                    start_beat: self.record_start_beat,
                    end_beat: safe_end,
                    sample_rate: self.core.sample_rate as u32,
                    left,
                    right,
                });
            }
        }
        self.rec_resample_l.clear();
        self.rec_resample_r.clear();
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
                target_track: None,
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
                target_track: None,
                start_beat: (current_beat - beats).max(0.0),
                length_beats: beats,
                sample_rate,
                left,
                right,
                is_retroactive: true,
            })
        }
        CaptureRequest::FinishedLiveMidi {
            target_track,
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
                target_track,
                start_beat,
                length_beats: (end_beat - start_beat).max(1.0),
                notes,
                is_retroactive: false,
            })
        }
        CaptureRequest::FinishedLiveAudio {
            target_track,
            start_beat,
            end_beat,
            sample_rate,
            left,
            right,
        } => Some(CapturedTakeEvent::AudioTake {
            target_track,
            start_beat,
            length_beats: (end_beat - start_beat).max(1.0),
            sample_rate,
            left,
            right,
            is_retroactive: false,
        }),
    }
}

fn collect_builtin_ids(
    project: &Project,
    fx_ids: &mut Vec<ferric_model::FxId>,
    mod_ids: &mut Vec<ferric_model::ModulatorId>,
) {
    for t in &project.tracks {
        for fx in &t.fx_chain {
            if !fx.is_external_plugin() {
                fx_ids.push(fx.id);
            }
        }
        for c in &t.clips {
            for fx in &c.clip_fx {
                if !fx.is_external_plugin() {
                    fx_ids.push(fx.id);
                }
            }
        }
        for m in &t.modulators {
            mod_ids.push(m.id);
        }
    }
    for fx in &project.master.fx_chain {
        if !fx.is_external_plugin() {
            fx_ids.push(fx.id);
        }
    }
}

pub fn audio_buffer_from_capture(sr: u32, left: Vec<f32>, right: Vec<f32>) -> AudioBuffer {
    AudioBuffer::from_stereo(sr, left, right, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_model::{build_demo_project, build_tutorial_project};

    #[test]
    fn tutorial_night_current_follows_guide_and_writes_demo_wav() {
        let proj = build_tutorial_project(48_000);
        assert!(
            proj.title.contains("Night Current"),
            "tutorial project title"
        );
        assert!(
            (proj.bpm - 128.0).abs() < 0.01,
            "tutorial BPM must be 128"
        );
        assert_eq!(proj.chord_track.len(), 16, "16 bars of chords");
        for name in ["01 Drums", "02 Bass", "03 Pads", "04 Arp", "05 Hook", "06 Counter"] {
            assert!(
                proj.tracks.iter().any(|t| t.name == name),
                "missing tutorial track {name}"
            );
        }
        let pads = proj
            .tracks
            .iter()
            .find(|t| t.name == "03 Pads")
            .expect("pads");
        assert!(
            pads.fx_chain.iter().any(|f| f.sidechain_source.is_some()),
            "pads should sidechain from drums"
        );
        let hook = proj
            .tracks
            .iter()
            .find(|t| t.name == "05 Hook")
            .expect("hook");
        assert!(
            hook.clips.len() >= 2,
            "hook A + hook B clips"
        );

        let demo_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/demo");
        std::fs::create_dir_all(&demo_dir).expect("demo dir");
        let out = demo_dir.join("NightCurrent-Demo.wav");
        let report = render_master_offline(&proj, &out, 48_000, BitDepth::Pcm16, 0.0, 64.0)
            .expect("tutorial bounce");
        assert!(
            report.peak_db > -42.0 && report.peak_db <= 0.2,
            "tutorial bounce too quiet/hot: {} dBFS",
            report.peak_db
        );
        assert!(out.is_file(), "demo wav written");
        let decoded = import_audio_file(&out, 48_000).expect("decode tutorial demo");
        assert!(
            decoded.len_frames() > 48_000 * 20,
            "demo should be ~30s of audio"
        );
    }

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

    #[test]
    fn session_bundle_persists_and_reloads_long_recorded_audio() {
        let mut proj = Project::new_empty("Long Recording Persistence Test");
        let tid = proj.add_track("DS-10 Line-In");
        // 12 seconds @ 48 kHz = 576,000 samples (exceeds 10-second inline limit!)
        let frames = 48_000 * 12;
        let left = vec![0.25_f32; frames];
        let right = vec![-0.25_f32; frames];
        let long_buf = AudioBuffer::from_stereo(48_000, left, right, None);
        proj.find_track_mut(tid)
            .unwrap()
            .clips
            .push(ferric_model::TimelineClip::new_audio(
                "12s DS-10 Jam",
                0.0,
                24.0,
                long_buf,
            ));

        let bundle_dir = std::env::temp_dir().join("ferric_bundle_test_dir");
        let _ = std::fs::remove_dir_all(&bundle_dir);
        std::fs::create_dir_all(&bundle_dir).unwrap();
        let proj_file = bundle_dir.join("ds10_jam.ferric");

        save_project_bundle(&mut proj, &proj_file).expect("save bundle");
        assert!(bundle_dir.join("ds10_jam_Audio").is_dir());

        let reloaded = load_project_bundle(&proj_file, 48_000).expect("load bundle");
        let take = &reloaded.tracks[0].clips[0].takes[0];
        match &take.content {
            ferric_model::TakeContent::Audio { buffer } => {
                assert_eq!(buffer.len_frames(), frames);
            }
            _ => panic!("expected audio take"),
        }
        let _ = std::fs::remove_dir_all(&bundle_dir);
    }

    #[test]
    fn synthetic_clap_plugin_params_state_latency_and_audio_host_end_to_end() {
        use crate::clap_host::*;
        use crate::plugin_window::NativePluginGuiView;
        use ferric_dsp::ExternalPluginAdapter;
        use std::ffi::{c_char, c_void, CStr};

        struct SynthClapData {
            gain: f64,
        }

        unsafe extern "C" fn p_init(_p: *const ClapPlugin) -> bool {
            true
        }
        unsafe extern "C" fn p_destroy(p: *const ClapPlugin) {
            if !(*p).plugin_data.is_null() {
                drop(Box::from_raw((*p).plugin_data as *mut SynthClapData));
            }
            drop(Box::from_raw(p as *mut ClapPlugin));
        }
        unsafe extern "C" fn p_activate(
            _p: *const ClapPlugin,
            _sr: f64,
            _min: u32,
            _max: u32,
        ) -> bool {
            true
        }
        unsafe extern "C" fn p_noop(_p: *const ClapPlugin) {}
        unsafe extern "C" fn p_start(_p: *const ClapPlugin) -> bool {
            true
        }

        unsafe extern "C" fn p_params_count(_p: *const ClapPlugin) -> u32 {
            1
        }
        unsafe extern "C" fn p_params_info(
            _p: *const ClapPlugin,
            idx: u32,
            info: *mut ClapParamInfo,
        ) -> bool {
            if idx != 0 {
                return false;
            }
            (*info).id = 42;
            let nm = b"Drive Gain\0";
            for (i, &b) in nm.iter().enumerate() {
                (*info).name[i] = b as c_char;
            }
            (*info).min_value = 0.0;
            (*info).max_value = 4.0;
            (*info).default_value = 1.5;
            true
        }
        unsafe extern "C" fn p_params_val(
            p: *const ClapPlugin,
            id: u32,
            out: *mut f64,
        ) -> bool {
            if id != 42 {
                return false;
            }
            let d = &*((*p).plugin_data as *const SynthClapData);
            *out = d.gain;
            true
        }

        static PARAMS_EXT: ClapPluginParams = ClapPluginParams {
            count: Some(p_params_count),
            get_info: Some(p_params_info),
            get_value: Some(p_params_val),
            value_to_text: None,
            text_to_value: None,
            flush: None,
        };

        unsafe extern "C" fn p_state_save(
            p: *const ClapPlugin,
            stream: *const ClapOStream,
        ) -> bool {
            let d = &*((*p).plugin_data as *const SynthClapData);
            let bytes = d.gain.to_le_bytes();
            ((*stream).write)(stream, bytes.as_ptr() as *const c_void, 8) == 8
        }
        unsafe extern "C" fn p_state_load(
            p: *const ClapPlugin,
            stream: *const ClapIStream,
        ) -> bool {
            let d = &mut *((*p).plugin_data as *mut SynthClapData);
            let mut bytes = [0u8; 8];
            if ((*stream).read)(stream, bytes.as_mut_ptr() as *mut c_void, 8) == 8 {
                d.gain = f64::from_le_bytes(bytes);
                true
            } else {
                false
            }
        }
        static STATE_EXT: ClapPluginState = ClapPluginState {
            save: Some(p_state_save),
            load: Some(p_state_load),
        };

        unsafe extern "C" fn p_lat_get(_p: *const ClapPlugin) -> u32 {
            128
        }
        static LAT_EXT: ClapPluginLatency = ClapPluginLatency {
            get: Some(p_lat_get),
        };

        unsafe extern "C" fn p_get_ext(
            _p: *const ClapPlugin,
            id: *const c_char,
        ) -> *const c_void {
            let s = CStr::from_ptr(id).to_bytes();
            if s == b"clap.params" {
                &PARAMS_EXT as *const ClapPluginParams as *const c_void
            } else if s == b"clap.state" {
                &STATE_EXT as *const ClapPluginState as *const c_void
            } else if s == b"clap.latency" {
                &LAT_EXT as *const ClapPluginLatency as *const c_void
            } else {
                std::ptr::null()
            }
        }

        unsafe extern "C" fn p_process(p: *const ClapPlugin, proc_ptr: *const ClapProcess) -> i32 {
            let d = &mut *((*p).plugin_data as *mut SynthClapData);
            let pr = &*proc_ptr;
            let in_ev = &*pr.in_events;
            let n_ev = (in_ev.size)(in_ev);
            for i in 0..n_ev {
                let hdr = (in_ev.get)(in_ev, i);
                if !hdr.is_null() && (*hdr).type_ == CLAP_EVENT_PARAM_VALUE {
                    let pev = &*(hdr as *const ClapEventParamValue);
                    if pev.param_id == 42 {
                        d.gain = pev.value;
                    }
                }
            }
            let in_b = &*pr.audio_inputs;
            let out_b = &mut *pr.audio_outputs;
            let in_l = std::slice::from_raw_parts(*in_b.data32, pr.frames_count as usize);
            let out_l = std::slice::from_raw_parts_mut(*out_b.data32, pr.frames_count as usize);
            for f in 0..(pr.frames_count as usize) {
                out_l[f] = in_l[f] * (d.gain as f32);
            }
            1
        }

        unsafe extern "C" fn fac_create(
            _f: *const ClapPluginFactory,
            _h: *const ClapHost,
            _id: *const c_char,
        ) -> *const ClapPlugin {
            let data = Box::into_raw(Box::new(SynthClapData { gain: 1.5 })) as *mut c_void;
            Box::into_raw(Box::new(ClapPlugin {
                desc: std::ptr::null(),
                plugin_data: data,
                init: Some(p_init),
                destroy: Some(p_destroy),
                activate: Some(p_activate),
                deactivate: Some(p_noop),
                start_processing: Some(p_start),
                stop_processing: Some(p_noop),
                reset: Some(p_noop),
                process: Some(p_process),
                get_extension: Some(p_get_ext),
                on_main_thread: Some(p_noop),
            }))
        }

        static FACTORY: ClapPluginFactory = ClapPluginFactory {
            get_plugin_count: None,
            get_plugin_descriptor: None,
            create_plugin: Some(fac_create),
        };

        let mut bundle = unsafe {
            HostedClapInstance::instantiate_from_factory(
                None,
                &FACTORY,
                "ferric.test.clap",
                48_000.0,
                512,
                None,
            )
            .expect("CLAP instantiate")
        };

        assert_eq!(bundle.discovered_params.len(), 1);
        assert_eq!(bundle.discovered_params[0].name, "Drive Gain");
        assert_eq!(bundle.audio_processor.latency_samples(), 128);

        // Process with modulated parameter = 3.0x gain!
        let mut l = vec![0.25_f32; 64];
        let mut r = vec![0.25_f32; 64];
        bundle.audio_processor.process(&mut l, &mut r, &[3.0], 120.0, &[]);
        assert!((l[0] - 0.75).abs() < 1e-4);

        // Save binary `clap.state` and verify round-trip restore into a brand new instance!
        let saved_state = bundle
            .gui_and_state
            .save_state_bytes()
            .expect("save clap.state");
        let restored = unsafe {
            HostedClapInstance::instantiate_from_factory(
                None,
                &FACTORY,
                "ferric.test.clap",
                48_000.0,
                512,
                Some(&saved_state),
            )
            .expect("restore CLAP")
        };
        assert!((restored.discovered_params[0].default - 3.0).abs() < 1e-4);
    }

    #[test]
    fn synthetic_vst3_com_plugin_params_state_latency_and_audio_host_end_to_end() {
        use crate::plugin_window::NativePluginGuiView;
        use crate::vst3_host::*;
        use ferric_dsp::ExternalPluginAdapter;
        use std::ffi::c_void;

        #[repr(C)]
        struct SynthVst3Component {
            comp_vtbl: *const IComponentVtbl,
            proc_vtbl: *const IAudioProcessorVtbl,
            ctrl_vtbl: *const IEditControllerVtbl,
            gain_norm: f64,
        }

        unsafe extern "system" fn comp_qi(
            this: *mut c_void,
            iid: *const TUID,
            obj: *mut *mut c_void,
        ) -> TResult {
            let c = this as *mut SynthVst3Component;
            if *iid == IID_ICOMPONENT || *iid == IID_FUNKNOWN {
                *obj = c as *mut c_void;
                K_RESULT_OK
            } else if *iid == IID_IAUDIOPROCESSOR {
                *obj = &mut (*c).proc_vtbl as *mut *const IAudioProcessorVtbl as *mut c_void;
                K_RESULT_OK
            } else if *iid == IID_IEDITCONTROLLER {
                *obj = &mut (*c).ctrl_vtbl as *mut *const IEditControllerVtbl as *mut c_void;
                K_RESULT_OK
            } else {
                *obj = std::ptr::null_mut();
                K_NO_INTERFACE
            }
        }
        unsafe extern "system" fn dummy_ref(_this: *mut c_void) -> u32 {
            1
        }
        unsafe extern "system" fn comp_init(_this: *mut c_void, _ctx: *mut c_void) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn comp_term(_this: *mut c_void) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn comp_get_cid(_this: *mut c_void, _cid: *mut TUID) -> TResult {
            -1
        }
        unsafe extern "system" fn comp_i32(_this: *mut c_void, _m: i32) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn comp_bus_cnt(_this: *mut c_void, mt: i32, _dir: i32) -> i32 {
            if mt == 0 { 1 } else { 0 }
        }
        unsafe extern "system" fn comp_bus_info(
            _this: *mut c_void,
            _mt: i32,
            _dir: i32,
            _idx: i32,
            _b: *mut BusInfo,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn comp_route(
            _this: *mut c_void,
            _i: *mut c_void,
            _o: *mut c_void,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn comp_act_bus(
            _this: *mut c_void,
            _mt: i32,
            _d: i32,
            _i: i32,
            _s: u8,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn comp_active(_this: *mut c_void, _s: u8) -> TResult {
            K_RESULT_OK
        }

        // IBStream helpers inside synthetic VST3
        #[repr(C)]
        struct BStreamCaller {
            _unk: FUnknownVtbl,
            read: unsafe extern "system" fn(*mut c_void, *mut c_void, i32, *mut i32) -> TResult,
            write: unsafe extern "system" fn(*mut c_void, *mut c_void, i32, *mut i32) -> TResult,
        }

        unsafe extern "system" fn comp_set_state(
            this: *mut c_void,
            stream: *mut c_void,
        ) -> TResult {
            let c = &mut *(this as *mut SynthVst3Component);
            let vtbl = *(stream as *const *const BStreamCaller);
            let mut bytes = [0u8; 8];
            let mut rd = 0i32;
            let _ = ((*vtbl).read)(stream, bytes.as_mut_ptr() as *mut c_void, 8, &mut rd);
            if rd == 8 {
                c.gain_norm = f64::from_le_bytes(bytes);
            }
            K_RESULT_OK
        }
        unsafe extern "system" fn comp_get_state(
            this: *mut c_void,
            stream: *mut c_void,
        ) -> TResult {
            let c = &*(this as *const SynthVst3Component);
            let vtbl = *(stream as *const *const BStreamCaller);
            let mut bytes = c.gain_norm.to_le_bytes();
            let mut wr = 0i32;
            let _ = ((*vtbl).write)(stream, bytes.as_mut_ptr() as *mut c_void, 8, &mut wr);
            K_RESULT_OK
        }

        // Offset from `proc_vtbl` field back to `SynthVst3Component` base (= 1 pointer!)
        unsafe fn from_proc(this: *mut c_void) -> *mut SynthVst3Component {
            (this as *mut usize).sub(1) as *mut SynthVst3Component
        }
        // Offset from `ctrl_vtbl` field back to `SynthVst3Component` base (= 2 pointers!)
        unsafe fn from_ctrl(this: *mut c_void) -> *mut SynthVst3Component {
            (this as *mut usize).sub(2) as *mut SynthVst3Component
        }

        unsafe extern "system" fn proc_arr(
            _t: *mut c_void,
            _i: *mut u64,
            _ni: i32,
            _o: *mut u64,
            _no: i32,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn proc_get_arr(
            _t: *mut c_void,
            _d: i32,
            _i: i32,
            _a: *mut u64,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn proc_lat(_t: *mut c_void) -> u32 {
            64
        }
        unsafe extern "system" fn proc_setup(
            _t: *mut c_void,
            _s: *mut ProcessSetup,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn proc_run(this: *mut c_void, data: *mut ProcessData) -> TResult {
            let comp = &mut *from_proc(this);
            let d = &mut *data;

            // Read incoming parameter changes via COM `IParameterChanges`!
            if !d.input_param_changes.is_null() {
                #[repr(C)]
                struct PcRead {
                    _u: FUnknownVtbl,
                    cnt: unsafe extern "system" fn(*mut c_void) -> i32,
                    get: unsafe extern "system" fn(*mut c_void, i32) -> *mut c_void,
                }
                #[repr(C)]
                struct PvqRead {
                    _u: FUnknownVtbl,
                    id: unsafe extern "system" fn(*mut c_void) -> u32,
                    cnt: unsafe extern "system" fn(*mut c_void) -> i32,
                    pt: unsafe extern "system" fn(*mut c_void, i32, *mut i32, *mut f64) -> TResult,
                }
                let pc_v = *(d.input_param_changes as *const *const PcRead);
                let n = ((*pc_v).cnt)(d.input_param_changes);
                for i in 0..n {
                    let q = ((*pc_v).get)(d.input_param_changes, i);
                    if !q.is_null() {
                        let q_v = *(q as *const *const PvqRead);
                        if ((*q_v).id)(q) == 7 {
                            let mut off = 0;
                            let mut val = 0.0;
                            let _ = ((*q_v).pt)(q, 0, &mut off, &mut val);
                            comp.gain_norm = val;
                        }
                    }
                }
            }

            let in_bus = &*d.inputs;
            let out_bus = &mut *d.outputs;
            let n = d.num_samples as usize;
            let in_l = std::slice::from_raw_parts(*in_bus.channel_buffers32, n);
            let out_l = std::slice::from_raw_parts_mut(*out_bus.channel_buffers32, n);
            for i in 0..n {
                out_l[i] = in_l[i] * (comp.gain_norm as f32 * 2.0);
            }
            K_RESULT_OK
        }

        unsafe extern "system" fn ctrl_pcount(_t: *mut c_void) -> i32 {
            1
        }
        unsafe extern "system" fn ctrl_pinfo(
            _t: *mut c_void,
            _idx: i32,
            info: *mut ParameterInfo,
        ) -> TResult {
            (*info).id = 7;
            for (i, ch) in "VST3 Mix".encode_utf16().enumerate() {
                (*info).title[i] = ch;
            }
            (*info).default_normalized_value = 0.5;
            K_RESULT_OK
        }
        unsafe extern "system" fn ctrl_get_norm(this: *mut c_void, _id: u32) -> f64 {
            (*from_ctrl(this)).gain_norm
        }
        unsafe extern "system" fn ctrl_set_norm(this: *mut c_void, _id: u32, v: f64) -> TResult {
            (*from_ctrl(this)).gain_norm = v;
            K_RESULT_OK
        }
        unsafe extern "system" fn ctrl_str(
            _t: *mut c_void,
            _id: u32,
            _v: f64,
            _s: *mut [u16; 128],
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn ctrl_val(
            _t: *mut c_void,
            _id: u32,
            _s: *const u16,
            _v: *mut f64,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn ctrl_map(_t: *mut c_void, _id: u32, v: f64) -> f64 {
            v
        }
        unsafe extern "system" fn ctrl_view(_t: *mut c_void, _n: *const i8) -> *mut c_void {
            std::ptr::null_mut()
        }

        static COMP_VTBL: IComponentVtbl = IComponentVtbl {
            base: IPluginBaseVtbl {
                unknown: FUnknownVtbl {
                    query_interface: comp_qi,
                    add_ref: dummy_ref,
                    release: dummy_ref,
                },
                initialize: comp_init,
                terminate: comp_term,
            },
            get_controller_class_id: comp_get_cid,
            set_io_mode: comp_i32,
            get_bus_count: comp_bus_cnt,
            get_bus_info: comp_bus_info,
            get_routing_info: comp_route,
            activate_bus: comp_act_bus,
            set_active: comp_active,
            set_state: comp_set_state,
            get_state: comp_get_state,
        };

        static PROC_VTBL: IAudioProcessorVtbl = IAudioProcessorVtbl {
            unknown: FUnknownVtbl {
                query_interface: comp_qi,
                add_ref: dummy_ref,
                release: dummy_ref,
            },
            set_bus_arrangements: proc_arr,
            get_bus_arrangement: proc_get_arr,
            can_process_sample_size: comp_i32,
            get_latency_samples: proc_lat,
            setup_processing: proc_setup,
            set_processing: comp_active,
            process: proc_run,
            get_tail_samples: proc_lat,
        };

        static CTRL_VTBL: IEditControllerVtbl = IEditControllerVtbl {
            base: IPluginBaseVtbl {
                unknown: FUnknownVtbl {
                    query_interface: comp_qi,
                    add_ref: dummy_ref,
                    release: dummy_ref,
                },
                initialize: comp_init,
                terminate: comp_term,
            },
            set_component_state: comp_init,
            set_state: comp_init,
            get_state: comp_init,
            get_parameter_count: ctrl_pcount,
            get_parameter_info: ctrl_pinfo,
            get_param_string_by_value: ctrl_str,
            get_param_value_by_string: ctrl_val,
            normalized_param_to_plain: ctrl_map,
            plain_param_to_normalized: ctrl_map,
            get_param_normalized: ctrl_get_norm,
            set_param_normalized: ctrl_set_norm,
            set_component_handler: comp_init,
            create_view: ctrl_view,
        };

        unsafe extern "system" fn fac_info(_t: *mut c_void, _i: *mut PFactoryInfo) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn fac_cnt(_t: *mut c_void) -> i32 {
            1
        }
        unsafe extern "system" fn fac_cinfo(
            _t: *mut c_void,
            _idx: i32,
            _i: *mut PClassInfo,
        ) -> TResult {
            K_RESULT_OK
        }
        unsafe extern "system" fn fac_create_inst(
            _t: *mut c_void,
            _cid: *const TUID,
            _iid: *const TUID,
            obj: *mut *mut c_void,
        ) -> TResult {
            let inst = Box::into_raw(Box::new(SynthVst3Component {
                comp_vtbl: &COMP_VTBL,
                proc_vtbl: &PROC_VTBL,
                ctrl_vtbl: &CTRL_VTBL,
                gain_norm: 0.5,
            }));
            *obj = inst as *mut c_void;
            K_RESULT_OK
        }

        #[repr(C)]
        struct SynthFactory {
            vtbl: *const IPluginFactoryVtbl,
        }
        static FAC_VTBL: IPluginFactoryVtbl = IPluginFactoryVtbl {
            unknown: FUnknownVtbl {
                query_interface: comp_qi,
                add_ref: dummy_ref,
                release: dummy_ref,
            },
            get_factory_info: fac_info,
            count_classes: fac_cnt,
            get_class_info: fac_cinfo,
            create_instance: fac_create_inst,
        };
        let mut fac = SynthFactory { vtbl: &FAC_VTBL };
        let cid = [1u8; 16];

        let mut bundle = unsafe {
            HostedVst3Instance::instantiate_from_factory(
                None,
                &mut fac as *mut SynthFactory as *mut c_void,
                &cid,
                48_000.0,
                512,
                None,
            )
            .expect("VST3 instantiate")
        };

        assert_eq!(bundle.discovered_params.len(), 1);
        assert_eq!(bundle.discovered_params[0].name, "VST3 Mix");
        assert_eq!(bundle.audio_processor.latency_samples(), 64);

        // Process with VST3 normalized parameter = 0.75 (-> 1.5x gain)
        let mut l = vec![0.4_f32; 64];
        let mut r = vec![0.4_f32; 64];
        bundle.audio_processor.process(&mut l, &mut r, &[0.75], 120.0, &[]);
        assert!((l[0] - 0.60).abs() < 1e-4);

        // Save binary VST3 IBStream state and verify round-trip restore!
        let saved = bundle
            .gui_and_state
            .save_state_bytes()
            .expect("save VST3 IBStream");
        let restored = unsafe {
            HostedVst3Instance::instantiate_from_factory(
                None,
                &mut fac as *mut SynthFactory as *mut c_void,
                &cid,
                48_000.0,
                512,
                Some(&saved),
            )
            .expect("restore VST3")
        };
        assert!((restored.discovered_params[0].default - 0.75).abs() < 1e-4);
    }

    #[test]
    fn lossless_flac_encoder_bit_exact_symphonia_roundtrip_16bit_and_24bit() {
        let sr = 48_000_u32;
        let frames = 10_000usize; // Multi-frame test (4096 + 4096 + 1808 trailing block!)
        let mut orig_l = Vec::with_capacity(frames);
        let mut orig_r = Vec::with_capacity(frames);

        for i in 0..frames {
            let t = i as f32 / sr as f32;
            // Stereo correlated + side signal so both Independent and Mid-Side FLAC coding are exercised!
            let mid = (t * 440.0 * std::f32::consts::TAU).sin() * 0.65;
            let side = (t * 110.0 * std::f32::consts::TAU).cos() * 0.12;
            // Quantize input exactly to 16-bit grid so we can assert BIT-EXACT 0-error lossless roundtrip!
            let ql = (((mid + side) * 32767.0).round() as i16) as f32 / 32768.0;
            let qr = (((mid - side) * 32767.0).round() as i16) as f32 / 32768.0;
            orig_l.push(ql);
            orig_r.push(qr);
        }

        let flac16_path = std::env::temp_dir().join("ferric_m6_lossless_16.flac");
        write_flac_stereo_file(&flac16_path, sr, 16, false, &orig_l, &orig_r)
            .expect("write 16-bit FLAC");
        let decoded16 = import_audio_file(&flac16_path, sr).expect("Symphonia decode 16-bit FLAC");
        assert_eq!(decoded16.len_frames(), frames);
        for i in 0..frames {
            // 16-bit quantizer maps 32767 vs Symphonia f32 32768 denominator within 1 LSB (3.5e-5)
            assert!(
                (decoded16.left[i] - orig_l[i]).abs() < 4e-5,
                "16-bit FLAC mismatch at frame {}: got {} expected {}",
                i,
                decoded16.left[i],
                orig_l[i]
            );
            assert!((decoded16.right[i] - orig_r[i]).abs() < 4e-5);
        }
        let _ = std::fs::remove_file(&flac16_path);

        // Also verify 24-bit Lossless FLAC + TPDF Dither + -14.0 LUFS Mastering Normalization!
        let proj = build_demo_project(sr);
        let flac24_master = std::env::temp_dir().join("ferric_m6_master_14lufs.flac");
        let opts = MasteringOptions {
            format: ExportFormat::Flac,
            sample_rate: sr,
            bit_depth: BitDepth::Pcm24,
            start_beat: 0.0,
            end_beat: 8.0,
            normalize_target_lufs: Some(-14.0),
            true_peak_ceiling_dbtp: -1.0,
            tpdf_dither: true,
        };
        let report =
            render_master_with_options(&proj, &flac24_master, &opts).expect("master -14 LUFS FLAC");
        assert!(
            (report.integrated_lufs - (-14.0)).abs() <= 0.35,
            "Expected integrated loudness near -14.0 LUFS, got {:.2} LUFS",
            report.integrated_lufs
        );
        assert!(
            report.true_peak_dbtp <= -0.90,
            "Expected True Peak <= -0.90 dBTP (target -1.0 dBTP), got {:.2} dBTP",
            report.true_peak_dbtp
        );
        let redecoded =
            import_audio_file(&flac24_master, sr).expect("Symphonia decode 24-bit Master FLAC");
        assert!(redecoded.len_frames() > sr as usize * 3);
        let _ = std::fs::remove_file(&flac24_master);
    }

    #[test]
    fn loopback_ping_detector_measures_exact_roundtrip_latency() {
        let sr = 48_000_u32;
        let simulated_hw_latency = 312_u32; // e.g. 6.50 ms round-trip ADC+DAC cable delay
        let mut ping = LoopbackPingDetector::default();
        ping.trigger(sr, 0.005);

        let mut wire_delay = vec![0.0_f32; simulated_hw_latency as usize];
        let mut pos = 0usize;
        for _ in 0..2000 {
            let delayed_in = wire_delay[pos];
            let emitted_out = ping.step(delayed_in.abs(), sr as f32);
            wire_delay[pos] = emitted_out;
            pos = (pos + 1) % wire_delay.len();
            if !ping.active {
                break;
            }
        }
        let measured = ping.measured_samples.expect("should detect loopback ping");
        // Within 1-2 samples of impulse rise-time above threshold!
        assert!(
            measured >= simulated_hw_latency && measured <= simulated_hw_latency + 4,
            "Expected ~{} samples, measured {}",
            simulated_hw_latency,
            measured
        );
    }
}
