# FerricDAW Beginner Tutorial  
## Build **Night Current** (dark techno)

**Preferred:** in the app click **🎓 Tutorial** (or Ferric menu → **Night Current Tutorial**, or Ctrl+K → “Night Current”).  
The coach walks you end-to-end: session → chords → Generate Arr → drums → hook → counter → sidechain → mix → bounce.  
Use **Do this step** when you want Ferric to perform a step; otherwise do it yourself and hit **Next**.

This markdown is the same recipe written out for offline reading.

When you’re done, it should match the **layout and feel** of the official demo:

**`demo/NightCurrent-Demo.wav`** ← play this first. That’s the finished product you’re aiming at.

(That WAV is bounced from the same tutorial recipe in CI/tests — so the guide and the demo stay honest.)

---

# 0. What you’re making

| | |
|---|---|
| **Title** | Night Current |
| **BPM** | **128** |
| **Feel** | Dark techno — warm night, heavy low end, synth hook |
| **Key loop** | **Cm → Ab → Eb → Bb** (one bar each, loop) |
| **Length** | **16 bars** (~30 seconds) |

## Song layout (same as the demo)

| Bars | Section | What’s in the demo |
|------|---------|-------------------|
| 1–2 | Intro | Sparse kick + bass |
| 3–4 | Groove in | Full drums |
| 5–8 | Hook A | Main synth hook |
| 9–12 | Stack | Counter line + arp + stabs |
| 13–16 | Hook B | Hook returns bigger over the stack |

## Features you’ll touch (one job each)

| Step | Ferric feature |
|------|----------------|
| A | New session, 128 BPM, save `.ferric` |
| B | Chord Lane |
| C | Generate Arr (Pads / Bass / Arp) |
| D | Drum Kit + Piano Roll / grid |
| E | Hook lead MIDI (PolySynth / Wavetable) |
| F | Arrangement mute/clip layout + counter |
| G | FX on the hook (reverb/delay) |
| H | Light sidechain (pad → kick) |
| I | Mixer + Mix A snapshot |
| J | Bounce master (WAV/FLAC, −14 LUFS) |
| *(optional)* | LPK25 / DS Line-In as color |

Skip for later: VST3 deep-dive, freeze, stem split, tempo-map ramps, full VCA console.

---

# 1. Short Ferric intro

```
TOP:     Transport (Play/Stop, BPM, record)
LEFT:    Browser (instruments, rig, samples, plugins)
CENTER:  Arrangement + Chord Lane  ← Night Current lives here
BOTTOM:  Dock (MIDI editor, FX, Mixer) — drag the top edge to resize
FOOTER:  Status
```

| Key | Action |
|-----|--------|
| **Space** | Play / Stop |
| **F1** | Quick help |
| **Ctrl+K** | Command palette |
| **Ctrl+S** | Save |
| **Ctrl+Z** | Undo |
| **Ctrl+B** | Bounce |

Play `demo/NightCurrent-Demo.wav`, then open Ferric.

---

# 2. Build Night Current

### A — New session

1. Launch `ferricdaw.exe`.
2. **Save As** → `NightCurrent.ferric`.
3. Set **BPM = 128**.
4. Clear or ignore Afterglow Copper if it’s loaded — this tutorial is a clean build.

**Checkpoint:** BPM reads 128; project saved.

---

### B — Chord Lane

1. Enable **Chord Lane**.
2. From bar 1, loop for **16 bars**:

   **Cm → Ab → Eb → Bb** (1 bar each)

**Checkpoint:** Chord blocks fill bars 1–16.

---

### C — Generate Pads, Bass, Arp

1. **Gen** / **Generate Arr** (Pad + Bass + Arp).
2. Rename → `03 Pads`, `02 Bass`, `04 Arp`.
3. Mute or trim **Arp** so it only plays **bars 9–16** (Stack + Hook B).
4. Keep pads relatively quiet (techno bed, not a blanket).

**Checkpoint:** Harmony moves with the lane; arp stays out of the intro/hook-A.

---

### D — Drums

1. Add **Studio Drum Kit** → `01 Drums`.
2. Pattern:
   - Bars **1–2**: kick on **1 and 3** only  
   - Bars **3–16**: kick every beat  
   - Clap/snare on **2 and 4** from bar 3  
   - Soft hats (don’t turn them into white-noise “rice”)

**Checkpoint:** Intro thinner than the groove; 128 BPM is solid.

---

### E — Hook lead

1. Add **PolySynth** or **Wavetable** → `05 Hook`.
2. Clip **Hook A** at **bar 5** (8 bars of phrase):

```
C3  G2  Bb2  C3 | Eb3  C3  G2
Ab2 Bb2 C3      | G2  (hold)
```

   (Keep it **an octave down** — bright C4 hooks sound like stock EDM, not dark techno.)

3. Copy to **bar 13** as **Hook B** (a bit louder).
4. Filter the synth cutoff low; short **Delay** + very light **Reverb** only.
5. Optional: EQ high shelf **down** a few dB on pads/arp/hook so the bed stays dark.

**Checkpoint:** Hook lands where the demo’s lead does.

---

### F — Counter + layout table

Add `06 Counter` (higher answering phrase) on **bars 9–12**, softer under **13–16**.

| Bars | Pads | Bass | Arp | Drums | Hook | Counter |
|------|------|------|-----|-------|------|---------|
| 1–2 | soft | soft | off | sparse | off | off |
| 3–4 | on | on | off | full | off | off |
| 5–8 | on | on | off | full | **on** | off |
| 9–12 | on | on | **on** | full | off/soft | **on** |
| 13–16 | on | on | on | full | **on** | soft |

**Checkpoint:** Blind listen matches the demo’s section shape.

---

### G / H — FX + sidechain

1. Hook already has delay/reverb from E.  
2. On `03 Pads`, **Compressor** sidechained to `01 Drums` — gentle pump.

**Checkpoint:** Pads breathe with the kick.

---

### I — Mixer + Mix A

Balance: kick loudest → bass → hook clear → pads/arp tucked.  
Store **Mix A**.

---

### J — Bounce

1. **Ctrl+B** · beats **0 → 64**  
2. Export `NightCurrent_Master.wav`  
3. A/B against `demo/NightCurrent-Demo.wav`

---

# 3. Done when

- [ ] `NightCurrent.ferric` @ 128 BPM  
- [ ] Cm–Ab–Eb–Bb × 16 bars  
- [ ] Layout matches the table  
- [ ] Bounce sits next to the demo without embarrassment  

Optional later: LPK25 live hook, DS Line-In texture, one automation move into Hook B.
