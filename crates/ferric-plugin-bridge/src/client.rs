use crate::protocol::{
    decode_response_payload, encode_process_block_into, encode_request, BridgeMidiEvent,
    BridgeRequest, BridgeResponse, ProcessBlockRef, DEFAULT_MAX_FRAMES,
};
use crate::shm::{AudioShmLayout, AudioShmRegion};
use ferric_dsp::MidiEvent;
use ferric_model::ExternalPluginFormat;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// How long the worker gets to connect to the pipe after it is started.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long loading the plugin inside the worker may take.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// Minimum time one audio block may take before the worker is considered hung.
pub const PROCESS_TIMEOUT_MIN: Duration = Duration::from_millis(500);
/// Time allowed for the Shutdown round trip and for the process to exit.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(windows)]
use crate::win_pipe::{create_shm_mapping, NamedPipeEndpoint, ShmMapping};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::System::Pipes::CreateNamedPipeW;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
#[cfg(windows)]
use windows_sys::Win32::System::Pipes::{PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT};

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
    /// Reused request buffer so audio blocks are encoded without allocating.
    req_buf: Vec<u8>,
    sample_rate: f64,
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
            let max_frames = max_frames.clamp(256, DEFAULT_MAX_FRAMES);
            let layout = AudioShmLayout::new(max_frames);

            let pipe_wide: Vec<u16> = pipe_name
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let pipe_handle = unsafe {
                CreateNamedPipeW(
                    pipe_wide.as_ptr(),
                    // Overlapped so every host-side wait has a deadline (audit H5).
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
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

            let mut pipe = match NamedPipeEndpoint::from_overlapped_server(pipe_handle) {
                Ok(p) => p,
                Err(e) => {
                    unsafe {
                        CloseHandle(pipe_handle);
                    }
                    return Err(format!("CreateEventW: {e}"));
                }
            };

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

            {
                let mut alive = || matches!(child.try_wait(), Ok(None));
                if let Err(err) = pipe.accept_timeout(CONNECT_TIMEOUT, &mut alive) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("plugin worker did not connect: {err}"));
                }
            }

            let mut client = Self {
                child,
                pipe,
                _shm_map: shm_map,
                shm,
                max_frames,
                midi_scratch: Vec::with_capacity(256),
                req_buf: Vec::with_capacity(16 * 1024),
                sample_rate,
                alive: true,
                plugin_path: plugin_path.to_path_buf(),
            };

            let load = BridgeRequest::LoadPlugin {
                format,
                path: plugin_path.to_string_lossy().into_owned(),
                plugin_id: plugin_id.to_string(),
                state_b64: state_b64.map(|s| s.to_string()),
                sample_rate,
                max_frames,
            };
            let result = client
                .send_request(&load, LOAD_TIMEOUT)
                .and_then(|_| client.recv_response(LOAD_TIMEOUT));
            match result {
                Ok(BridgeResponse::Loaded { .. }) => Ok(client),
                Ok(BridgeResponse::Error { message }) => {
                    client.kill_worker();
                    Err(message)
                }
                Ok(other) => {
                    client.kill_worker();
                    Err(format!("unexpected load response: {other:?}"))
                }
                Err(e) => {
                    client.kill_worker();
                    Err(format!("plugin worker failed to load the plugin: {e}"))
                }
            }
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
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
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
        // Generous per-block deadline: a hung worker must never stall the
        // audio thread indefinitely, but slow first blocks are tolerated.
        let block_time = Duration::from_secs_f64(frames as f64 / self.sample_rate.max(8_000.0));
        let timeout = PROCESS_TIMEOUT_MIN.max(block_time * 8);

        let encoded = encode_process_block_into(
            &mut self.req_buf,
            &ProcessBlockRef {
                frames,
                bpm,
                params,
                midi: &self.midi_scratch,
            },
        );
        let ok = encoded.is_ok() && self.exchange_processed(timeout);
        if ok {
            self.shm.copy_output_to_lr(left, right);
            Ok(())
        } else {
            // Hung or crashed: kill the worker so it cannot answer late and
            // desynchronise the protocol; the slot goes silent.
            self.kill_worker();
            left.fill(0.0);
            right.fill(0.0);
            Err(BridgeWorkerDeath)
        }
    }

    #[cfg(windows)]
    fn exchange_processed(&mut self, timeout: Duration) -> bool {
        let child = &mut self.child;
        let mut alive = || matches!(child.try_wait(), Ok(None));
        if self
            .pipe
            .write_frame_timeout(&self.req_buf, timeout, &mut alive)
            .is_err()
        {
            return false;
        }
        match self.pipe.read_payload_timeout(timeout, &mut alive) {
            Ok(payload) => matches!(decode_response_payload(payload), Ok(BridgeResponse::Processed)),
            Err(_) => false,
        }
    }

    #[cfg(not(windows))]
    fn exchange_processed(&mut self, _timeout: Duration) -> bool {
        false
    }

    fn send_request(&mut self, req: &BridgeRequest, timeout: Duration) -> Result<(), String> {
        let frame = encode_request(req)?;
        #[cfg(windows)]
        {
            let child = &mut self.child;
            let mut alive = || matches!(child.try_wait(), Ok(None));
            self.pipe
                .write_frame_timeout(&frame, timeout, &mut alive)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(windows))]
        let _ = (frame, timeout);
        Ok(())
    }

    fn recv_response(&mut self, timeout: Duration) -> Result<BridgeResponse, String> {
        #[cfg(windows)]
        {
            let child = &mut self.child;
            let mut alive = || matches!(child.try_wait(), Ok(None));
            let payload = self
                .pipe
                .read_payload_timeout(timeout, &mut alive)
                .map_err(|e| e.to_string())?;
            decode_response_payload(payload)
        }
        #[cfg(not(windows))]
        {
            let _ = timeout;
            Err("unsupported".into())
        }
    }

    /// Ask the worker to exit; kill it if it does not within the timeouts.
    /// Never blocks longer than about `2 * SHUTDOWN_TIMEOUT`.
    pub fn shutdown(mut self) {
        if self.alive && self.is_alive() {
            let _ = self
                .send_request(&BridgeRequest::Shutdown, SHUTDOWN_TIMEOUT)
                .and_then(|_| self.recv_response(SHUTDOWN_TIMEOUT));
            let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
            while Instant::now() < deadline {
                if !matches!(self.child.try_wait(), Ok(None)) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.kill_worker();
    }

    /// Test helper: spawn the worker with a plugin that never returns from
    /// processing, to exercise the hung-worker timeout.
    pub fn spawn_test_stall(
        worker_exe: &Path,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Self, String> {
        Self::spawn_and_connect(
            worker_exe,
            Path::new("__test_stall__"),
            ExternalPluginFormat::Clap,
            "test.stall",
            None,
            sample_rate,
            max_frames,
        )
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