# Progress Ledger

**Last updated:** 2026-11-15  
**HEAD at prior handoff:** `e0c1af5` / ship `a79dc84` (v1.1.6)  
**Active goal:** ★ ACTIVE MISSION in `HANDOFF_CODEX.md` — flaw review, more instruments, visual run, same tests, GitHub ship (target **v1.1.7+**).

## Completed (prior sessions)

- [x] `/engineering` registries + **HANDOFF_CODEX.md**
- [x] P0/P1 repairs through v1.1.5
- [x] Pad A/B chrome pass (v1.1.6)
- [x] Published v1.1.6; only-current channel

## Assigned to Codex (open)

- [ ] Adversarial flaw review → ISSUE_REGISTRY → fix P0/P1 (+ solid P2s)
- [ ] Add more instruments (real DSP + palette + presets + function-test)
- [ ] Final end-user visual run (every surface; Pad A/B chrome bar)
- [ ] `cargo check -D warnings` + `cargo test --workspace` + FunctionTest-Windows.ps1
- [ ] Bump, publish GitHub release, delete prior tag, update registries

## Blockers / accepted limits

- I-05 egui every-widget hit-test (accepted unless Codex expands coverage)
- I-12 operator `GH_TOKEN` stale env (clear before `gh`)
- I-13 true ASIO (blocked by design for portable)
- I-14 Mac/Linux ship (out of scope)
