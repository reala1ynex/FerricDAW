# Test Registry

**Last updated:** 2026-11-15

The candidate results below include the I-17 and I-18 session-safety fixes and the bounce chrome change. Release-build visual evidence and its limits are recorded in `PROGRESS.md`.

| ID | Suite | Scope | Command / location | Last result | Covers |
|----|-------|-------|--------------------|-------------|--------|
| T-01 | Unit/integration workspace | All crates | `cargo test --workspace` | **PASS** v1.1.7 candidate 2026-11-15 (`last-cargo-test.txt`) — app 6, dsp 7, engine 8+1+1, model 7, bridge 1 | Model, DSP, engine, updater unit tests, save guards |
| T-02 | Updater allowlist/SHA | ferric-app | `reject_non_github_url_and_missing_sha` | **PASS** | I-02, N-03 |
| T-03 | BOM feed parse | ferric-app | `feed_parses_with_bom` | **PASS** | I-11 |
| T-04 | Semver compare | ferric-app | `semver_compare` | **PASS** | R-14 |
| T-05 | Sequential function-test | App handlers | `tools/FunctionTest-Windows.ps1` | **PASS 59/59** v1.1.7 candidate (`last-function-test.txt`) | R-19, F-19, new instrument actions |
| T-06 | Zero-warning check | Workspace | `RUSTFLAGS=-D warnings cargo check --workspace --all-targets` | **PASS** v1.1.7 candidate (`last-cargo-check.txt`) | N-05 |
| T-07 | Plugin E2E (net) | plugin-bridge | `verify_real_downloaded_clap_and_vst3…` | **PASS** | F-12 |
| T-08 | SMF roundtrip | ferric-model | `smf_midi_roundtrip…` | **PASS** | R-08 |
| T-09 | CI Windows function-test | GitHub Actions | `ci-release.yml` step | Configured; runs on push | I-07 |

## Gaps (accepted)

- No automated egui pixel/widget hit-testing (I-05).
- No soak/stress harness for 8-hour sessions.
- No signed binary / SmartScreen attestation test.
- Updater download/apply not exercised against live GitHub in unit tests (network + side effects).
