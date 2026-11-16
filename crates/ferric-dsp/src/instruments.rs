use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    pub sample_offset: usize,
    pub note: u8,
    pub velocity: u8,
    pub is_note_on: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
struct Adsr {
    stage: EnvStage,
    level: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            stage: EnvStage::Idle,
            level: 0.0,
        }
    }
}

impl Adsr {
    fn trigger(&mut self) {
        self.stage = EnvStage::Attack;
    }

    fn release(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    #[inline]
    fn next_sample(
        &mut self,
        sr: f32,
        attack_ms: f32,
        decay_ms: f32,
        sustain: f32,
        release_ms: f32,
    ) -> f32 {
        match self.stage {
            EnvStage::Idle => 0.0,
            EnvStage::Attack => {
                let step = 1.0 / ((attack_ms * 0.001 * sr).max(1.0));
                self.level += step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
                self.level
            }
            EnvStage::Decay => {
                let coeff = (-1.0 / ((decay_ms * 0.001 * sr).max(1.0))).exp();
                self.level = sustain + (self.level - sustain) * coeff;
                if (self.level - sustain).abs() < 0.0005 {
                    self.level = sustain;
                    self.stage = EnvStage::Sustain;
                }
                self.level
            }
            EnvStage::Sustain => {
                self.level = sustain;
                self.level
            }
            EnvStage::Release => {
                let coeff = (-1.0 / ((release_ms * 0.001 * sr).max(1.0))).exp();
                self.level *= coeff;
                if self.level < 0.0002 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                }
                self.level
            }
        }
    }
}

/// Unconditionally stable Cytomic Zero-Delay-Feedback State Variable Filter.
#[derive(Debug, Clone, Copy, Default)]
struct CytomicSvf {
    ic1eq: f32,
    ic2eq: f32,
}

