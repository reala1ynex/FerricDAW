# Release Readiness

**Classification:** v1.1.12 published and verified, with the visual coverage limits below.

| Gate | Status | Evidence |
|------|--------|----------|
| Warning-denied workspace check | PASS | `last-cargo-check.txt` |
| Workspace tests | PASS | `last-cargo-test.txt` |
| Windows function test | PASS | 59/59, `last-function-test.txt` |
| Release build | PASS | `target/ship` |
| Main visual workflows | PASS with limits | `PROGRESS.md`: release UI, format selection, Afterglow WAV bounce, tutorial page navigation and save-before-rebuild; earlier browser and dock exploration |
| Every widget and tutorial action | Not exhaustively tested | I-05 remains open; do not represent navigation as completing every tutorial action |
| True ASIO | Blocked by portable build design | I-13 |
| GitHub v1.1.12 release and remote feed/hash | PASS | Downloaded feed reports 1.1.12, begins with `{`, has no BOM, and SHA-256 `df47ae3c82ec0f1afbb92b629bcf6439a06c83597db79d408b80e3d8a871f91b` matches the downloaded v1.1.12 zip |
| Only-current release channel | PASS | v1.1.11 release/tag removed after v1.1.12 verification; remote release/tag inventory shows v1.1.12 only |

Public release: https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.12. Tagged commit: `4e3c754`. Clear stale `GH_TOKEN` and `GITHUB_TOKEN`; set `CARGO_TARGET_DIR=target/ship` if the default release binary is in use.
