# Release Readiness

**Classification:** **CONDITIONALLY READY** → target **VERIFIED READY** after v1.1.5 GitHub assets verified

**App version:** 1.1.5 (shipping this pass)

## Gates

| Gate | Status | Evidence |
|------|--------|----------|
| P0 defects resolved | PASS | I-01, I-02 in tree |
| P1 defects resolved or blocked | PASS | I-03, I-07–I-09 resolved; I-13 blocked by design |
| `cargo test --workspace` | PASS | `last-cargo-test.txt` |
| Windows function-test | PASS | 57/57 `last-function-test.txt` |
| README matches version | PASS | 1.1.5 |
| Updater feed BOM-free | PENDING verify after publish | Package script writes UTF-8 no BOM |
| Hardened updater/dirty-close in release binary | PENDING publish | This release |
| Engineering registries | PASS | `/engineering` |
| Mac/Linux portable | N/A | N-06 |
| Full egui widget hit-test | Accepted limit | I-05 |

## Known limitations

1. Function-test validates handlers, not every egui hit target (I-05).
2. Portable ASIO is best-effort; expect WASAPI fallback honesty note (I-13).
3. Publishing needs valid `gh` keyring auth (clear stale `GH_TOKEN`) (I-12).
4. Mac/Linux packaging deferred (I-14).
