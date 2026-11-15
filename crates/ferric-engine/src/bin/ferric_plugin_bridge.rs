//! Out-of-process CLAP/VST3 worker (`ferric-plugin-bridge.exe`).

use base64::Engine as _;
use ferric_dsp::{ExternalPluginAdapter, MidiEvent};
use ferric_engine::plugin_window::NativePluginGuiView;
use ferric_engine::{ClapGuiAndStateHandle, HostedClapInstance, HostedVst3Instance, Vst3GuiAndStateHandle};
use ferric_model::ExternalPluginFormat;
use ferric_plugin_bridge::protocol::{
    decode_request, encode_response, BridgeRequest, BridgeResponse,
};
use ferric_plugin_bridge::{AudioShmLayout, AudioShmRegion};
use ferric_plugin_bridge::win_pipe::{open_shm_mapping, NamedPipeEndpoint};
use std::env;
use std::path::PathBuf;

enum WorkerPlugin {
    Clap {
        audio: HostedClapInstance,
        state: ClapGuiAndStateHandle,
    },
    Vst3 {
        audio: HostedVst3Instance,
        state: Vst3GuiAndStateHandle,
    },
    TestPassthrough,
}

struct WorkerState {
    plugin: Option<WorkerPlugin>,
    shm: AudioShmRegion,
    max_frames: u32,
    sample_rate: f64,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
}

fn main() {
    env_logger::try_init().ok();
    let args: Vec<String> = env::args().collect();
    let mut pipe_name = None;
    let mut shm_name = None;
    let mut max_frames = 4096u32;
    let mut sample_rate = 48_000.0f64;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pipe" => {
                i += 1;
                pipe_name = args.get(i).cloned();
            }
            "--shm" => {
                i += 1;
                shm_name = args.get(i).cloned();
            }
            "--max-frames" => {
                i += 1;
                max_frames = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(4096);
            }
            "--sample-rate" => {
                i += 1;
                sample_rate = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(48_000.0);
            }
            _ => {}
        }
        i += 1;
    }
    let (Some(pipe_name), Some(shm_name)) = (pipe_name, shm_name) else {
        eprintln!("Usage: ferric-plugin-bridge --pipe NAME --shm NAME [--max-frames N] [--sample-rate HZ]");
        std::process::exit(2);
    };

    let layout = AudioShmLayout::new(max_frames);
    let shm_map = match open_shm_mapping(&shm_name, layout.byte_len) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("open shm: {e}");
            std::process::exit(1);
        }
    };
    let shm = unsafe { AudioShmRegion::from_mapped_ptr(layout, shm_map.view) };

    let mut pipe = match NamedPipeEndpoint::connect_client(&pipe_name) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("connect pipe: {e}");
            std::process::exit(1);
        }
    };

    let blk = max_frames as usize;
    let mut worker = WorkerState {
        plugin: None,
        shm,
        max_frames,
        sample_rate,
        scratch_l: vec![0.0; blk],
        scratch_r: vec![0.0; blk],
    };

    // The mapping handle must live for the process lifetime.
    let _shm_guard = shm_map;

    loop {
        let frame = match pipe.read_frame() {
            Ok(f) => f,
            Err(e) => {
                eprintln!("pipe read: {e}");
                break;
            }
        };
        let req = match decode_request(&frame) {
            Ok(r) => r,
            Err(e) => {
                respond(&mut pipe, BridgeResponse::Error { message: e });
                continue;
            }
        };
        match handle_request(&mut worker, &req) {
            Ok(resp) => respond(&mut pipe, resp),
            Err(msg) => respond(&mut pipe, BridgeResponse::Error { message: msg }),
        }
        if matches!(req, BridgeRequest::Shutdown) {
            break;
        }
    }
}

fn respond(pipe: &mut NamedPipeEndpoint, resp: BridgeResponse) {
    if let Ok(bytes) = encode_response(&resp) {
        let _ = pipe.write_frame(&bytes);
    }
}

fn handle_request(worker: &mut WorkerState, req: &BridgeRequest) -> Result<BridgeResponse, String> {
    match req {
        BridgeRequest::LoadPlugin {
            format,
            path,
            plugin_id,
            state_b64,
            sample_rate,
            max_frames,
        } => load_plugin(worker, *format, path, plugin_id, state_b64.as_deref(), *sample_rate, *max_frames),
        BridgeRequest::ProcessBlock {
            frames,
            bpm,
            params,
            midi,
        } => process_block(worker, *frames, *bpm, params, midi),
        BridgeRequest::GetState => get_state(worker),
        BridgeRequest::SetState { state_b64 } => set_state(worker, state_b64),
        BridgeRequest::Shutdown => Ok(BridgeResponse::Ok),
        BridgeRequest::Heartbeat => Ok(BridgeResponse::Pong),
    }
}

