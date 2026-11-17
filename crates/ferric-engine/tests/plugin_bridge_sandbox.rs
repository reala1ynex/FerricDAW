#![cfg(windows)]

use ferric_engine::plugin_bridge::process_sandbox_or_silence;
use ferric_plugin_bridge::BridgeHostClient;
use std::path::PathBuf;

#[test]
fn sandbox_worker_crash_does_not_panic_host() {
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_ferric-plugin-bridge"));
    let mut client =
        BridgeHostClient::spawn_test_passthrough(&worker, 48_000.0, 512).expect("spawn worker");

    let mut left = vec![0.0f32; 128];
    let mut right = vec![0.0f32; 128];
    assert!(process_sandbox_or_silence(&mut client, &mut left, &mut right));

    // Identity passthrough echo.
    assert!(left[0].abs() > 0.01);

    client.kill_worker();

    left.fill(1.0);
    right.fill(1.0);
    let still_alive = process_sandbox_or_silence(&mut client, &mut left, &mut right);
    assert!(!still_alive);
    assert!(left.iter().all(|s| *s == 0.0));
    assert!(right.iter().all(|s| *s == 0.0));
}
#[test]
fn hung_worker_times_out_instead_of_blocking_the_audio_thread() {
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_ferric-plugin-bridge"));
    let mut client =
        BridgeHostClient::spawn_test_stall(&worker, 48_000.0, 512).expect("spawn worker");
    let mut left = vec![0.5f32; 128];
    let mut right = vec![0.5f32; 128];
    let started = std::time::Instant::now();
    let ok = process_sandbox_or_silence(&mut client, &mut left, &mut right);
    let waited = started.elapsed();
    assert!(!ok, "a hung worker must be reported as dead");
    assert!(
        waited < std::time::Duration::from_secs(3),
        "process_block blocked for {waited:?}"
    );
    assert!(left.iter().all(|s| *s == 0.0));
    assert!(!client.is_alive(), "the hung worker must be killed");
    // Shutdown of an already-killed worker returns promptly.
    let started = std::time::Instant::now();
    client.shutdown();
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn shutdown_of_healthy_worker_is_bounded() {
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_ferric-plugin-bridge"));
    let client =
        BridgeHostClient::spawn_test_passthrough(&worker, 48_000.0, 512).expect("spawn worker");
    let started = std::time::Instant::now();
    client.shutdown();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
