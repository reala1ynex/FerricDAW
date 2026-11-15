# Release Readiness

**Classification:** **CONDITIONALLY READY**

**App version:** 1.1.4 published; post-1.1.4 tree hardens dirty-close + updater. Recommend **1.1.5** republish so those binaries reach users.

## Gates

| Gate | Status | Evidence |
|------|--------|----------|
| P0 defects resolved | PASS | I-01, I-02 resolved in tree |
| P1 defects resolved or blocked | PASS | I-03, I-07, I-08, I-09 resolved; I-13 blocked by design |
| `cargo test --workspace` | PASS | `engineering/last-cargo-test.txt` 2026-11-15 |
| Windows function-test | PASS | 57/57 — `engineering/last-function-test.txt` |
| README matches version | PASS | 1.1.4 |
| Updater feed BOM-free on last publish | PASS (prior) | v1.1.4 release assets |
| Engineering registries present | PASS | `/engineering` |
| Mac/Linux portable | N/A / deferred | N-06 |
| Full egui widget hit-test | FAIL accepted | I-05 |
| Hardened updater/dirty-close in GitHub release binary | FAIL until 1.1.5 | Tree-only until republish |

## Known limitations for operators

1. Function-test validates handlers, not every egui hit target.
2. Portable ASIO is best-effort; expect WASAPI fallback honesty note.
3. Publishing still requires valid `gh` authentication on the operator machine.
4. Security: updater now refuses empty SHA and non-GitHub hosts — **ship 1.1.5** to put that binary in the wild.
