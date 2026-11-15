use crate::protocol::{
    decode_response, encode_request, BridgeMidiEvent, BridgeRequest, BridgeResponse,
    DEFAULT_MAX_FRAMES,
};
use crate::shm::{AudioShmLayout, AudioShmRegion};
use ferric_dsp::MidiEvent;
use ferric_model::ExternalPluginFormat;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use uuid::Uuid;

#[cfg(windows)]
use crate::win_pipe::{create_shm_mapping, NamedPipeEndpoint, ShmMapping};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::System::Pipes::CreateNamedPipeW;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};

#[derive(Debug)]
pub struct BridgeWorkerDeath;

pub struct BridgeHostConfig {
    pub worker_exe: PathBuf,
    pub pipe_name: String,
    pub shm_name: String,
    pub max_frames: u32,
    pub sample_rate: f64,
}

// SAFETY: Owned exclusively by the audio thread (`Arc<Mutex<..>>`); handles are not shared across threads.
unsafe impl Send for BridgeHostClient {}

pub struct BridgeHostClient {
    child: Child,
    #[cfg(windows)]
    pipe: NamedPipeEndpoint,
    #[cfg(windows)]
    _shm_map: ShmMapping,
    shm: AudioShmRegion,
    max_frames: u32,
    midi_scratch: Vec<BridgeMidiEvent>,
    alive: bool,
    plugin_path: PathBuf,
}

impl BridgeHostClient {
    pub fn spawn_and_connect(
        worker_exe: &Path,
        plugin_path: &Path,
        format: ExternalPluginFormat,
        plugin_id: &str,
        state_b64: Option<&str>,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Self, String> {
        #[cfg(not(windows))]
        {
            let _ = (
                worker_exe,
                plugin_path,
                format,
                plugin_id,
                state_b64,
                sample_rate,
                max_frames,
            );
            return Err("plugin bridge requires Windows".into());
        }

        #[cfg(windows)]
        {
            let session = Uuid::new_v4();
            let pipe_name = format!(r"\\.\pipe\ferric-plugin-bridge-{}", session);
            let shm_name = format!("Local\\ferric-plugin-bridge-{}", session);
            let max_frames = max_frames.max(256).min(DEFAULT_MAX_FRAMES);
            let layout = AudioShmLayout::new(max_frames);

            let pipe_wide: Vec<u16> = pipe_name
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let pipe_handle = unsafe {
                CreateNamedPipeW(
                    pipe_wide.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    1,
                    65536,
                    65536,
                    0,
                    std::ptr::null_mut(),
                )
            };
            if pipe_handle == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "CreateNamedPipeW failed: {}",
                    io::Error::last_os_error()
                ));
            }

            let shm_map = create_shm_mapping(&shm_name, layout.byte_len)
                .map_err(|e| format!("CreateFileMappingW: {e}"))?;
            let shm = unsafe { AudioShmRegion::new_host(layout, shm_map.view) };
            shm.write_header();

            let mut child = Command::new(worker_exe)
                .arg("--pipe")
                .arg(&pipe_name)
                .arg("--shm")
                .arg(&shm_name)
                .arg("--max-frames")
                .arg(max_frames.to_string())
                .arg("--sample-rate")
                .arg(sample_rate.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("failed to spawn ferric-plugin-bridge: {e}"))?;

            let connected = unsafe { ConnectNamedPipe(pipe_handle, std::ptr::null_mut()) };
            if connected == 0 {
                let err = io::Error::last_os_error();
                let raw = err.raw_os_error().unwrap_or(0);
                // ERROR_PIPE_CONNECTED (535) is success.
                if raw != 535 {
                    let _ = child.kill();
                    unsafe {
                        CloseHandle(pipe_handle);
                    }
                    return Err(format!("ConnectNamedPipe: {err}"));
                }
            }

            let pipe = NamedPipeEndpoint::from_connected_server(pipe_handle);

            let mut client = Self {
                child,
                pipe,
                _shm_map: shm_map,
                shm,
                max_frames,
                midi_scratch: Vec::with_capacity(64),
                alive: true,
                plugin_path: plugin_path.to_path_buf(),
            };