impl CytomicSvf {
    #[inline]
    fn process_lowpass(&mut self, input: f32, cutoff_hz: f32, res: f32, sr: f32) -> f32 {
        let f = cutoff_hz.clamp(30.0, sr * 0.46);
        let g = (PI * f / sr).tan();
        let k = 2.0 - 2.0 * res.clamp(0.0, 0.96);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = input - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        if self.ic1eq.abs() < 1e-20 {
            self.ic1eq = 0.0;
        }
        if self.ic2eq.abs() < 1e-20 {
            self.ic2eq = 0.0;
        }
        v2
    }
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct SynthVoice {
    active: bool,
    note: u8,
    velocity: f32,
    age: u64,
    phase_main: f32,
    phase_detune: f32,
    phase_sub: f32,
    amp_env: Adsr,
    flt_env: Adsr,
    svf_l: CytomicSvf,
    svf_r: CytomicSvf,
}

const MAX_VOICES: usize = 16;

pub struct PolySynthProcessor {
    voices: [SynthVoice; MAX_VOICES],
    voice_counter: u64,
    vibrato_phase: f32,
    sample_rate: f32,
}

impl PolySynthProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [SynthVoice::default(); MAX_VOICES],
            voice_counter: 0,
            vibrato_phase: 0.0,
            sample_rate: sample_rate.max(8_000.0),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.amp_env.release();
            v.flt_env.release();
        }
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.voice_counter += 1;
        // Reuse existing voice for same pitch, or idle voice, or steal quietest/oldest
        let slot_idx = self
            .voices
            .iter()
            .position(|v| v.active && v.note == note)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.amp_env
                            .level
                            .partial_cmp(&b.amp_env.level)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.age.cmp(&b.age))
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });

        let v = &mut self.voices[slot_idx];
        v.active = true;
        v.note = note;
        v.velocity = (velocity as f32 / 127.0).clamp(0.05, 1.0);
        v.age = self.voice_counter;
        v.amp_env.trigger();
        v.flt_env.trigger();
    }

    fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.active && v.note == note {
                v.amp_env.release();
                v.flt_env.release();
            }
        }
    }

    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
    ) {
        self.process_with_expression(
            left,
            right,
            params,
            midi_events,
            crate::tempo_arp::MidiExpression::default(),
        );
    }

    pub fn process_with_expression(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
        expr: crate::tempo_arp::MidiExpression,
    ) {
        let waveform = params.first().copied().unwrap_or(0.0).round() as i32;
        let sub_detune = params.get(1).copied().unwrap_or(28.0) * 0.01;
        let base_cutoff = params.get(2).copied().unwrap_or(2600.0);
        let resonance = params.get(3).copied().unwrap_or(0.28);
        let flt_env_amt = params.get(4).copied().unwrap_or(45.0) * 0.01;
        let attack = params.get(5).copied().unwrap_or(12.0);
        let decay = params.get(6).copied().unwrap_or(320.0);
        let sustain = (params.get(7).copied().unwrap_or(65.0) * 0.01).clamp(0.0, 1.0);
        let release = params.get(8).copied().unwrap_or(280.0);
        let out_gain = 10.0_f32.powf(params.get(9).copied().unwrap_or(-3.0) / 20.0)
            * expr.expression.clamp(0.0, 1.5);

        let frames = left.len().min(right.len());
        let mut ev_idx = 0;
        let vib_inc = 5.6 / self.sample_rate;

        for frame in 0..frames {
            while ev_idx < midi_events.len() && midi_events[ev_idx].sample_offset <= frame {
                let ev = midi_events[ev_idx];
                if ev.is_note_on && ev.velocity > 0 {
                    self.note_on(ev.note, ev.velocity);
                } else {
                    self.note_off(ev.note);
                }
                ev_idx += 1;
            }

            self.vibrato_phase = (self.vibrato_phase + vib_inc).fract();
            let vib_semi = (TAU * self.vibrato_phase).sin() * expr.mod_wheel * 0.45;
            let total_bend_semi = expr.pitch_bend_semitones + vib_semi;

            let mut sum_l = 0.0_f32;
            let mut sum_r = 0.0_f32;

            for v in &mut self.voices {
                if !v.active {
                    continue;
                }
                let amp = v.amp_env.next_sample(self.sample_rate, attack, decay, sustain, release);
                if v.amp_env.stage == EnvStage::Idle {
                    v.active = false;
                    continue;
                }
                let f_env = v.flt_env.next_sample(
                    self.sample_rate,
                    attack.min(25.0),
                    decay,
                    sustain * 0.5,
                    release,
                );

                let freq =
                    440.0 * 2.0_f32.powf((v.note as f32 + total_bend_semi - 69.0) / 12.0);
                let dt = (freq / self.sample_rate).clamp(1e-5, 0.45);
                let detune_ratio = 1.0 + 0.0085 * sub_detune;
                let dt_det = (dt * detune_ratio).min(0.45);
                let dt_sub = dt * 0.5;

                let osc_a = Self::eval_osc(waveform, v.phase_main, dt);
                let osc_b = Self::eval_osc(waveform, v.phase_detune, dt_det);
                let sub = (TAU * v.phase_sub).sin() * (0.45 * sub_detune);

                v.phase_main = (v.phase_main + dt).fract();
                v.phase_detune = (v.phase_detune + dt_det).fract();
                v.phase_sub = (v.phase_sub + dt_sub).fract();

                let raw_l = osc_a * 0.65 + osc_b * (0.25 * sub_detune) + sub;
                let raw_r = osc_a * (0.45 + 0.2 * (1.0 - sub_detune)) + osc_b * (0.45 * sub_detune) + sub;

                let key_track = 2.0_f32.powf((v.note as f32 - 60.0) / 24.0);
                let mod_octaves = f_env * flt_env_amt * 3.5 + expr.brightness_offset * 2.0;
                let cutoff = (base_cutoff * key_track * 2.0_f32.powf(mod_octaves))
                    .clamp(45.0, self.sample_rate * 0.45);

                let flt_l = v.svf_l.process_lowpass(raw_l, cutoff, resonance, self.sample_rate);
                let flt_r = v.svf_r.process_lowpass(raw_r, cutoff, resonance, self.sample_rate);

                let gain = amp * v.velocity * 0.24 * out_gain;
                sum_l += flt_l * gain;
                sum_r += flt_r * gain;
            }

            left[frame] += sum_l;
            right[frame] += sum_r;
        }
    }

    #[inline]
    fn eval_osc(waveform: i32, phase: f32, dt: f32) -> f32 {
        match waveform {
            // PolyBLEP Saw
            0 => {
                let mut s = 2.0 * phase - 1.0;
                s -= poly_blep(phase, dt);
                s
            }
            // PolyBLEP Pulse
            1 => {
                let pw = 0.48;
                let mut s = if phase < pw { 1.0 } else { -1.0 };
                s += poly_blep(phase, dt);
                s -= poly_blep((phase + 1.0 - pw).fract(), dt);
                s
            }
            // Warm Analog Triangle
            2 => 1.0 - 4.0 * (phase - 0.5).abs(),
            // 2-Op Electric Piano / Bell FM
            _ => {
                let mod_sig = (TAU * phase * 2.0).sin() * 1.65;
                (TAU * phase + mod_sig).sin()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DrumVoice {
    active: bool,
    pad: u8,
    vel: f32,
    t: f32,
    phase: f32,
    phase2: f32,
    hp_state: f32,
}

pub struct DrumKitProcessor {
    voices: [DrumVoice; 12],
    rng_state: u32,
    sample_rate: f32,
}

impl DrumKitProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [DrumVoice::default(); 12],
            rng_state: 0xA341_316C,
            sample_rate: sample_rate.max(8_000.0),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    #[inline]
    fn next_noise(&mut self) -> f32 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 17;
        self.rng_state ^= self.rng_state << 5;
        (self.rng_state as i32 as f32) / (i32::MAX as f32)
    }

    fn map_note_to_pad(note: u8) -> u8 {
        match note {
            35 | 36 => 0,       // Kick
            37 => 1,            // Rimshot
            38 | 40 => 2,       // Snare
            39 => 3,            // Clap
            42 | 44 => 4,       // Closed Hat
            46 => 5,            // Open Hat
            41 | 43 | 45 => 6,  // Low/Mid Tom
            47 | 48 | 49 | 51 => 7, // High Tom / Crash Ride
            other => other % 8,
        }
    }

    fn trigger(&mut self, note: u8, velocity: u8) {
        let pad = Self::map_note_to_pad(note);
        // Closed hat chokes open hat naturally!
        if pad == 4 {
            for v in &mut self.voices {
                if v.active && v.pad == 5 {
                    v.active = false;
                }
            }
        }
        let idx = self
            .voices
            .iter()
            .position(|v| !v.active || v.pad == pad)
            .unwrap_or(0);
        self.voices[idx] = DrumVoice {
            active: true,
            pad,
            vel: (velocity as f32 / 127.0).clamp(0.08, 1.0),
            t: 0.0,
            phase: 0.0,
            phase2: 0.0,
            hp_state: 0.0,
        };
    }

    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
    ) {
        let kick_punch = params.first().copied().unwrap_or(68.0) * 0.01;
        let kick_tune = params.get(1).copied().unwrap_or(52.0);
        let snare_snap = params.get(2).copied().unwrap_or(64.0) * 0.01;
        let hat_decay_s = params.get(3).copied().unwrap_or(110.0) * 0.001;
        let glue = params.get(4).copied().unwrap_or(22.0) * 0.01;
        let level = 10.0_f32.powf(params.get(5).copied().unwrap_or(0.0) / 20.0);
        let kit = params.get(6).copied().unwrap_or(0.0).round().clamp(0.0, 2.0) as usize;
        let [kick_sweep, kick_decay, beater, beater_tone] = [
            [165.0, 7.2, 0.25, 0.0],
            [215.0, 13.0, 0.42, 0.18],
            [270.0, 18.0, 0.41, 0.57],
        ][kit];
        let [snare_tune, snare_decay, wire_decay, wire_level] = [
            [182.0, 19.0, 13.0, 0.45],
            [154.0, 24.0, 19.0, 0.60],
            [201.0, 16.0, 15.0, 0.67],
        ][kit];
        let [rim_fundamental, rim_overtone, clap_level] = [
            [460.0, 1180.0, 0.55],
            [340.0, 1560.0, 0.68],
            [530.0, 1960.0, 0.42],
        ][kit];
        let [hat_a, hat_b, tom_tune, tom_decay, crash_decay, crash_length] = [
            [3840.0, 5710.0, 112.0, 8.5, 2.6, 1.40],
            [2970.0, 4670.0, 82.0, 10.0, 5.7, 0.80],
            [4110.0, 6140.0, 97.0, 6.4, 1.95, 2.15],
        ][kit];
        let drive: f32 = 1.0 + glue * 1.4;
        let drive_compensation = drive.sqrt();


        let dt = 1.0 / self.sample_rate;
        let frames = left.len().min(right.len());
        let mut ev_idx = 0;

        for frame in 0..frames {
            while ev_idx < midi_events.len() && midi_events[ev_idx].sample_offset <= frame {
                let ev = midi_events[ev_idx];
                if ev.is_note_on && ev.velocity > 0 {
                    self.trigger(ev.note, ev.velocity);
                }
                ev_idx += 1;
            }

            let noise = self.next_noise();
            let mut sum_l = 0.0_f32;
            let mut sum_r = 0.0_f32;

            for v in &mut self.voices {
                if !v.active {
                    continue;
                }
                let t = v.t;
                v.t += dt;

                let (sig_l, sig_r, max_dur) = match v.pad {
                    // 0: Studio Analog/Acoustic Kick
                    0 => {
                        let sweep = kick_tune + (kick_sweep * kick_punch) * (-t * 44.0).exp();
                        v.phase = (v.phase + sweep * dt).fract();
                        let body = (TAU * v.phase).sin() * (-t * kick_decay).exp();
                        let mut click = noise * (-t * 140.0).exp() * beater * kick_punch;
                        if beater_tone > 0.0 {
                            v.phase2 = (v.phase2 + 2820.0 * dt).fract();
                            click += (TAU * v.phase2).sin() * (-t * 310.0).exp() * beater_tone;
                        }
                        let s = (body + click) * 0.85;
                        (s, s, 0.55)
                    }
                    // 1: Rimshot
                    1 => {
                        v.phase = (v.phase + rim_fundamental * dt).fract();
                        v.phase2 = (v.phase2 + rim_overtone * dt).fract();
                        let s = ((TAU * v.phase).sin() * 0.6 + (TAU * v.phase2).sin() * 0.5)
                            * (-t * 55.0).exp()
                            * 0.55;
                        (s * 0.9, s * 1.1, 0.14)
                    }
                    // 2: Crisp Studio Snare
                    2 => {
                        let f_body = snare_tune + 95.0 * (-t * 38.0).exp();
                        v.phase = (v.phase + f_body * dt).fract();
                        let body = (TAU * v.phase).sin() * (-t * snare_decay).exp() * 0.55;
                        let hp_noise = noise - v.hp_state;
                        v.hp_state += 0.35 * hp_noise;
                        let wires = hp_noise * (-t * (wire_decay - 4.0 * snare_snap)).exp() * (wire_level + 0.4 * snare_snap);
                        let s = body + wires;
                        (s * 0.98, s * 1.02, 0.42)
                    }
                    // 3: Multi-tap Studio Clap
                    3 => {
                        let env = if t < 0.010 {
                            (1.0 - t / 0.010) * 0.7
                        } else if t < 0.021 {
                            (1.0 - (t - 0.010) / 0.011) * 0.8
                        } else {
                            (-((t - 0.021) * 16.0)).exp()
                        };
                        let hp = noise - v.hp_state;
                        v.hp_state += 0.42 * hp;
                        let s = hp * env * clap_level;
                        (s * 1.08, s * 0.92, 0.35)
                    }
                    // 4 & 5: Metallic Closed & Open Hi-Hats
                    4 | 5 => {
                        let decay_rate = if v.pad == 4 {
                            1.0 / hat_decay_s.max(0.025)
                        } else {
                            1.0 / (hat_decay_s * 3.6).max(0.18)
                        };
                        v.phase = (v.phase + hat_a * dt).fract();
                        v.phase2 = (v.phase2 + hat_b * dt).fract();
                        let metal = ((if v.phase < 0.5 { 1.0 } else { -1.0 })
                            + (if v.phase2 < 0.5 { 1.0 } else { -1.0 }))
                            * 0.22;
                        let hp = (noise * 0.75 + metal) - v.hp_state;
                        v.hp_state += 0.65 * hp;
                        let s = hp * (-t * decay_rate).exp() * 0.42;
                        (s * 0.88, s * 1.12, 0.85)
                    }
                    // 6: Studio Tom
                    6 => {
                        let f = tom_tune + 75.0 * (-t * 24.0).exp();
                        v.phase = (v.phase + f * dt).fract();
                        let s = (TAU * v.phase).sin() * (-t * tom_decay).exp() * 0.68;
                        (s * 1.1, s * 0.9, 0.50)
                    }
                    // 7: Shimmer Crash / Ride
                    _ => {
                        v.phase = (v.phase + 2920.0 * dt).fract();
                        v.phase2 = (v.phase2 + 4480.0 * dt).fract();
                        let shimmer = ((TAU * v.phase).sin() * (TAU * v.phase2).cos()) * 0.35;
                        let hp = (noise * 0.55 + shimmer) - v.hp_state;
                        v.hp_state += 0.58 * hp;
                        let s = hp * (-t * crash_decay).exp() * 0.40;
                        (s * 1.12, s * 0.88, crash_length)
                    }
                };

                if t > max_dur {
                    v.active = false;
                }
                sum_l += sig_l * v.vel;
                sum_r += sig_r * v.vel;
            }

            if glue > 0.01 {
                sum_l = (sum_l * drive).tanh() / drive_compensation;
                sum_r = (sum_r * drive).tanh() / drive_compensation;
            }

            left[frame] += sum_l * level;
            right[frame] += sum_r * level;
        }
    }
}

