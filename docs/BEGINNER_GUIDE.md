# Build Night Current

A short dark-techno track. About 30 seconds. Same song as the in-app tutorial.

**Easiest path:** open Ferric → **🎓 Tutorial** (or Ferric menu → Night Current Tutorial, or Ctrl+K). The coach on the right walks you through it. Use **Do this step** if you want Ferric to do the work; otherwise do it yourself and hit **Next**.

This page is the same recipe offline. Feature dump for later: [`FEATURE_GUIDE.md`](FEATURE_GUIDE.md) or **F1 → Feature Guide**.

**Listen first:** play `demo/NightCurrent-Demo.wav`. That’s the finish line.

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
| 9–12 | Stack | Counter + arp + stabs |
| 13–16 | Hook B | Hook comes back louder |

---

## Where things live

```
TOP      Transport — play, stop, BPM, record
LEFT     Browser — instruments, hardware, samples, plugins
CENTER   Arrangement + Chord Lane
BOTTOM   Dock — MIDI editor, FX, mixer (drag the top edge to resize)
```

Handy keys: **Space** play/stop · **F1** help · **Ctrl+K** command palette · **Ctrl+S** save · **Ctrl+B** bounce

---

## Steps

### A — New session

1. Launch Ferric.
2. **Save As** → `NightCurrent.ferric`.
3. Set **BPM to 128**.
4. If Afterglow Copper loaded, start fresh (New Empty Session) — this tutorial wants a clean project.

Done when: BPM says 128 and the file is saved.

---

### B — Chord Lane

1. Turn on **Chord Lane**.
2. From bar 1, loop for 16 bars: **Cm → Ab → Eb → Bb** (one bar each).

Done when: chord blocks cover bars 1–16.

---

### C — Pads, bass, arp

1. Hit **Gen** / **Generate Arr** (Pad + Bass + Arp).
2. Rename tracks to `03 Pads`, `02 Bass`, `04 Arp`.
3. Mute or trim the arp so it only plays **bars 9–16**.
4. Keep pads quiet — bed, not a blanket.

Done when: harmony follows the lane; arp stays out of the intro and Hook A.

---

### D — Drums

1. Add **Studio Drum Kit** → name it `01 Drums`.
2. Pattern:
   - Bars **1–2**: kick on **1 and 3** only
   - Bars **3–16**: kick every beat
   - Clap/snare on **2 and 4** from bar 3
   - Soft hats (don’t turn them into static)

Done when: intro feels thinner than the groove.

---

### E — Hook

1. Add **PolySynth** or **Wavetable** → `05 Hook`.
2. Draw **Hook A** at **bar 5** (about 8 bars):

```
C3  G2  Bb2  C3 | Eb3  C3  G2
Ab2 Bb2 C3      | G2  (hold)
```

Keep it **an octave down**. Bright C4 leads sound like stock EDM.

3. Copy to **bar 13** as **Hook B** (a bit louder).
4. Low filter cutoff. Short delay + a touch of reverb.
5. Optional: nudge pads/arp/hook high shelf down so the bed stays dark.

Done when: the hook sits where the demo’s lead sits.

---

### F — Counter + layout

Add `06 Counter` — a higher answering phrase on **bars 9–12**, softer under **13–16**.

| Bars | Pads | Bass | Arp | Drums | Hook | Counter |
|------|------|------|-----|-------|------|---------|
| 1–2 | soft | soft | off | sparse | off | off |
| 3–4 | on | on | off | full | off | off |
| 5–8 | on | on | off | full | **on** | off |
| 9–12 | on | on | **on** | full | off/soft | **on** |
| 13–16 | on | on | on | full | **on** | soft |

Done when: eyes closed, the sections feel like the demo.

---

### G / H — FX + sidechain

1. Hook already has delay/reverb from E.
2. On `03 Pads`, add a **Compressor** sidechained to `01 Drums` — gentle pump.

Done when: pads duck a little with the kick.

---

### I — Mix

Kick loudest → bass → hook clear → pads/arp tucked. Store **Mix A**.

---

### J — Bounce

1. **Ctrl+B**, beats **0 → 64**
2. Export `NightCurrent_Master.wav`
3. A/B against `demo/NightCurrent-Demo.wav` (Golden Bounce helps after export)

---

## Checklist

- [ ] `NightCurrent.ferric` at 128 BPM
- [ ] Cm–Ab–Eb–Bb across 16 bars
- [ ] Layout matches the table above
- [ ] Bounce holds up next to the demo

Later, if you want: USB MIDI for the hook, Line-In texture, one automation move into Hook B.
