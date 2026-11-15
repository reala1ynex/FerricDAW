# Release Readiness

**Classification:** v1.1.9 published and verified, with the visual coverage limits below.

| Gate | Status | Evidence |
|------|--------|----------|
| Warning-denied workspace check | PASS | `last-cargo-check.txt` |
| Workspace tests | PASS | `last-cargo-test.txt` |
| Windows function test | PASS | 59/59, `last-function-test.txt` |
| Release build | PASS | `target/ship` |
| Main visual workflows | PASS with limits | `PROGRESS.md`: release UI, format selection, Afterglow WAV bounce, tutorial page navigation and save-before-rebuild; earlier browser and dock exploration |
| Every widget and tutorial action | Not exhaustively tested | I-05 remains open; do not represent navigation as completing every tutorial action |
| True ASIO | Blocked by portable build design | I-13 |
| GitHub v1.1.9 release and remote feed/hash | PASS | Downloaded feed reports 1.1.9, begins with `{`, has no BOM, and SHA-256 `c7b53e51fc38a558f49bc3335400c3f20f0efb905824127da9806fb034eb076c` matches the v1.1.9 zip |
| Only-current release channel | PASS | v1.1.8 release and tag removed after v1.1.9 verification; remote release/tag inventory shows v1.1.9 only |

Public release: https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.9. Tagged commit: `7a46173`. Clear stale `GH_TOKEN` and `GITHUB_TOKEN`; set `CARGO_TARGET_DIR=target/ship` if the default release binary is in use.