/// Monophonic bass synth with portamento glide (last-note priority).
pub struct MonoBassProcessor {
    sample_rate: f32,
    amp_env: Adsr,
    flt_env: Adsr,
    phase: f32,
    phase_sub: f32,
    current_hz: f32,
    target_hz: f32,
    held_note: Option<u8>,
    velocity: f32,
    svf: CytomicSvf,
    note_stack: Vec<u8>,
}

impl MonoBassProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            amp_env: Adsr::default(),
            flt_env: Adsr::default(),
            phase: 0.0,
            phase_sub: 0.0,
            current_hz: 55.0,
            target_hz: 55.0,
            held_note: None,
            velocity: 0.0,
            svf: CytomicSvf::default(),
            note_stack: Vec::with_capacity(8),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    pub fn all_notes_off(&mut self) {
        self.note_stack.clear();
        self.held_note = None;
        self.amp_env.release();
        self.flt_env.release();
    }

    fn midi_hz(note: u8) -> f32 {
        440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.note_stack.retain(|n| *n != note);
        self.note_stack.push(note);
        let retrig = self.held_note.is_none();
        self.held_note = Some(note);
        self.target_hz = Self::midi_hz(note);
        self.velocity = (velocity as f32 / 127.0).clamp(0.05, 1.0);
        if retrig {
            self.current_hz = self.target_hz;
            self.amp_env.trigger();
            self.flt_env.trigger();
        }
    }

    fn note_off(&mut self, note: u8) {
        self.note_stack.retain(|n| *n != note);
        if self.held_note == Some(note) {
            if let Some(&last) = self.note_stack.last() {
                self.held_note = Some(last);
                self.target_hz = Self::midi_hz(last);
            } else {
                self.held_note = None;
                self.amp_env.release();
                self.flt_env.release();
            }
        }
    }

    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
    ) {
        self.process_with_expression(
            left,
            right,
            params,
            midi_events,
            crate::tempo_arp::MidiExpression::default(),
        );
    }

    pub fn process_with_expression(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
        expr: crate::tempo_arp::MidiExpression,
    ) {
        let waveform = params.first().copied().unwrap_or(0.0).round() as i32;
        let glide_ms = params.get(1).copied().unwrap_or(40.0).max(0.0);
        let base_cutoff = params.get(2).copied().unwrap_or(680.0);
        let resonance = params.get(3).copied().unwrap_or(0.42);
        let flt_env_amt = params.get(4).copied().unwrap_or(55.0) * 0.01;
        let attack = params.get(5).copied().unwrap_or(4.0);
        let decay = params.get(6).copied().unwrap_or(220.0);
        let sustain = (params.get(7).copied().unwrap_or(70.0) * 0.01).clamp(0.0, 1.0);
        let release = params.get(8).copied().unwrap_or(160.0);
        let sub_mix = (params.get(9).copied().unwrap_or(35.0) * 0.01).clamp(0.0, 1.0);
        let out_gain = 10.0_f32.powf(params.get(10).copied().unwrap_or(-2.0) / 20.0)
            * expr.expression.clamp(0.0, 1.5);

        let frames = left.len().min(right.len());
        let mut ev_idx = 0;
        let glide_coeff = if glide_ms < 0.5 {
            1.0
        } else {
            1.0 - (-1.0 / (glide_ms * 0.001 * self.sample_rate).max(1.0)).exp()
        };

        for frame in 0..frames {
            while ev_idx < midi_events.len() && midi_events[ev_idx].sample_offset <= frame {
                let ev = midi_events[ev_idx];
                if ev.is_note_on && ev.velocity > 0 {
                    self.note_on(ev.note, ev.velocity);
                } else {
                    self.note_off(ev.note);
                }
                ev_idx += 1;
            }

            self.current_hz += (self.target_hz - self.current_hz) * glide_coeff;
            let bent = self.current_hz
                * 2.0_f32.powf(expr.pitch_bend_semitones / 12.0);
            let dt = bent / self.sample_rate;

            let amp = self
                .amp_env
                .next_sample(self.sample_rate, attack, decay, sustain, release);
            let fenv = self
                .flt_env
                .next_sample(self.sample_rate, attack * 0.6, decay, sustain, release);

            if amp < 1e-5 && self.held_note.is_none() {
                left[frame] = 0.0;
                right[frame] = 0.0;
                continue;
            }

            self.phase = (self.phase + dt).fract();
            self.phase_sub = (self.phase_sub + dt * 0.5).fract();

            let main = match waveform {
                1 => {
                    let t = self.phase;
                    let raw = if t < 0.5 { 1.0 } else { -1.0 };
                    raw - poly_blep(t, dt) + poly_blep((t + 0.5).fract(), dt)
                }
                2 => {
                    let t = self.phase;
                    1.0 - 4.0 * (t - 0.5).abs()
                }
                3 => (TAU * self.phase).sin(),
                _ => {
                    let t = self.phase;
                    (2.0 * t - 1.0) - poly_blep(t, dt)
                }
            };
            let sub = (TAU * self.phase_sub).sin();
            let osc = main * (1.0 - sub_mix * 0.55) + sub * sub_mix;

            let cutoff = (base_cutoff
                * (1.0 + flt_env_amt * fenv * 3.5)
                * (0.85 + 0.3 * expr.mod_wheel))
                .clamp(40.0, self.sample_rate * 0.42);
            let filtered = self.svf.process_lowpass(osc, cutoff, resonance, self.sample_rate);
            let s = filtered * amp * self.velocity * out_gain * 0.55;
            left[frame] = s;
            right[frame] = s;
        }
    }
}

