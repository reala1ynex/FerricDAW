# Release Readiness

**Classification:** v1.1.12 published and verified, with the visual coverage limits below.

| Gate | Status | Evidence |
|------|--------|----------|
| Warning-denied workspace check | PASS | `last-cargo-check.txt` |
| Workspace tests | PASS | `last-cargo-test.txt` |
| Windows function test | PASS | 62/62 on the 1.1.13 candidate (59/59 at v1.1.12), `last-function-test.txt` |
| Release build | PASS | `target/ship` |
| Main visual workflows | PASS with limits | `PROGRESS.md`: release UI, format selection, Afterglow WAV bounce, tutorial page navigation and save-before-rebuild; earlier browser and dock exploration |
| Every widget and tutorial action | Not exhaustively tested | I-05 remains open; do not represent navigation as completing every tutorial action |
| True ASIO | Blocked by portable build design | I-13 |
| GitHub v1.1.12 release and remote feed/hash | PASS | Downloaded feed reports 1.1.12, begins with `{`, has no BOM, and SHA-256 `df47ae3c82ec0f1afbb92b629bcf6439a06c83597db79d408b80e3d8a871f91b` matches the downloaded v1.1.12 zip |
| Only-current release channel | PASS | v1.1.11 release/tag removed after v1.1.12 verification; remote release/tag inventory shows v1.1.12 only |

Public release: https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.12. Tagged commit: `4e3c754`. Clear stale `GH_TOKEN` and `GITHUB_TOKEN`; set `CARGO_TARGET_DIR=target/ship` if the default release binary is in use.

## Local 1.1.13 development candidate

**Classification: local engineering candidate; publication gate open.** The
published v1.1.12 tag and feed above are independent of this work. The candidate
addresses layout, keyboard timing, and codec interoperability. It must not be
published as the requested finished release while the full visual/action matrix
is incomplete and MP3 requires a separately installed Windows dependency.

Latest gate run (2026-11-16 14:20–14:25 ET, uncommitted-tree build later committed locally):
warning-denied check passed, 37 workspace tests passed, function test 62/62,
packaging passed. MP3 now uses a bundled encoder, so the FFmpeg dependency noted
above no longer applies. Publication still needs the owner's go-ahead; the open
visual/action items below (I-05, I-28) are disclosed rather than closed.

Candidate evidence is in `CANDIDATE_VERIFICATION.md`, with reference mappings
and remaining visual differences in `UI_REFERENCE_TRACEABILITY.md`, and the
feature/control routes in `NATIVE_CONTROL_INVENTORY.md`.
