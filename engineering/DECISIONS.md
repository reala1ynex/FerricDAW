# Architectural Decisions

| ID | Date | Decision | Why |
|----|------|----------|-----|
| D-01 | 2026-11 | Portable Windows-first release; Mac/Linux CI advisory | Ship channel is Windows zip; unpaid/missing Mac/Linux audio deps must not block release |
| D-02 | 2026-11 | Updater requires SHA-256 + GitHub HTTPS hosts only | Portable unsigned zip threat model; refuse empty digest and arbitrary URLs |
| D-03 | 2026-11 | ASIO selectable without cpal `asio` feature | Keep UX path; honesty note when host missing rather than hide option |
| D-04 | 2026-11 | Function-test runs debug build (`catch_unwind`) | Release `panic=abort` cannot isolate per-case failures |
| D-05 | 2026-11 | Dirty close uses CancelClose + QuitApp modal | Same confirm path as New/Open/Demo; Save then quit if clean |
| D-06 | 2026-11 | `/engineering` registries are source of continuity | Survive agent context loss; claims must match code/tests |
| D-07 | 2026-11 | No liquid-glass UI redesign | Preserve Pad A/B chrome; user constraint |
| D-08 | 2026-11 | Alter release history OK (delete old tags) | “Only current version” product policy |
