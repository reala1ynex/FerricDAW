//! IPC protocol and host-side client for the `ferric-plugin-bridge` worker process.
//! Audio blocks travel through a pre-sized Windows file mapping; control messages use
//! length-prefixed JSON on a named pipe (allocation-free on the RT path after setup).

mod client;
pub mod protocol;
mod shm;

#[cfg(windows)]
pub mod win_pipe;

pub use client::{
    default_worker_exe_path, BridgeHostClient, BridgeHostConfig, BridgeWorkerDeath,
};
pub use protocol::{
    BridgeMidiEvent, BridgeRequest, BridgeResponse, BRIDGE_IO_VERSION, DEFAULT_MAX_FRAMES,
};
pub use shm::{AudioShmLayout, AudioShmRegion, ShmHeader};