            client.send_request(&BridgeRequest::LoadPlugin {
                format,
                path: plugin_path.to_string_lossy().into_owned(),
                plugin_id: plugin_id.to_string(),
                state_b64: state_b64.map(|s| s.to_string()),
                sample_rate,
                max_frames,
            })?;
            match client.recv_response()? {
                BridgeResponse::Loaded { .. } => {}
                BridgeResponse::Error { message } => return Err(message),
                other => return Err(format!("unexpected load response: {other:?}")),
            }
            Ok(client)
        }
    }

    pub fn spawn_test_passthrough(
        worker_exe: &Path,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Self, String> {
        Self::spawn_and_connect(
            worker_exe,
            Path::new("__test_passthrough__"),
            ExternalPluginFormat::Clap,
            "test.passthrough",
            None,
            sample_rate,
            max_frames,
        )
    }

    pub fn plugin_path(&self) -> &Path {
        &self.plugin_path
    }

    pub fn latency_samples(&self) -> u32 {
        0
    }

    pub fn is_alive(&mut self) -> bool {
        if !self.alive {
            return false;
        }
        match self.child.try_wait() {
            Ok(Some(_status)) => {
                self.alive = false;
                false
            }
            Ok(None) => true,
            Err(_) => {
                self.alive = false;
                false
            }
        }
    }

    pub fn kill_worker(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.alive = false;
    }

    pub fn process_block(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        bpm: f64,
        midi_events: &[MidiEvent],
    ) -> Result<(), BridgeWorkerDeath> {
        if !self.is_alive() {
            left.fill(0.0);
            right.fill(0.0);
            return Err(BridgeWorkerDeath);
        }

        self.midi_scratch.clear();
        for m in midi_events {
            self.midi_scratch.push(BridgeMidiEvent {
                sample_offset: m.sample_offset as u32,
                note: m.note,
                velocity: m.velocity,
                is_note_on: m.is_note_on,
            });
        }

        self.shm.copy_lr_to_input(left, right);

        let frames = left.len().min(right.len()).min(self.max_frames as usize) as u32;
        let req = BridgeRequest::ProcessBlock {
            frames,
            bpm,
            params: params.to_vec(),
            midi: std::mem::take(&mut self.midi_scratch),
        };

        match self.send_request(&req) {
            Ok(()) => {}
            Err(_) => {
                self.alive = false;
                left.fill(0.0);
                right.fill(0.0);
                return Err(BridgeWorkerDeath);
            }
        }

        match self.recv_response() {
            Ok(BridgeResponse::Processed) => {
                self.shm.copy_output_to_lr(left, right);
                Ok(())
            }
            Ok(BridgeResponse::Error { .. }) | Err(_) => {
                self.alive = false;
                left.fill(0.0);
                right.fill(0.0);
                Err(BridgeWorkerDeath)
            }
            Ok(_) => {
                self.alive = false;
                left.fill(0.0);
                right.fill(0.0);
                Err(BridgeWorkerDeath)
            }
        }
    }

    fn send_request(&mut self, req: &BridgeRequest) -> Result<(), String> {
        let frame = encode_request(req)?;
        #[cfg(windows)]
        self.pipe
            .write_frame(&frame)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn recv_response(&mut self) -> Result<BridgeResponse, String> {
        #[cfg(windows)]
        {
            let frame = self.pipe.read_frame().map_err(|e| e.to_string())?;
            decode_response(&frame)
        }
        #[cfg(not(windows))]
        Err("unsupported".into())
    }

    pub fn shutdown(mut self) {
        let _ = self.send_request(&BridgeRequest::Shutdown);
        let _ = self.recv_response();
        let _ = self.child.wait();
        self.alive = false;
    }
}

/// Resolve the `ferric-plugin-bridge` worker executable next to the current process.
pub fn default_worker_exe_path() -> PathBuf {
    if let Ok(from_env) = std::env::var("FERRIC_PLUGIN_BRIDGE_EXE") {
        return PathBuf::from(from_env);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("ferric-plugin-bridge.exe");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from("ferric-plugin-bridge.exe")
}