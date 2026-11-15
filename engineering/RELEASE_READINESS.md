# Release Readiness

**Classification:** **CONDITIONALLY READY** (shipping gates met; egui pixel hit-test + true ASIO + Mac/Linux packaging explicitly deferred)

**App version:** **1.1.5**  
**Release:** https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.5  
**Commit:** `f14bcd8`

## Gates

| Gate | Status | Evidence |
|------|--------|----------|
| P0 defects resolved | PASS | I-01, I-02 shipped |
| P1 defects resolved or blocked | PASS | I-03, I-07–I-09 shipped; I-13 blocked by design |
| `cargo test --workspace` | PASS | `last-cargo-test.txt` |
| Windows function-test (debug) | PASS | 57/57 |
| Function-test on packaged release binary | PASS | `dist/FerricDAW/ferricdaw.exe` 57/57 |
| README matches version | PASS | 1.1.5 |
| Updater feed BOM-free + SHA match | PASS | remote `latest.json`; sha256=`af057367…2133` |
| Only-current release channel | PASS | v1.1.4 release/tag removed; v1.1.5 Latest |
| Engineering registries | PASS | `/engineering` |
| Mac/Linux portable | N/A | N-06 / I-14 |
| Full egui widget hit-test | Accepted limit | I-05 |
| True ASIO in portable zip | Blocked | I-13 |

## Why not VERIFIED READY

Remaining accepted/blocked limits (I-05, I-13, I-14) are documented product constraints, not open P0/P1 defects. Classification stays **CONDITIONALLY READY** per evidence rules (do not claim VERIFIED READY solely because no more P0/P1 remain when verification scope is incomplete).

## Operator notes

- Stale `GH_TOKEN` env breaks `gh`; clear it and use keyring (`Publish-GitHubRelease.ps1` already clears it).
- Desktop copy: `C:\Users\ndrpe\Desktop\FerricDAW-v1.1.5-win-x64.zip`
