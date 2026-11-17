//! Out-of-process external plugin adapter (Windows IPC via `ferric-plugin-bridge`).

use crate::plugin_window::PluginQuarantine;
use ferric_dsp::{db_to_lin, ExternalPluginAdapter, MidiEvent};
use ferric_model::ExternalPluginFormat;
use ferric_plugin_bridge::{default_worker_exe_path, BridgeHostClient, BridgeWorkerDeath};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

static PENDING_CRASH_QUARANTINE: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();

fn pending_crashes() -> &'static Mutex<Vec<PathBuf>> {
    PENDING_CRASH_QUARANTINE.get_or_init(|| Mutex::new(Vec::new()))
}

/// UI-thread hook: apply worker-crash paths recorded from the audio thread.
pub fn drain_pending_plugin_quarantine(quarantine: &mut PluginQuarantine) {
    let Ok(mut pending) = pending_crashes().lock() else {
        return;
    };
    for path in pending.drain(..) {
        quarantine.mark_suspect(path.as_path());
    }
}

pub struct SandboxedExternalPlugin {
    client: Arc<Mutex<BridgeHostClient>>,
    plugin_path: PathBuf,
    dead: bool,
}

impl SandboxedExternalPlugin {
    pub fn spawn(
        plugin_path: PathBuf,
        format: ExternalPluginFormat,
        plugin_id: &str,
        state_b64: Option<&str>,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Self, String> {
        let worker = default_worker_exe_path();
        let client = BridgeHostClient::spawn_and_connect(
            &worker,
            &plugin_path,
            format,
            plugin_id,
            state_b64,
            sample_rate,
            max_frames,
        )?;

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            plugin_path,
            dead: false,
        })
    }

    fn on_worker_death(&mut self) {
        if self.dead {
            return;
        }
        self.dead = true;
        if let Ok(mut pending) = pending_crashes().lock() {
            pending.push(self.plugin_path.clone());
        }
        log::warn!(
            "Sandboxed plugin worker died — quarantining {}",
            self.plugin_path.display()
        );
    }

    pub fn plugin_path(&self) -> &Path {
        &self.plugin_path
    }
}

impl ExternalPluginAdapter for SandboxedExternalPlugin {
    fn latency_samples(&self) -> usize {
        if let Ok(guard) = self.client.lock() {
            guard.latency_samples() as usize
        } else {
            0
        }
    }

    fn reset(&mut self) {
        // Worker resets on load; optional future IPC Reset message.
    }

    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        bpm: f64,
        midi_events: &[MidiEvent],
    ) {
        if self.dead {
            left.fill(0.0);
            right.fill(0.0);
            return;
        }

        let trim_db = params.get(4).copied().unwrap_or(0.0);
        let trim = db_to_lin(trim_db);

        // Never wait for the UI thread on the audio thread: if the client is
        // momentarily busy, output silence for this block.
        let result = match self.client.try_lock() {
            Ok(mut guard) => guard.process_block(left, right, params, bpm, midi_events),
            Err(std::sync::TryLockError::WouldBlock) => {
                left.fill(0.0);
                right.fill(0.0);
                return;
            }
            Err(std::sync::TryLockError::Poisoned(_)) => Err(BridgeWorkerDeath),
        };

        if result.is_err() {
            left.fill(0.0);
            right.fill(0.0);
            self.on_worker_death();
            return;
        }

        if trim != 1.0 {
            for s in left.iter_mut() {
                *s *= trim;
            }
            for s in right.iter_mut() {
                *s *= trim;
            }
        }
    }
}

impl Drop for SandboxedExternalPlugin {
    fn drop(&mut self) {
        if let Ok(client) = Arc::try_unwrap(self.client.clone()) {
            if let Ok(guard) = client.into_inner() {
                guard.shutdown();
            }
        }
    }
}

/// Test helper: detects worker death without panicking.
pub fn process_sandbox_or_silence(
    client: &mut BridgeHostClient,
    left: &mut [f32],
    right: &mut [f32],
) -> bool {
    let n = left.len().min(right.len());
    for i in 0..n {
        left[i] = 0.25;
        right[i] = -0.25;
    }
    match client.process_block(left, right, &[], 120.0, &[]) {
        Ok(()) => true,
        Err(BridgeWorkerDeath) => false,
    }
}