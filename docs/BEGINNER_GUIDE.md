# Build Night Current

A short dark-techno track (~30 seconds). Same song as the in-app tutorial.

**Best way:** open Ferric → **🎓 Tutorial** (Ferric menu, or Ctrl+K → Night Current). Follow **Do this** in the panel. If you get stuck, click **Do this step for me** — it rebuilds the song through that step (saves you time; replaces the open project).

This page is the offline copy. Full feature list: [`FEATURE_GUIDE.md`](FEATURE_GUIDE.md) or **F1**.

**Listen first:** `demo/NightCurrent-Demo.wav` — that’s what you’re aiming for.

---

## The song

| | |
|---|---|
| Title | Night Current |
| BPM | 128 |
| Feel | Dark techno — warm low end, filtered hook |
| Chords | **Cm → Ab → Eb → Bb** (one bar each, loop) |
| Length | 16 bars |

### Sections

| Bars | Section | What’s happening |
|------|---------|------------------|
| 1–2 | Intro | Thin kick + bass |
| 3–4 | Groove | Full drums |
| 5–8 | Hook A | Main synth line |
| 9–12 | Stack | Counter + arp |
| 13–16 | Hook B | Hook comes back louder |

### Screen map

```
TOP      Transport — play, stop, BPM, record
LEFT     Browser — instruments, hardware, samples, plugins
CENTER   Arrangement + Chord Lane
BOTTOM   Dock — MIDI editor, FX, mixer (drag the top edge to resize)
```

**Space** play/stop · **F1** help · **Ctrl+K** palette · **Ctrl+S** save · **Ctrl+B** bounce

---

## Steps

### A — Start a clean 128 BPM session

1. Ferric menu → **New Empty Session** (skip Afterglow if it’s open).
2. Top bar → set **BPM to 128**.
3. **Ctrl+S** → save as `NightCurrent.ferric`.
4. Turn **Loop** on for bars **1–16** (beats 0–64).

Done when: BPM reads 128 and the file is saved.

---

### B — Chord loop

1. Above the timeline, turn on **Chord Lane**.
2. From bar 1 place: **Cm, Ab, Eb, Bb** — one bar each.
3. Repeat until you fill **16 bars**.

Done when: chord blocks cover bars 1–16.

---

### C — Pads, bass, arp

1. Chord Lane toolbar → **Gen / Generate Arr** (Pad + Bass + Arp).
2. Rename tracks to `03 Pads`, `02 Bass`, `04 Arp`.
3. Mute or trim `04 Arp` so it only plays **bars 9–16**.
4. Pull pads down — bed, not a blanket.

Done when: harmony follows the lane; arp stays out of intro and Hook A.

---

### D — Drums

1. Left Browser → **Studio Drum Kit**. Name the track `01 Drums`.
2. Bars **1–2**: kick on **1 and 3** only.
3. Bars **3–16**: kick every beat; clap/snare on **2 and 4**.
4. Soft hats — keep them quiet.

Done when: intro feels thinner than the groove.

---

### E — Hook

1. Browser → **PolySynth** (or Wavetable). Name it `05 Hook`.
2. At **bar 5**, draw ~8 bars of MIDI **an octave down**, e.g.

```
C3  G2  Bb2  C3 | Eb3  C3  G2
Ab2 Bb2 C3      | G2  (hold)
```

Bright C4 leads sound like stock EDM — keep it low.

3. Copy that clip to **bar 13** (Hook B) and nudge it louder.
4. Lower filter cutoff. Add short **Delay** + light **Reverb** on the track FX.

Done when: the hook sits where the demo’s lead sits.

---

### F — Counter + layout

1. Add a synth/MIDI track named `06 Counter`.
2. Higher answering phrase on **bars 9–12**.
3. Softer (or shorter) under **bars 13–16**.
4. Mute/unmute so sections match:

| Bars | Pads | Bass | Arp | Drums | Hook | Counter |
|------|------|------|-----|-------|------|---------|
| 1–2 | soft | soft | off | sparse | off | off |
| 3–4 | on | on | off | full | off | off |
| 5–8 | on | on | off | full | **on** | off |
| 9–12 | on | on | **on** | full | off/soft | **on** |
| 13–16 | on | on | on | full | **on** | soft |

Done when: eyes closed, the sections feel like the demo.

---

### G / H — Sidechain

1. Select `03 Pads`.
2. Bottom dock → **FX** → add **Compressor**.
3. Set Sidechain source to `01 Drums`.
4. Keep it gentle — pads breathe, don’t choke.
5. Optional: same on `02 Bass`.

Done when: pads duck a little with the kick.

---

### I — Mix

1. Open **Mixer** in the bottom dock.
2. Kick loudest → bass → hook clear → pads/arp tucked.
3. Store **Mix A**.

---

### J — Bounce

1. **Ctrl+B** (or Ferric menu → Bounce).
2. Range: beats **0 → 64**.
3. WAV, **−14 LUFS** on.
4. Save as `NightCurrent_Master.wav`.
5. A/B against `demo/NightCurrent-Demo.wav` (Play demo WAV in the tutorial, or Golden Bounce after export).

---

## Checklist

- [ ] `NightCurrent.ferric` at 128 BPM
- [ ] Cm–Ab–Eb–Bb across 16 bars
- [ ] Layout matches the table
- [ ] Bounce holds up next to the demo

Later: USB MIDI for the hook, Line-In texture, one automation move into Hook B.
