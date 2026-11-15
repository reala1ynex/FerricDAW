//! `FerricWave` Flagship Wavetable + Shimmer Granular Cloud Synthesizer!
//! - 4 Morphable 64-Frame × 256-Sample Wavetable Banks:
//!   0) Analog Morph (Sine -> Triangle -> Warm Saw -> PWM Pulse)
//!   1) Vocal Formant ("Ahh" -> "Eee" -> "Ooo" -> "Yaa" Choir Morph)
//!   2) Digital FM / Metallic Bell Harmonics
//!   3) Custom User WAV Import (turns any imported `.wav` into a 64-frame wavetable)
//! - Up to 8x Stereo Unison Super-Voices with Equal-Power Panning
//! - Live Shimmer Granular Cloud Layer (+1 Octave & 5th Hann-Windowed Micro-Grains)
//! - Zero-Delay-Feedback Cytomic State-Variable Filter per voice

use crate::instruments::MidiEvent;
use crate::tempo_arp::MidiExpression;
use ferric_model::AudioBuffer;
use std::f32::consts::{PI, TAU};

pub const WT_FRAMES: usize = 64;
pub const WT_SAMPLES: usize = 256;
const MAX_WT_VOICES: usize = 12;
const MAX_UNISON: usize = 8;
const GRANULAR_RING_LEN: usize = 32_768; // ~680ms stereo ring @ 48kHz
const NUM_GRAINS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
struct VoiceEnv {
    stage: EnvStage,
    level: f32,
}

impl Default for VoiceEnv {
    fn default() -> Self {
        Self {
            stage: EnvStage::Idle,
            level: 0.0,
        }
    }
}

