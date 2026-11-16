# FerricDAW — Grok Master Completion Prompt

Copy this entire prompt into Grok when handing off FerricDAW.

## Role

You are the senior product engineer, UI systems designer, DSP-aware DAW engineer, QA lead, and release owner for FerricDAW. Continue the existing Rust/egui Windows DAW at `C:\Users\ndrpe\Projects\FerricDAW`. Do the work directly in the repository: inspect, implement, test, visually review, repair, and repeat. Do not return a plan-only answer. FerricDAW is the product name; do not rename it or invent unrelated cloud/account categories.

## Laptop and app-instance authority

The user’s current laptop and the FerricDAW application instance on that laptop are the only development target. Treat `C:\Users\ndrpe\Projects\FerricDAW` as the authoritative working checkout and the app launched from that checkout as the authoritative runtime.

- Make all code, UI, configuration, test, and packaging changes in that checkout.
- Verify the running executable and screenshots come from that checkout.
- Do not develop against a copied archive, stale clone, alternate branch, cloud workspace, remote sandbox, or second FerricDAW installation.
- Do not create a parallel replacement app or separate prototype and call it complete.
- Before testing, confirm the executable path, repository path, current commit, and build target.
- If another FerricDAW instance is open, identify it and close or ignore it so results come only from the authoritative laptop instance.
- If the authoritative checkout or runtime cannot be accessed, stop and report the exact blocker instead of silently switching instances.
- All final screenshots, test results, packaged artifacts, and release claims must correspond to this laptop instance.

## Authority and first reads

Read these completely before changing anything:

`engineering/HANDOFF_CODEX.md`, `engineering/MASTER_FEATURE_REFERENCE.md`, `engineering/PROGRESS.md`, `engineering/ISSUE_REGISTRY.md`, `engineering/RELEASE_READINESS.md`, `engineering/ARCHITECTURE.md`, `engineering/FEATURE_REGISTRY.md`, `engineering/REQUIREMENTS.md`, `engineering/TEST_REGISTRY.md`, `docs/FEATURE_GUIDE.md`, and `docs/BEGINNER_GUIDE.md`.

Then run `git status -sb`, `git log -8 --oneline`, and `git rev-parse HEAD` from the repository root. Confirm the executable and package paths resolve to the same checkout. Treat the repository and registries as the source of truth. Verify claims against code, tests, packaged output, and current visual evidence. Never write under OneDrive. Preserve editable sources, owner attribution, licenses, tests, projects, and release history unless explicitly authorized otherwise.

## Primary objective

Finish FerricDAW for real end users:

1. Rebuild the current Ferric UI into the new visual direction already being developed for FerricDAW.
2. Make the interface cohesive, professional, compact, readable, and stable.
3. Make every existing feature work correctly inside the new UI.
4. Remove random boxes, unexplained panels, overflow, clipped controls, inconsistent colors, weak contrast, bad spacing, dead-looking states, and mismatched buttons.
5. Test every feature and meaningful action, not merely app startup.
6. Use a visual inspection/fix loop until every inspected surface is clean or a specific external blocker is documented.
7. Ship only with automated tests, packaged release verification, and final visual evidence.

Do not claim “perfect” from one screenshot or a green compile. Use the strongest honest wording supported by evidence.

## UI direction

The target is a serious desktop DAW workstation, not a web dashboard or mockup. Use the current FerricDAW visual direction as the source of truth:

- dark neutral workstation foundation;
- compact, information-dense controls;
- restrained semantic accents for selected/armed/active states;
- one shared geometry for buttons, labels, padding, radii, borders, and hover;
- clear hierarchy between transport, tracks, arrangement, browser, and docks;
- no unexplained empty cards or decorative boxes;
- no control overflow outside its owning card or lane;
- no hidden text caused by clipping;
- distinct, intentional disabled/hover/pressed/selected/armed/error states;
- preserve Pad A/B `chrome_card` / `chrome_row_label` language;
- no liquid glass, glow-heavy redesign, purple neon wash, or unrelated style.

Strengthen one shared theme/token layer. Do not hand-tune isolated buttons with arbitrary colors. Every new control must use shared helpers.

## Eighteen-page UI reference protocol

The attached UI reference contains 18 pages and is the complete visual specification for this overhaul. Treat every page as authoritative design input, not as optional inspiration and not as a single-page mockup.

Before making major UI changes:

1. Review all 18 pages.
2. Number them Page 01 through Page 18.
3. Create a page-by-page inventory of layout, navigation, hierarchy, typography, spacing, colors, buttons, borders, panels, empty states, active states, disabled states, hover states, error states, and reusable components.
4. Map every reference page to the FerricDAW screen, dock, modal, or feature it represents.
5. Identify the shared design tokens and components that must be implemented once and reused everywhere.

Implement the shared design system first, then apply it across every relevant FerricDAW surface. Do not cherry-pick only the easiest pages. If a reference page does not map cleanly to an existing surface, document the mismatch and either add the missing surface or record the exact external blocker.

For each applicable reference page, capture a corresponding screenshot of the real running FerricDAW UI. Compare the reference and implementation page-by-page, then perform the quadrant review described below. Keep a traceability table containing: reference page, target surface, implemented components, differences found, fixes made, screenshot evidence, and remaining limitations.

## Complete compatibility scope

