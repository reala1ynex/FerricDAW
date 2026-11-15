# Release Readiness

**Classification:** v1.1.12 candidate, with the visual coverage limits below.

| Gate | Status | Evidence |
|------|--------|----------|
| Warning-denied workspace check | PASS | `last-cargo-check.txt` |
| Workspace tests | PASS | `last-cargo-test.txt` |
| Windows function test | PASS | 59/59, `last-function-test.txt` |
| Release build | PASS | `target/ship` |
| Main visual workflows | PASS with limits | `PROGRESS.md`: release UI, format selection, Afterglow WAV bounce, tutorial page navigation and save-before-rebuild; earlier browser and dock exploration |
| Every widget and tutorial action | Not exhaustively tested | I-05 remains open; do not represent navigation as completing every tutorial action |
| True ASIO | Blocked by portable build design | I-13 |
| GitHub v1.1.12 release and remote feed/hash | PENDING | Verify after publish; feed must begin with `{`, have no BOM, and match the v1.1.12 zip SHA-256 |
| Only-current release channel | PENDING | Remove v1.1.11 release/tag only after v1.1.12 feed and artifact verification |

Public release: v1.1.11 remains current until v1.1.12 is published and verified. Clear stale `GH_TOKEN` and `GITHUB_TOKEN`; set `CARGO_TARGET_DIR=target/ship` if the default release binary is in use.