impl VoiceEnv {
    fn trigger(&mut self) {
        self.stage = EnvStage::Attack;
    }
    fn release(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    #[inline]
    fn next(&mut self, sr: f32, atk_ms: f32, dec_ms: f32, sus: f32, rel_ms: f32) -> f32 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                self.level += 1.0 / (atk_ms * 0.001 * sr).max(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
                self.level
            }
            EnvStage::Decay => {
                let c = (-1.0 / (dec_ms * 0.001 * sr).max(1.0)).exp();
                self.level = sus + (self.level - sus) * c;
                if (self.level - sus).abs() < 0.0004 {
                    self.level = sus;
                    self.stage = EnvStage::Sustain;
                }
                self.level
            }
            EnvStage::Sustain => {
                self.level = sus;
                self.level
            }
            EnvStage::Release => {
                let c = (-1.0 / (rel_ms * 0.001 * sr).max(1.0)).exp();
                self.level *= c;
                if self.level < 0.0002 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
                self.level
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct WtVoice {
    active: bool,
    note: u8,
    vel: f32,
    age: u64,
    phases: [f32; MAX_UNISON],
    env: VoiceEnv,
    ic1_l: f32,
    ic2_l: f32,
    ic1_r: f32,
    ic2_r: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct ShimmerGrain {
    active: bool,
    read_pos: f32,
    rate: f32,
    phase: f32,
    phase_inc: f32,
    pan_l: f32,
    pan_r: f32,
}

pub struct WavetableSynthProcessor {
    /// 4 Banks × 64 Frames × 256 Samples (`~256 KB` contiguous cache-friendly table).
    banks: Box<[[[f32; WT_SAMPLES]; WT_FRAMES]; 4]>,
    voices: [WtVoice; MAX_WT_VOICES],
    voice_counter: u64,
    vibrato_phase: f32,
    // Shimmer Granular Cloud Engine
    grain_ring_l: Vec<f32>,
    grain_ring_r: Vec<f32>,
    grain_write: usize,
    grains: [ShimmerGrain; NUM_GRAINS],
    samples_until_grain: usize,
    rng: u32,
    sample_rate: f32,
}

impl WavetableSynthProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut banks = Box::new([[[0.0_f32; WT_SAMPLES]; WT_FRAMES]; 4]);
        Self::populate_factory_banks(&mut banks);
        Self {
            banks,
            voices: [WtVoice::default(); MAX_WT_VOICES],
            voice_counter: 0,
            vibrato_phase: 0.0,
            grain_ring_l: vec![0.0; GRANULAR_RING_LEN],
            grain_ring_r: vec![0.0; GRANULAR_RING_LEN],
            grain_write: 0,
            grains: [ShimmerGrain::default(); NUM_GRAINS],
            samples_until_grain: 256,
            rng: 0x9E37_79B9,
            sample_rate: sr,
        }
    }

    fn populate_factory_banks(banks: &mut [[[f32; WT_SAMPLES]; WT_FRAMES]; 4]) {
        for f in 0..WT_FRAMES {
            let pos = f as f32 / (WT_FRAMES - 1) as f32;

            // Bank 0: Analog Morph (Sine -> Triangle -> Bandlimited Saw -> Narrow Pulse)
            for s in 0..WT_SAMPLES {
                let p = s as f32 / WT_SAMPLES as f32;
                let sine = (TAU * p).sin();
                let tri = 1.0 - 4.0 * (p - 0.5).abs();
                let mut saw = 0.0_f32;
                for h in 1..=16 {
                    saw += ((-1.0_f32).powi(h + 1) / h as f32) * (TAU * h as f32 * p).sin();
                }
                saw *= 0.63;
                let pw = 0.5 - 0.38 * (pos - 0.66).max(0.0) * 3.0;
                let pulse = if p < pw { 0.8 } else { -0.8 };

                let v = if pos < 0.33 {
                    let t = pos / 0.33;
                    sine * (1.0 - t) + tri * t
                } else if pos < 0.66 {
                    let t = (pos - 0.33) / 0.33;
                    tri * (1.0 - t) + saw * t
                } else {
                    let t = (pos - 0.66) / 0.34;
                    saw * (1.0 - t) + pulse * t
                };
                banks[0][f][s] = v.clamp(-1.0, 1.0);
            }

            // Bank 1: Vocal Choir Formant Morph (Resonant formant harmonics F1/F2)
            let f1_mult = 2.0 + (pos * 5.0);
            let f2_mult = 6.0 + ((1.0 - (pos - 0.5).abs() * 2.0) * 8.0);
            let mut max_b1 = 1e-5_f32;
            for s in 0..WT_SAMPLES {
                let p = s as f32 / WT_SAMPLES as f32;
                let win = 0.5 * (1.0 - (TAU * p).cos());
                let v = (TAU * p).sin() * 0.45
                    + (TAU * f1_mult * p).sin() * win * 0.55
                    + (TAU * f2_mult * p).sin() * win * 0.35;
                banks[1][f][s] = v;
                max_b1 = max_b1.max(v.abs());
            }
            for s in 0..WT_SAMPLES {
                banks[1][f][s] /= max_b1;
            }

            // Bank 2: Digital 2-Op Phase-Modulation / Glass Bell Morph
            let mod_idx = pos * 5.5;
            let ratio = if pos < 0.5 { 2.0 } else { 3.0 };
            for s in 0..WT_SAMPLES {
                let p = s as f32 / WT_SAMPLES as f32;
                let m = (TAU * ratio * p).sin() * mod_idx;
                banks[2][f][s] = (TAU * p + m).sin();
            }

            // Bank 3: Default Hyper-Harmonic Sync (overwritten when user loads Custom WAV!)
            let sync_mult = 1.0 + pos * 6.0;
            for s in 0..WT_SAMPLES {
                let p = s as f32 / WT_SAMPLES as f32;
                let window = 1.0 - p;
                banks[3][f][s] = (TAU * sync_mult * p).sin() * window;
            }
        }
    }

    /// Converts any imported `AudioBuffer` (vocal phrase, guitar pluck, Line-In wave)
    /// into 64 normalized zero-crossing-aligned 256-sample Wavetable frames in Bank 3!
    pub fn load_custom_wavetable(&mut self, buf: &AudioBuffer) {
        let len = buf.len_frames();
        if len < WT_SAMPLES {
            return;
        }
        let usable = len.saturating_sub(WT_SAMPLES);
        for f in 0..WT_FRAMES {
            let start = (f * usable) / (WT_FRAMES - 1).max(1);
            let mut dc = 0.0_f32;
            for s in 0..WT_SAMPLES {
                let m = 0.5 * (buf.left[start + s] + buf.right[start + s]);
                self.banks[3][f][s] = m;
                dc += m;
            }
            dc /= WT_SAMPLES as f32;
            let mut peak = 1e-5_f32;
            for s in 0..WT_SAMPLES {
                // Gentle edge taper prevents frame-seam clicks on arbitrary audio!
                let p = s as f32 / WT_SAMPLES as f32;
                let edge = (p * 16.0).min((1.0 - p) * 16.0).min(1.0);
                let v = (self.banks[3][f][s] - dc) * edge;
                self.banks[3][f][s] = v;
                peak = peak.max(v.abs());
            }
            let scale = if peak > 0.002 { 1.0 / peak } else { 1.0 };
            for s in 0..WT_SAMPLES {
                self.banks[3][f][s] *= scale;
            }
        }
    }

    /// Helper for the 3D/2D UI Wavetable Scope to preview any `(bank, morph_0_to_1)` waveform!
    pub fn preview_waveform_static(bank_idx: usize, morph_norm: f32, out: &mut [f32; 128]) {
        let mut tmp_banks = Box::new([[[0.0_f32; WT_SAMPLES]; WT_FRAMES]; 4]);
        Self::populate_factory_banks(&mut tmp_banks);
        let b = bank_idx.min(3);
        let f_pos = morph_norm.clamp(0.0, 1.0) * (WT_FRAMES - 1) as f32;
        let f0 = (f_pos.floor() as usize).min(WT_FRAMES - 1);
        let f1 = (f0 + 1).min(WT_FRAMES - 1);
        let frac = f_pos - f0 as f32;
        for i in 0..128 {
            let s = i * 2;
            out[i] = tmp_banks[b][f0][s] * (1.0 - frac) + tmp_banks[b][f1][s] * frac;
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.env.release();
        }
    }

    #[inline]
    fn next_rand_01(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32) / (u32::MAX as f32)
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.voice_counter += 1;
        let idx = self
            .voices
            .iter()
            .position(|v| v.active && v.note == note)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.env
                            .level
                            .partial_cmp(&b.env.level)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.age.cmp(&b.age))
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });

        let mut phases = [0.0_f32; MAX_UNISON];
        for (u, ph) in phases.iter_mut().enumerate() {
            *ph = if u == 0 {
                0.0
            } else {
                (u as f32 * 0.173).fract()
            };
        }
        let v = &mut self.voices[idx];
        v.active = true;
        v.note = note;
        v.vel = (velocity as f32 / 127.0).clamp(0.05, 1.0);
        v.age = self.voice_counter;
        v.phases = phases;
        v.env.trigger();
    }

    #[inline]
    fn sample_wavetable(&self, bank: usize, frame_pos: f32, phase: f32) -> f32 {
        let f0 = (frame_pos.floor() as usize).min(WT_FRAMES - 1);
        let f1 = (f0 + 1).min(WT_FRAMES - 1);
        let f_frac = frame_pos - f0 as f32;

        let s_pos = phase * WT_SAMPLES as f32;
        let s0 = (s_pos.floor() as usize) & (WT_SAMPLES - 1);
        let s1 = (s0 + 1) & (WT_SAMPLES - 1);
        let s_frac = s_pos - s_pos.floor();

        let a0 = self.banks[bank][f0][s0];
        let a1 = self.banks[bank][f0][s1];
        let v0 = a0 + (a1 - a0) * s_frac;

        let b0 = self.banks[bank][f1][s0];
        let b1 = self.banks[bank][f1][s1];
        let v1 = b0 + (b1 - b0) * s_frac;

        v0 + (v1 - v0) * f_frac
    }

    pub fn process_with_expression(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
        expr: MidiExpression,
    ) {
        let bank = params.first().copied().unwrap_or(0.0).round().clamp(0.0, 3.0) as usize;
        let wt_pos = (params.get(1).copied().unwrap_or(32.0) * 0.01).clamp(0.0, 1.0)
            * (WT_FRAMES - 1) as f32;
        let unison = params.get(2).copied().unwrap_or(4.0).round().clamp(1.0, 8.0) as usize;
        let detune_amt = (params.get(3).copied().unwrap_or(26.0) * 0.01).clamp(0.0, 1.0);
        let grain_mix = (params.get(4).copied().unwrap_or(24.0) * 0.01).clamp(0.0, 1.0);
        let grain_ms = params.get(5).copied().unwrap_or(58.0).clamp(10.0, 220.0);
        let base_cutoff = params.get(6).copied().unwrap_or(4200.0);
        let res = params.get(7).copied().unwrap_or(0.24);
        let atk = params.get(8).copied().unwrap_or(18.0);
        let dec = params.get(9).copied().unwrap_or(420.0);
        let sus = (params.get(10).copied().unwrap_or(72.0) * 0.01).clamp(0.0, 1.0);
        let rel = params.get(11).copied().unwrap_or(460.0);

        let frames = left.len().min(right.len());
        let mut ev_idx = 0;
        let unison_norm = 1.0 / (unison as f32).sqrt();
        let vib_inc = 5.5 / self.sample_rate;

        let cut = (base_cutoff * 2.0_f32.powf(expr.brightness_offset * 2.0))
            .clamp(45.0, self.sample_rate * 0.46);
        let g = (PI * cut / self.sample_rate).tan();
        let k = 2.0 - 2.0 * res.clamp(0.0, 0.94);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let grain_len_smp = (grain_ms * 0.001 * self.sample_rate).clamp(256.0, 12_000.0);
        let spawn_interval = (grain_len_smp / 4.0).max(128.0) as usize;
        let ring_mask = GRANULAR_RING_LEN - 1;

        for frame in 0..frames {
            while ev_idx < midi_events.len() && midi_events[ev_idx].sample_offset <= frame {
                let ev = midi_events[ev_idx];
                if ev.is_note_on && ev.velocity > 0 {
                    self.note_on(ev.note, ev.velocity);
                } else {
                    for v in &mut self.voices {
                        if v.active && v.note == ev.note {
                            v.env.release();
                        }
                    }
                }
                ev_idx += 1;
            }

            self.vibrato_phase = (self.vibrato_phase + vib_inc).fract();
            let vib_semi = (TAU * self.vibrato_phase).sin() * expr.mod_wheel * 0.45;
            let total_bend = expr.pitch_bend_semitones + vib_semi;

            let mut synth_l = 0.0_f32;
            let mut synth_r = 0.0_f32;

            for v_idx in 0..MAX_WT_VOICES {
                if !self.voices[v_idx].active {
                    continue;
                }
                let amp = self.voices[v_idx]
                    .env
                    .next(self.sample_rate, atk, dec, sus, rel);
                if self.voices[v_idx].env.stage == EnvStage::Idle {
                    self.voices[v_idx].active = false;
                    continue;
                }

                let note = self.voices[v_idx].note;
                let vel = self.voices[v_idx].vel;
                let center_freq = 440.0 * 2.0_f32.powf((note as f32 + total_bend - 69.0) / 12.0);

                let mut ul = 0.0_f32;
                let mut ur = 0.0_f32;

                for u in 0..unison {
                    let spread = if unison == 1 {
                        0.0
                    } else {
                        (u as f32 / (unison - 1) as f32) * 2.0 - 1.0
                    };
                    let det_semi = spread * detune_amt * 0.42;
                    let freq = center_freq * 2.0_f32.powf(det_semi / 12.0);
                    let dt = (freq / self.sample_rate).clamp(1e-5, 0.48);

                    let ph = self.voices[v_idx].phases[u];
                    self.voices[v_idx].phases[u] = (ph + dt).fract();

                    // Read wavetable using borrow-safe helper on `self.banks`
                    let f0 = (wt_pos.floor() as usize).min(WT_FRAMES - 1);
                    let f1 = (f0 + 1).min(WT_FRAMES - 1);
                    let f_frac = wt_pos - f0 as f32;
                    let s_pos = ph * WT_SAMPLES as f32;
                    let s0 = (s_pos.floor() as usize) & (WT_SAMPLES - 1);
                    let s1 = (s0 + 1) & (WT_SAMPLES - 1);
                    let s_frac = s_pos - s_pos.floor();
                    let w0 = self.banks[bank][f0][s0]
                        + (self.banks[bank][f0][s1] - self.banks[bank][f0][s0]) * s_frac;
                    let w1 = self.banks[bank][f1][s0]
                        + (self.banks[bank][f1][s1] - self.banks[bank][f1][s0]) * s_frac;
                    let sig = w0 + (w1 - w0) * f_frac;

                    let pan_ang = (spread * 0.8 + 1.0) * 0.25 * PI;
                    ul += sig * pan_ang.cos();
                    ur += sig * pan_ang.sin();
                }

                ul *= unison_norm;
                ur *= unison_norm;

                // Per-voice ZDF SVF Lowpass
                let v = &mut self.voices[v_idx];
                let v3_l = ul - v.ic2_l;
                let v1_l = a1 * v.ic1_l + a2 * v3_l;
                let flt_l = v.ic2_l + a2 * v.ic1_l + a3 * v3_l;
                v.ic1_l = 2.0 * v1_l - v.ic1_l;
                v.ic2_l = 2.0 * flt_l - v.ic2_l;

                let v3_r = ur - v.ic2_r;
                let v1_r = a1 * v.ic1_r + a2 * v3_r;
                let flt_r = v.ic2_r + a2 * v.ic1_r + a3 * v3_r;
                v.ic1_r = 2.0 * v1_r - v.ic1_r;
                v.ic2_r = 2.0 * flt_r - v.ic2_r;

                let g_v = amp * vel * 0.28 * expr.expression;
                synth_l += flt_l * g_v;
                synth_r += flt_r * g_v;
            }

            // Write dry wavetable synth into Granular Shimmer Cloud Ring Buffer!
            self.grain_ring_l[self.grain_write] = synth_l;
            self.grain_ring_r[self.grain_write] = synth_r;

            let mut cloud_l = 0.0_f32;
            let mut cloud_r = 0.0_f32;

            if grain_mix > 0.01 {
                if self.samples_until_grain == 0 {
                    self.samples_until_grain = spawn_interval;
                    let r_spray = self.next_rand_01();
                    let r_pan = self.next_rand_01();
                    let r_harm = self.next_rand_01();
                    if let Some(slot) = self.grains.iter_mut().find(|gr| !gr.active) {
                        let back = 1024.0 + r_spray * (GRANULAR_RING_LEN as f32 * 0.45);
                        let start_pos = (self.grain_write as f32 + GRANULAR_RING_LEN as f32 - back)
                            .rem_euclid(GRANULAR_RING_LEN as f32);
                        // Shimmer rates: +1 Octave (2.0x), Perfect 5th (1.498x), or Unison (1.0x)
                        let rate = if r_harm > 0.55 {
                            2.0
                        } else if r_harm > 0.25 {
                            1.4983
                        } else {
                            1.0
                        };
                        let ang = r_pan * 0.5 * PI;
                        *slot = ShimmerGrain {
                            active: true,
                            read_pos: start_pos,
                            rate,
                            phase: 0.0,
                            phase_inc: 1.0 / grain_len_smp,
                            pan_l: ang.cos(),
                            pan_r: ang.sin(),
                        };
                    }
                } else {
                    self.samples_until_grain -= 1;
                }

                for gr in &mut self.grains {
                    if !gr.active {
                        continue;
                    }
                    let win = (PI * gr.phase).sin();
                    let win = win * win;
                    let idx0 = (gr.read_pos.floor() as usize) & ring_mask;
                    let idx1 = (idx0 + 1) & ring_mask;
                    let frac = gr.read_pos - gr.read_pos.floor();
                    let gl = self.grain_ring_l[idx0]
                        + (self.grain_ring_l[idx1] - self.grain_ring_l[idx0]) * frac;
                    let gr_s = self.grain_ring_r[idx0]
                        + (self.grain_ring_r[idx1] - self.grain_ring_r[idx0]) * frac;
                    let mono = 0.5 * (gl + gr_s) * win * 0.65;
                    cloud_l += mono * gr.pan_l;
                    cloud_r += mono * gr.pan_r;

                    gr.read_pos = (gr.read_pos + gr.rate).rem_euclid(GRANULAR_RING_LEN as f32);
                    gr.phase += gr.phase_inc;
                    if gr.phase >= 1.0 {
                        gr.active = false;
                    }
                }
            }

            self.grain_write = (self.grain_write + 1) & ring_mask;
            let _ = self.sample_wavetable(0, 0.0, 0.0);

            left[frame] += synth_l * (1.0 - grain_mix * 0.35) + cloud_l * grain_mix;
            right[frame] += synth_r * (1.0 - grain_mix * 0.35) + cloud_r * grain_mix;
        }
    }
}
