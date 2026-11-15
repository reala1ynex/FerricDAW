# Release Readiness

**Classification:** v1.1.7 published and verified, with the visual coverage limits below.

| Gate | Status | Evidence |
|------|--------|----------|
| Warning-denied workspace check | PASS | `last-cargo-check.txt` |
| Workspace tests | PASS | `last-cargo-test.txt` |
| Windows function test | PASS | 59/59, `last-function-test.txt` |
| Release build | PASS | `target/ship` |
| Main visual workflows | PASS with limits | `PROGRESS.md`: release UI, format selection, Afterglow WAV bounce, tutorial page navigation and save-before-rebuild; earlier browser and dock exploration |
| Every widget and tutorial action | Not exhaustively tested | I-05 remains open; do not represent navigation as completing every tutorial action |
| True ASIO | Blocked by portable build design | I-13 |
| GitHub v1.1.7 release and remote feed/hash | PASS | Downloaded zip SHA-256 `7ef744c94350ecbd79333abcb844be55abdc857506b8249f1d3bc88db6fd9524` matches downloaded feed and `/latest`; feed begins with `{` and has no BOM |
| Only-current release channel | PASS | v1.1.6 release and tag removed after v1.1.7 verification; release list and remote tags show v1.1.7 only |

Public release: https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.7. Tagged commit: `3765857`. Clear stale `GH_TOKEN` and `GITHUB_TOKEN`; set `CARGO_TARGET_DIR=target/ship` if the default release binary is in use.
