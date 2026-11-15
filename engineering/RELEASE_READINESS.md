# Release Readiness

**Classification:** **CONDITIONALLY READY**

**App version:** **1.1.6**  
**Release:** https://github.com/reala1ynex/FerricDAW/releases/tag/v1.1.6  
**Ship commit:** `a79dc84` · **HEAD:** `9ce2698`

## Gates

| Gate | Status | Evidence |
|------|--------|----------|
| P0 defects resolved | PASS | I-01, I-02 |
| P1 defects resolved or blocked | PASS | I-03, I-07–I-09; I-13 blocked |
| `cargo test --workspace` | PASS | prior logs in `last-cargo-test.txt` |
| Windows function-test | PASS | 57/57 |
| README matches version | PASS | 1.1.6 |
| Updater feed BOM-free + SHA | PASS | `29b2a6d3…b5a3` |
| Only-current release channel | PASS | v1.1.5 removed; v1.1.6 Latest |
| Pad A/B chrome consistency pass | PASS | v1.1.6 UI chrome |
| Engineering registries + Codex handoff | PASS | `HANDOFF_CODEX.md` |
| Mac/Linux portable | N/A | I-14 |
| Full egui widget hit-test | Accepted | I-05 |
| True ASIO in portable zip | Blocked | I-13 |

## Why not VERIFIED READY

I-05 / I-13 / I-14 remain accepted or blocked scope limits. Do not claim VERIFIED READY while those verification/product gaps exist.

## Operator notes

- Clear stale `GH_TOKEN` before `gh`.
- Desktop: `FerricDAW-v1.1.6-win-x64.zip`
- Codex: start at `engineering/HANDOFF_CODEX.md`