fn load_plugin(
    worker: &mut WorkerState,
    format: ExternalPluginFormat,
    path: &str,
    plugin_id: &str,
    state_b64: Option<&str>,
    sample_rate: f64,
    max_frames: u32,
) -> Result<BridgeResponse, String> {
    worker.sample_rate = sample_rate;
    worker.max_frames = max_frames;
    let blk = max_frames.max(256) as usize;
    worker.scratch_l.resize(blk, 0.0);
    worker.scratch_r.resize(blk, 0.0);

    if path == "__test_passthrough__" {
        worker.plugin = Some(WorkerPlugin::TestPassthrough);
        return Ok(BridgeResponse::Loaded {
            latency_samples: 0,
        });
    }

    let path = PathBuf::from(path);
    let initial = state_b64
        .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok());

    match format {
        ExternalPluginFormat::Clap => {
            let bundle = HostedClapInstance::load_full(
                &path,
                plugin_id,
                sample_rate,
                blk,
                initial.as_deref(),
            )?;
            let latency = bundle.audio_processor.latency_samples() as u32;
            worker.plugin = Some(WorkerPlugin::Clap {
                audio: bundle.audio_processor,
                state: bundle.gui_and_state,
            });
            Ok(BridgeResponse::Loaded { latency_samples: latency })
        }
        ExternalPluginFormat::Vst3 => {
            let bundle = HostedVst3Instance::load_full(
                &path,
                plugin_id,
                sample_rate,
                blk,
                initial.as_deref(),
            )?;
            let latency = bundle.audio_processor.latency_samples() as u32;
            worker.plugin = Some(WorkerPlugin::Vst3 {
                audio: bundle.audio_processor,
                state: bundle.gui_and_state,
            });
            Ok(BridgeResponse::Loaded { latency_samples: latency })
        }
    }
}

fn process_block(
    worker: &mut WorkerState,
    frames: u32,
    bpm: f64,
    params: &[f32],
    midi: &[ferric_plugin_bridge::BridgeMidiEvent],
) -> Result<BridgeResponse, String> {
    let frames = frames as usize;
    if frames == 0 || frames > worker.max_frames as usize {
        return Err("invalid frame count".into());
    }

    let inp = worker.shm.input_stereo_interleaved_mut(frames);
    for i in 0..frames {
        worker.scratch_l[i] = inp[i * 2];
        worker.scratch_r[i] = inp[i * 2 + 1];
    }
    let left = &mut worker.scratch_l[..frames];
    let right = &mut worker.scratch_r[..frames];

    let midi_events: Vec<MidiEvent> = midi
        .iter()
        .map(|m| MidiEvent {
            sample_offset: m.sample_offset as usize,
            note: m.note,
            velocity: m.velocity,
            is_note_on: m.is_note_on,
        })
        .collect();

    match worker.plugin.as_mut() {
        Some(WorkerPlugin::TestPassthrough) => {
            // Echo input → output (identity).
        }
        Some(WorkerPlugin::Clap { audio, .. }) => {
            audio.process(left, right, params, bpm, &midi_events);
        }
        Some(WorkerPlugin::Vst3 { audio, .. }) => {
            audio.process(left, right, params, bpm, &midi_events);
        }
        None => {
            left.fill(0.0);
            right.fill(0.0);
        }
    }

    {
        let out = worker.shm.output_stereo_interleaved_mut(frames);
        for i in 0..frames {
            out[i * 2] = left[i];
            out[i * 2 + 1] = right[i];
        }
    }

    Ok(BridgeResponse::Processed)
}

fn get_state(worker: &mut WorkerState) -> Result<BridgeResponse, String> {
    let bytes = match worker.plugin.as_mut() {
        Some(WorkerPlugin::Clap { state, .. }) => state.save_state_bytes(),
        Some(WorkerPlugin::Vst3 { state, .. }) => state.save_state_bytes(),
        _ => None,
    };
    let state_b64 = bytes
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b))
        .unwrap_or_default();
    Ok(BridgeResponse::State { state_b64 })
}

fn set_state(worker: &mut WorkerState, state_b64: &str) -> Result<BridgeResponse, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(state_b64)
        .map_err(|e| e.to_string())?;
    let ok = match worker.plugin.as_mut() {
        Some(WorkerPlugin::Clap { state, .. }) => state.load_state_bytes(&bytes),
        Some(WorkerPlugin::Vst3 { state, .. }) => state.load_state_bytes(&bytes),
        _ => false,
    };
    if ok {
        Ok(BridgeResponse::Ok)
    } else {
        Err("plugin state load failed".into())
    }
}