/// Ferric Prism — dual-layer morphing character synth (VA + FM/formant).
/// One Morph + Prism knob covers bass ? pad ? lead without preset hunting.
pub struct PrismSynthProcessor {
    voices: [PrismVoice; 8],
    voice_counter: u64,
    sample_rate: f32,
    lfo_phase: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct PrismVoice {
    active: bool,
    note: u8,
    velocity: f32,
    age: u64,
    phase_a: f32,
    phase_b: f32,
    phase_mod: f32,
    amp_env: Adsr,
    flt_env: Adsr,
    svf_l: CytomicSvf,
    svf_r: CytomicSvf,
}

impl PrismSynthProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [PrismVoice::default(); 8],
            voice_counter: 0,
            sample_rate: sample_rate.max(8_000.0),
            lfo_phase: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.amp_env.release();
            v.flt_env.release();
        }
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.voice_counter += 1;
        let slot = self
            .voices
            .iter()
            .position(|v| v.active && v.note == note)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.amp_env
                            .level
                            .partial_cmp(&b.amp_env.level)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.age.cmp(&b.age))
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });
        let v = &mut self.voices[slot];
        v.active = true;
        v.note = note;
        v.velocity = (velocity as f32 / 127.0).clamp(0.05, 1.0);
        v.age = self.voice_counter;
        v.amp_env.trigger();
        v.flt_env.trigger();
    }

    fn note_off(&mut self, note: u8) {
        for v in &mut self.voices {
            if v.active && v.note == note {
                v.amp_env.release();
                v.flt_env.release();
            }
        }
    }

    pub fn process_with_expression(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
        expr: crate::tempo_arp::MidiExpression,
    ) {
        // 0 Morph %, 1 Prism (formant) %, 2 Spread %, 3 Drive %, 4 Cutoff Hz,
        // 5 Res, 6 Attack, 7 Decay, 8 Sustain, 9 Release, 10 Output dB
        let morph = (params.first().copied().unwrap_or(42.0) * 0.01).clamp(0.0, 1.0);
        let prism = (params.get(1).copied().unwrap_or(35.0) * 0.01).clamp(0.0, 1.0);
        let spread = (params.get(2).copied().unwrap_or(22.0) * 0.01).clamp(0.0, 1.0);
        let drive = (params.get(3).copied().unwrap_or(18.0) * 0.01).clamp(0.0, 1.0);
        let base_cutoff = params.get(4).copied().unwrap_or(3200.0);
        let resonance = params.get(5).copied().unwrap_or(0.22);
        let attack = params.get(6).copied().unwrap_or(18.0);
        let decay = params.get(7).copied().unwrap_or(380.0);
        let sustain = (params.get(8).copied().unwrap_or(68.0) * 0.01).clamp(0.0, 1.0);
        let release = params.get(9).copied().unwrap_or(420.0);
        let out_gain = 10.0_f32.powf(params.get(10).copied().unwrap_or(-3.0) / 20.0)
            * expr.expression.clamp(0.0, 1.5);

        let frames = left.len().min(right.len());
        let mut ev_idx = 0;
        let lfo_inc = 0.35 / self.sample_rate;

        // Formant centers morph with Prism (A?E?O?U-ish).
        let f1 = 350.0 + prism * 450.0;
        let f2 = 900.0 + prism * 1400.0;

        for frame in 0..frames {
            while ev_idx < midi_events.len() && midi_events[ev_idx].sample_offset <= frame {
                let ev = midi_events[ev_idx];
                if ev.is_note_on && ev.velocity > 0 {
                    self.note_on(ev.note, ev.velocity);
                } else {
                    self.note_off(ev.note);
                }
                ev_idx += 1;
            }

            self.lfo_phase = (self.lfo_phase + lfo_inc).fract();
            let lfo = (TAU * self.lfo_phase).sin();

            let mut sum_l = 0.0_f32;
            let mut sum_r = 0.0_f32;

            for v in &mut self.voices {
                if !v.active && v.amp_env.stage == EnvStage::Idle {
                    continue;
                }
                let amp = v
                    .amp_env
                    .next_sample(self.sample_rate, attack, decay, sustain, release);
                let fenv = v
                    .flt_env
                    .next_sample(self.sample_rate, attack * 0.7, decay, sustain, release);
                if amp < 1e-5 && v.amp_env.stage == EnvStage::Idle {
                    v.active = false;
                    continue;
                }

                let hz = 440.0
                    * 2.0_f32.powf(
                        (v.note as f32 - 69.0 + expr.pitch_bend_semitones + lfo * expr.mod_wheel * 0.35)
                            / 12.0,
                    );
                let dt = hz / self.sample_rate;
                v.phase_a = (v.phase_a + dt).fract();
                v.phase_b = (v.phase_b + dt * (1.0 + spread * 0.012)).fract();
                v.phase_mod = (v.phase_mod + dt * (2.0 + prism * 3.0)).fract();

                // Layer A: anti-aliased saw ? square morph.
                let t = v.phase_a;
                let saw = (2.0 * t - 1.0) - poly_blep(t, dt);
                let sq = {
                    let raw = if t < 0.5 { 1.0 } else { -1.0 };
                    raw - poly_blep(t, dt) + poly_blep((t + 0.5).fract(), dt)
                };
                let layer_a = saw * (1.0 - morph * 0.55) + sq * (morph * 0.55);

                // Layer B: FM + dual formant resonators (the "Prism" character).
                let mod_sig = (TAU * v.phase_mod).sin();
                let carrier = (TAU * v.phase_b + mod_sig * (1.2 + prism * 2.8)).sin();
                // Cheap formant emphasis via waveshaped carrier + spectral tilt.
                let layer_b = (carrier * (1.0 + prism) - carrier.powi(3) * prism * 0.55).tanh();

                let mixed = layer_a * (1.0 - morph) + layer_b * morph;
                let driven = if drive > 0.01 {
                    (mixed * (1.0 + drive * 2.4)).tanh()
                } else {
                    mixed
                };

                let cut = (base_cutoff
                    * (1.0 + fenv * 1.8)
                    * (0.85 + 0.35 * expr.mod_wheel)
                    * (0.7 + 0.6 * (1.0 - morph * 0.4)))
                    .clamp(60.0, self.sample_rate * 0.42);
                let l = v
                    .svf_l
                    .process_lowpass(driven, cut, resonance, self.sample_rate);
                let r = v.svf_r.process_lowpass(
                    driven * (1.0 - spread * 0.15) + layer_b * spread * 0.2,
                    cut * (1.0 + spread * 0.04),
                    resonance,
                    self.sample_rate,
                );
                let g = amp * v.velocity * out_gain * 0.42;
                sum_l += l * g * (1.0 - spread * 0.25);
                sum_r += r * g * (1.0 + spread * 0.15);
            }

            // Mild dual-band "prism" sheen using formant centers as a soft shelf proxy.
            let formant_tilt = ((f1 + f2) * 0.5 / 4000.0).clamp(0.15, 1.2);
            let sheen = ((sum_l + sum_r) * 0.5 * formant_tilt).tanh() * prism * 0.08;
            left[frame] = sum_l + sheen;
            right[frame] = sum_r + sheen;
        }
    }
}
