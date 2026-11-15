# Release Readiness

**Classification:** v1.1.8 published and verified, with the visual coverage limits below.

| Gate | Status | Evidence |
|------|--------|----------|
| Warning-denied workspace check | PASS | `last-cargo-check.txt` |
| Workspace tests | PASS | `last-cargo-test.txt` |
| Windows function test | PASS | 59/59, `last-function-test.txt` |
| Release build | PASS | `target/ship` |
| Main visual workflows | PASS with limits | `PROGRESS.md`: release UI, format selection, Afterglow WAV bounce, tutorial page navigation and save-before-rebuild; earlier browser and dock exploration |
| Every widget and tutorial action | Not exhaustively tested | I-05 remains open; do not represent navigation as completing every tutorial action |
| True ASIO | Blocked by portable build design | I-13 |
| GitHub v1.1.8 release and remote feed/hash | PASS | Downloaded feed reports 1.1.8, begins with `{`, has no BOM, and SHA-256 `3e80f96dff18dfa9ed10b51bf7fa6295aad60a0b4feb7e11f6eef6b2e6e0bfec` matches the v1.1.8 zip |
| Only-current release channel | PASS | v1.1.7 release and tag removed after v1.1.8 verification; remote release/tag inventory shows v1.1.8 only |

Public release: https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.8. Tagged commit: `13f830c`. Clear stale `GH_TOKEN` and `GITHUB_TOKEN`; set `CARGO_TARGET_DIR=target/ship` if the default release binary is in use.
