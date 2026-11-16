# FerricDAW brand reference and native UI traceability

Baseline: `6c5d1ccb7692282750d6b4c2369cf732c2dd935c`, Windows x64 GNU.
The supplied raster PDF contains **23** numbered pages, not 18. Pages 01–18 below
are the requested core inventory. Pages 19–23 were also visually reviewed.
The PDF is a design reference; its concept names and hypothetical hardware,
devices, versions and marketing claims are not requirements for this checkout.

| Page | Design inventory | Native surface and comparison / work remaining |
|---|---|---|
| 01 Cover | Orange-front/cyan-back lattice, white/orange uppercase wordmark, large cover motion, index | Menu identity and Help. Desktop uses clean small treatment; cover glitch is editorial. |
| 02 Brand story | Three columns; lattice diagrams encode audio, control, device, engine and routing | Semantic palette and app identity; no runtime brand-story panel required. |
| 03 Voice | Short label plus units; steady sentence-case messages, colored warning/fault examples | Transport, device/status, Help, errors. Preserve exact runtime qualifications and remove ambiguous labels as reviewed. |
| 04 Logo anatomy | Cyan back face, orange front face and wave core; solid mono wordmark | App icon and compact menu lockup; large geometry and decorative effects are not runtime controls. |
| 05 Lockups | Horizontal, stacked, mark-only, wordmark-only and monochrome | Header and Windows icon; print variants have no independently testable app surface. |
| 06 Clear space | Horizontal min 120 px, reduced standalone mark 16 px, keep margin around mark | Header/icon; inspect at laptop scale and minimum window. |
| 07 Misuse | No stretch, rotation, recolor, cropping or small-size glitch | App icon and header; compare without glow or clipping. |
| 08 Palette | Void #07080A, Panel #12161B, Steel #8AB4BD, Orange #FF5A1F, Cyan #00E5FF, Pink #FF2E63; restrained use | Shared theme definitions; orange audio/action, cyan selection/control, pink faults. |
| 09 Semantic color | Play green, loop cyan, record red, arm pink, solo yellow, mute blue; labeled states; meter zones | Transport, arrangement, mixer, routing and meters; verify idle/active/armed and contrast. |
| 10 Typography | DejaVu Sans Mono; aligned readouts, real units, white/steel content, caps section labels | Transport BBT, duration, tempo and meter; shared captions. Existing text font fallback and exact typeface tracked separately. |
| 11 Effects | Clean editable surfaces; limited, reduced-motion transition effects | Native editor remains clean. Cover/splash effects not required for normal startup. |
| 12 Iconography | Square-keyline outline icons, labeled through interaction; distinct idle, hover, selected, disabled | Transport and panel affordances; stock native icon migration incomplete. |
| 13 Components | Compact knobs/faders, segmented peak meter, strong/neutral/ghost/danger buttons, focus outline, error/disabled/pressed, select and XY | Shared theme; applies across transport, pads, browser, Kaoss, dialogs, track controls and editors. |
| 14 Main layout | Identity/transport, browser, wide arrangement, selected-track inspector, mixer/master, footer | Existing bottom FX dock is the inspector equivalent; current sample/preset classification retained. Baseline DNA/footer clipping and large Session reservation. |
| 15 Channel strip | Color/name, input, trim/phase, inserts, sends, pan, fader, stereo meters, mute/solo/arm, output; master distinct | Mix / Route; phase and exact reference strip layout differ, do not imply controls exist. |
| 16 Plugin template | Title, preset rail, modular section grid, parameter diagrams, keyboard, status | Native FX/instrument editor. External CLAP/VST3 editors draw their own GUIs; host must not skin their internals. Oxide is a concept example. |
| 17 Instruments | Sampler waveform and ranges; drum pads and 16-step grid; colored steps and live playhead | SamplePlayer, Samples, Drum Kit, Pads and MIDI drum grid. Prototype key-zone/pattern editor not part of existing feature set. |
| 18 MIDI tools | Piano keys, grid and playhead, orange notes/cyan ghost layer, velocity/CC, arp/chord/monitor | Piano roll, automation, Chord Lane, arp and MIDI activity; no standalone event-monitor panel. |
| 19 Families | Orange instrument, cyan effect and pink MIDI; similar device shells | Consider category accents in FX and MIDI; preserve existing names and routing. |
| 20 Motion | Startup animation, clean reduced-motion paths and meter ballistics | Meter rendering is applicable; real peak-hold behavior requires validation. No artificial startup delay. |
| 21 Digital applications | Site, social assets, splash, installer | Portable DAW; no installer/site project authorized here. |
| 22 Print | Flat mark and ink limits, merchandise | No native UI counterpart. |
| 23 Colophon | Design assets and concept provenance | Preserve attribution; no native UI counterpart. |

The color, state, geometry, focus and meter rules belong in `theme.rs`.
Bindings and persistence remain in the existing model and engine pathways. For
the feature-to-code-to-test baseline see `MASTER_FEATURE_REFERENCE.md`. Live
capture and click evidence is separate from handler, model and DSP evidence.

## Packaged visual review, Windows laptop

Captured at the normal maximized workstation viewport (1638 × 1024 capture).
Evidence names distinguish first-pass screenshots from repaired candidates. The
external PDF contains design layouts, not 18 separate runnable windows.

| Pages | Evidence and quadrant/seam observations |
|---|---|
| 01–07, 10–12 | Workstation: FERRICDAW menu identity is top left; viewing the rest of the viewport confirms clean working chrome with steady text. Native title/app icon uses the flat lattice. Full split-color horizontal lockup, DejaVu embedding and transport icon set are still outstanding. |
| 08–09, 13 | Transport play capture: active state and playhead visible at top; dark surfaces and semantic labels throughout. New running/armed/mute/solo colors have shared sources. Meter segments are rendered visually; calibrated peak-hold and every focus/disabled treatment have not been verified. |
| 14 | Workstation and resize captures: top-left DNA/browser controls remain inside their regions; top-right ruler gains space when Session is closed; bottom-left Pad picker buttons fit the card and Audio I/O scrolls; bottom-right dock opens at a compact size and resizes in both directions. Vertical browser/track seam and horizontal dock/arrangement seam inspected. Independent right inspector still absent. |
| 15 | First-pass Afterglow Mix: top quadrants show scene slots, conductor and arrangement; bottom quadrants show channel labels, faders and stereo segments. Right edge is a horizontal scroll viewport. The reference's always visible master and exact strip anatomy are unverified. |
| 16, 19 | FX/preset and Kaoss captures: preset selection and track FX count in top half; bottom shows loaded Electric Piano and working XY control. Card and editor seam visible. Third-party editor surfaces and dedicated device chrome not visually qualified. |
| 17 | Samples and Afterglow FX: +Trk created the sample and painted an orange waveform; built-in Drum Kit opened in the rack. Key zones, device-specific drum grid, and sample waveform edit scope differ from the concept page. |
| 18 | Chord/conductor and scene states are visible in Afterglow. Piano-roll and drum-grid interactive coverage is incomplete; displaying a chord lane does not test its editor or generation commands. |
| 20–23 | Meters are the applicable part of 20. Other motion, print, site, installer and colophon assets are outside this Windows portable runtime review. |
| Supporting workflows | Night Current step A, Help Quickstart, Samples, Presets, Route, Kaoss, Launch, dirty close Save/Discard/Cancel, and Bounce/Golden Bounce were exercised as described in the candidate verification record. Not every control on those windows was exercised. |