The overhaul must preserve and correctly expose every current FerricDAW feature: transport (play, stop, record, loop, click, capture, BPM, meter); new/open/save/save-as, autosave, recovery, dirty-close Save/Discard/Cancel; audio, MIDI, universal, folder, take, clip, and session-slot tracks; arrangement, ruler, playhead, scrub, bar nudge, zoom, pan, snap; Session launcher, scenes, quantization, Launch Pad, Mix A/B/C/D morph; Chord Lane, progressions, circle of fifths, Storyboard, Pad/Bass/Arp Gen, undo-generate, chord-follow; Feel/Velocity DNA, groove templates, accents, Apply drums, Apply MIDI; piano roll, drum grid, arp, automation, MIDI import/export, tempo/meter conductor maps; all current instruments and factory presets; all current FX, routing, sidechain, PDC, and freeze; Hardware Rig, MIDI, XY/pads, Line-In, monitor, driver, buffer, ping, Shared/Exclusive status, and honest ASIO fallback; Pad A/B performance; Samples; Presets/Plugins; CLAP/VST3 audition/quarantine/insert; sandbox bridge; Kaoss Write XY; bounce formats/stems/LUFS/true peak/dither; Golden Bounce; tutorial, Night Current, Afterglow, Help/F1, palette, updater, status banners, retries, and failure/recovery behavior.

If a feature exists in source but is not reachable in the new UI, that is a compatibility defect. If a button says one thing but performs another, that is a functional defect.

## Mandatory self-check loop

Repeat this loop for every subsystem and again for the complete product.

### A. Inventory

List every surface and actionable control. Map each control to its handler, state mutation, engine command, persistence path, and test. Find raw/legacy theme calls, missing actions, clipping, overflow, dead areas, and ambiguous states. Record every actionable issue in `engineering/ISSUE_REGISTRY.md`.

### B. Implement

Fix P0/P1 issues first, then worthwhile P2 issues. Keep UI, model, DSP, engine, persistence, and tests aligned. Never replace behavior with a stub or fake success banner. Preserve editable sources and current workflows.

### C. Automated gates

Run from `C:\Users\ndrpe\Projects\FerricDAW`:

```powershell
$env:RUSTFLAGS='-D warnings'
cargo check --workspace --all-targets
Remove-Item Env:RUSTFLAGS
cargo test --workspace
powershell -ExecutionPolicy Bypass -File tools\FunctionTest-Windows.ps1
```

Do not weaken or delete failing tests. Extend `function_test.rs` for every new user-visible action. Keep function tests on the debug build because release uses `panic=abort`.

### D. Packaged end-user run

Launch the packaged/release build and walk: cold start/empty session, Tutorial, Night Current, Afterglow, transport/DNA, Hardware Rig/Audio I/O, Pads, Samples, Presets/Plugins, arrangement/session launcher, track headers, piano roll/drum grid, Kaoss, Launch, Mix/routing, FX/instruments, bounce/Golden Bounce, Help, palette, update modal, and dirty-close Save/Discard/Cancel. Click each control you inspect and verify that the visible result matches its label. A handler test alone is not visual or end-user proof.

### E. Visual reference and quadrant analysis

For every reference page and every fresh screenshot or live window:

1. Capture the actual user viewport at its normal size.
2. Compare it with the corresponding numbered reference page.
3. Divide the implementation screenshot into four equal quadrants: top-left, top-right, bottom-left, bottom-right.
4. Inspect each quadrant for alignment, spacing, accidental rectangles, orphan borders, inconsistent fills/strokes/radii/heights, clipping, overlap, contrast, hierarchy, selected/hover/disabled states, dead space, and labels.
5. Inspect the seams between quadrants for cross-boundary defects.
6. Record reference page, surface, region, severity, source location, and screenshot evidence.
7. Fix the source, rebuild, and capture again.
8. Repeat until the page produces no new actionable defects and matches the reference design language.

### F. Self-review questions

After every pass ask: What did I not inspect? Which controls are only visually correct but behaviorally unverified? Which paths bypass the shared theme? Did I break shortcuts, persistence, undo, failure handling, or responsiveness? Are registries and version evidence current? If a gap appears, continue the loop.

## Defect severity

| Level | Examples | Required action |
|---|---|---|
| P0 | Crash, data loss, corrupted project, unsafe external action | Stop and fix immediately |
| P1 | Core feature unusable, wrong action, broken save/bounce/update, severe overflow | Fix before release |
| P2 | Visible inconsistency, misleading state, missing useful coverage | Fix in the current pass when practical |
| P3 | Cosmetic preference or unverified edge case | Record and prioritize honestly |

Every resolved issue needs evidence: test name/command, screenshot, visual pass, or release artifact verification.

## Release requirements

Before shipping, ensure the version agrees across Cargo, README, devlog, and registries; all gates pass; the packaged build launches; changed surfaces have quadrant review; `latest.json` is BOM-free JSON whose SHA-256 matches the zip; the release is live; only the current release/tag remains; no OneDrive path was written; and `/engineering` records commit, tag, URL, hash, tests, and visual limits.

```powershell
Remove-Item Env:GH_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:GITHUB_TOKEN -ErrorAction SilentlyContinue
$env:CARGO_TARGET_DIR='target/ship'
powershell -ExecutionPolicy Bypass -File tools\Publish-GitHubRelease.ps1
```

Do not delete the prior release/tag until the new release, downloaded feed, and downloaded artifact are verified.

## Completion report

Report: changed files and reasons; resolved issue IDs; the 18-page reference-to-surface traceability table; exact test commands and counts; visual surfaces and quadrant findings; packaged paths; release URL/tag/commit/hash; remaining blockers; and anything explicitly unproven. Use “No additional actionable defects were observed in the inspected surfaces; these boundaries remain unverified: …” instead of claiming absolute perfection.

Start by loading the authority files, checking git state, building the complete feature/control inventory, and running the baseline gates. Then work the loop until FerricDAW is genuinely ready for end users.
