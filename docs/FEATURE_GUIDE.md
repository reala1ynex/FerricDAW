# FerricDAW Feature Guide

What’s in the studio and where to find it. For building the tutorial song, use [`BEGINNER_GUIDE.md`](BEGINNER_GUIDE.md) or the in-app **🎓 Tutorial**.

---

## Layout

| Area | What’s there |
|------|----------------|
| Top | Play / Rec / Loop / Click / BPM / Pads / Help / Tutorial / command palette |
| DNA strip | Feel + Velocity knobs (groove pocket and accents) |
| Left | Hardware Rig · Samples · Presets / CLAP / VST3 |
| Center | Arrangement · session clips · Chord Lane · Storyboard sections |
| Lower dock | MIDI editor · FX · Kaoss · Launch Pad · Mixer |
| Footer | Status · Plugin Playground insert |

---

## Groove DNA + Velocity DNA

Under the transport.

| Control | What it does |
|---------|----------------|
| Feel 0–100 | How deep the timing pocket goes |
| Groove picker | Straight · Techno Pocket · Boom-Bap · Push |
| Velocity 0–100 | Accent contrast |
| Accent shape | Flat · Offbeat Lift · Downbeat Punch · Hat Chatter |
| Apply drums | Push the template onto drum tracks |
| Apply all MIDI | Same for instrument tracks |

Live Feel follows the project knob. After **Apply**, tracks keep their pocket even if you later set Feel to 0. Accents are stable for a given project (same song → same hits). Bounce matches what you hear.

---

## HostDNA Synth

**Ctrl+K → Add HostDNA Track**, or drop **HOST-DNA** on an FX chain.

| Action | What it does |
|--------|----------------|
| Init from Host | Builds an editable starting patch from a coarse PC fingerprint |
| Reseed PC | Rebuild Init on this machine |
| Randomize | New seed, still editable |
| Paste seed | Hex from another machine reproduces that Init |
| A / B / C | Three siblings from the same seed |

You only see a **Host Seed** hex — no serials or MACs. The seed travels with the project. Not DRM; patches play anywhere.

---

## Chord Storyboard

Arrangement toolbar (section chips) + Chord Lane **Gen**.

1. Pick a section: Intro / Groove / Hook / Stack.
2. **Gen Pad / Bass / Arp** (or Chord Lane **Gen** for all three).
3. Read the patch card for what got made.
4. **Undo Generate** restores the previous state.

Generate only writes inside that section’s beat range.

---

## Golden Bounce

After a successful master bounce (**Ctrl+B**).

| Control | What it does |
|---------|----------------|
| Open Golden Bounce | Side-by-side LUFS / true peak / duration |
| Pick reference / Use demo | Default: `demo/NightCurrent-Demo.wav` |
| Loudness-match A/B | Hear your bounce matched to the reference level |
| Waveforms | Rough overview of both files |
| Δ LUFS | How far you are from the reference |

---

## Scene Launch Pad

Lower dock → **Launch Pad**.

- Scene pads fire session scenes
- Quantize: 1 Bar · 1 Beat · 1/4
- Optional Mix morph toward Mix A–D over N beats
- Keys **1–8** fire scenes while the Launch Pad tab is focused

Morph starts from the **live mix** and moves toward the Mix slot — absolute, no compounding.

---

## Plugin Playground

Browser → Presets / CLAP · VST3.

1. **▶ Audition** loads the plugin on a sandbox track.
2. If it crashes, it’s quarantined and Insert stays blocked.
3. If it survives, use the footer **Insert into selected track**.
4. **+ Insert** still inserts directly (also sandboxed by default).

A bad audition should never take down Ferric.

---

## Hardware Rig

Browser → Hardware Rig.

| Piece | Notes |
|-------|--------|
| Skins | Default Desk · Laptop Pads · Handheld Tip (labels only) |
| Status | Exclusive vs Shared · buffer · last ping |
| Columns | MIDI · XY/Pads · Line-In |
| Spawn | Generic USB MIDI + Line-In |

Tip presets are optional names, not brand locks.

**WASAPI Exclusive** locks the selected output when the device allows it; otherwise Ferric falls back to Shared and says so.

---

## Everyday studio tools

### Tracks & mix

- Universal tracks (audio + MIDI + folders + session slots)
- **+ Track** menu: Vocal (mic) · MIDI · Instrument · Audio · Universal
- Track header **In** + Mon — mic vs MIDI is clear; instrument tracks are MIDI-only
- VCA groups · Mix A/B/C/D · sidechain · PDC · freeze (`Ctrl+F`)
- Auto equal-power crossfades · glue (`Ctrl+G`) · 4-way stem split

### Instruments & FX

- PolySynth · MonoBass · Prism · HostDNA · FerricWave · Drum Kit · Sampler
- Vocal Auto-Pitch · Tuner · Amp/Cab · EQ · Comp · OTT · Tape · Delay · Reverb · IR
- CLAP / VST3 with floating editors + out-of-process sandbox

### Arrangement & MIDI

- Session launcher · Chord Lane · circle of fifths / progressions · Chord-Follow
- Tempo/meter conductor maps · piano roll · 16-step drum grid · arp
- Piano roll: right-click note edit · Backspace deletes · drag notes · scroll-zoom at cursor
- Playhead: click/drag ruler · drag the triangle · click empty lane · scrub BBT · ◀/▶ bar nudge
- Wheel / middle-drag / Alt-drag pans time and tracks
- Dual keyboard pads (letters = Pad A, numpad/number row = Pad B)
- Kaoss / XY pad on the selected track’s instrument + FX · Tap Tempo · Line-In BPM · loopback ping

### Bounce & sessions

- WAV / FLAC / MP3 / OGG · −14 LUFS · true-peak · TPDF dither
- Stems folder · Golden Bounce A/B
- `.ferric` projects · crash autosave · undo/redo · first-run chooser

---

## First hour ideas

1. **🎓 Tutorial** → Night Current (or play the demo WAV).
2. Turn up DNA Feel / Velocity on hats — hear pocket without redrawing notes.
3. Ctrl+K → HostDNA → Reseed / siblings → twist Morph/Cutoff.
4. Storyboard **Gen Pad** on Hook → Undo Generate.
5. Bounce → Golden Bounce vs demo.
6. Launch Pad: Intro → Groove, quantized; optional Mix morph.
7. Hardware Rig: pick a skin, check Exclusive/Shared status.

---

## Doc map

| Doc | Role |
|-----|------|
| This file | Feature map |
| `BEGINNER_GUIDE.md` | Night Current recipe |
| `NIGHT_CURRENT_DEMO.md` | Demo WAV notes |
| In-app **F1** | Quickstart + this guide + shortcuts |
| In-app **🎓 Tutorial** | Step coach |
