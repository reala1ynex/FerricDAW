use crate::instruments::MidiEvent;
use ferric_model::AudioBuffer;
use std::f32::consts::PI;

const MAX_SAMPLER_VOICES: usize = 16;

#[derive(Debug, Clone, Copy, Default)]
struct SamplerVoice {
    active: bool,
    releasing: bool,
    note: u8,
    velocity: f32,
    pos_frames: f64,
    end_frame: usize,
    rate: f64,
    env: f32,
    ic1_l: f32,
    ic2_l: f32,
    ic1_r: f32,
    ic2_r: f32,
}

/// 16-Voice Polyphonic Chromatic & Transient-Slicing Studio Sampler!
/// Plays any loaded `AudioBuffer` chromatically across 128 keys OR chops loops across pads (C2 = 36+).
pub struct SamplePlayerProcessor {
    voices: [SamplerVoice; MAX_SAMPLER_VOICES],
    pub buffer: Option<AudioBuffer>,
    pub slices: Vec<usize>,
    default_pluck: AudioBuffer,
    sample_rate: f32,
}

impl SamplePlayerProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let default_pluck = Self::build_default_acoustic_piano_sample(sr as u32);
        Self {
            voices: [SamplerVoice::default(); MAX_SAMPLER_VOICES],
            buffer: None,
            slices: Vec::new(),
            default_pluck,
            sample_rate: sr,
        }
    }

    pub fn set_sample(&mut self, buffer: Option<AudioBuffer>, slices: Vec<usize>) {
        self.buffer = buffer;
        self.slices = slices;
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            v.releasing = true;
        }
    }

    fn build_default_acoustic_piano_sample(sr: u32) -> AudioBuffer {
        // Warm C4 (261.63 Hz) studio electric/acoustic hybrid grand sample so Sampler plays immediately!
        let frames = (sr as usize * 3) / 2;
        let mut l = vec![0.0_f32; frames];
        let mut r = vec![0.0_f32; frames];
        let f0 = 261.6256_f32;
        for i in 0..frames {
            let t = i as f32 / sr as f32;
            let ph = std::f32::consts::TAU * f0 * t;
            let hammer = (-t * 42.0).exp() * (7.0 * ph).sin() * 0.14;
            let body = ph.sin() * 0.62
                + (2.0 * ph).sin() * 0.25 * (-t * 2.5).exp()
                + (3.0 * ph).sin() * 0.12 * (-t * 4.0).exp()
                + (4.0 * ph).sin() * 0.06 * (-t * 6.0).exp();
            let env = (t / 0.003).min(1.0) * (-t * 2.1).exp();
            let s = (body + hammer) * env * 0.75;
            l[i] = s;
            r[i] = s * 0.99;
        }
        AudioBuffer::from_stereo(sr, l, r, None)
    }

    fn trigger_voice(
        &mut self,
        note: u8,
        velocity: u8,
        root_key: f32,
        slice_mode: bool,
        start_trim_pct: f32,
    ) {
        let active_buf = self.buffer.as_ref().unwrap_or(&self.default_pluck);
        let total_frames = active_buf.len_frames();
        if total_frames < 4 {
            return;
        }

        let sr_ratio = active_buf.sample_rate as f64 / self.sample_rate as f64;
        let (start_frame, end_frame, rate) = if slice_mode && !self.slices.is_empty() {
            // Map MIDI 36 (C2) upward to transient slices!
            let slice_idx = (note.saturating_sub(36) as usize) % self.slices.len();
            let s0 = self.slices[slice_idx].min(total_frames - 2);
            let s1 = self
                .slices
                .get(slice_idx + 1)
                .copied()
                .unwrap_or(total_frames)
                .min(total_frames);
            (s0 as f64, s1.max(s0 + 1), sr_ratio)
        } else {
            let s0 = ((start_trim_pct * 0.01) as f64 * total_frames as f64)
                .clamp(0.0, (total_frames - 2) as f64);
            let semi = note as f32 - root_key;
            let pitch_ratio = 2.0_f64.powf(semi as f64 / 12.0) * sr_ratio;
            (s0, total_frames, pitch_ratio)
        };

        let slot = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.env
                            .partial_cmp(&b.env)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0)
            });

        self.voices[slot] = SamplerVoice {
            active: true,
            releasing: false,
            note,
            velocity: (velocity as f32 / 127.0).clamp(0.05, 1.0),
            pos_frames: start_frame,
            end_frame,
            rate,
            env: 0.0,
            ic1_l: 0.0,
            ic2_l: 0.0,
            ic1_r: 0.0,
            ic2_r: 0.0,
        };
    }

    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        midi_events: &[MidiEvent],
    ) {
        let root_key = params.first().copied().unwrap_or(60.0);
        let slice_mode = params.get(1).copied().unwrap_or(0.0) >= 0.5;
        let start_trim = params.get(2).copied().unwrap_or(0.0);
        let cutoff = params.get(3).copied().unwrap_or(14000.0);
        let res = params.get(4).copied().unwrap_or(0.18);
        let attack_ms = params.get(5).copied().unwrap_or(2.0);
        let release_ms = params.get(6).copied().unwrap_or(240.0);
        let out_gain = 10.0_f32.powf(params.get(7).copied().unwrap_or(0.0) / 20.0);

        let atk_step = 1.0 / (attack_ms * 0.001 * self.sample_rate).max(1.0);
        let rel_coeff = (-1.0 / (release_ms * 0.001 * self.sample_rate).max(1.0)).exp();

        let f = cutoff.clamp(50.0, self.sample_rate * 0.46);
        let g = (PI * f / self.sample_rate).tan();
        let k = 2.0 - 2.0 * res.clamp(0.0, 0.92);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let frames = left.len().min(right.len());
        let mut ev_idx = 0;

        for frame in 0..frames {
            while ev_idx < midi_events.len() && midi_events[ev_idx].sample_offset <= frame {
                let ev = midi_events[ev_idx];
                if ev.is_note_on && ev.velocity > 0 {
                    self.trigger_voice(ev.note, ev.velocity, root_key, slice_mode, start_trim);
                } else {
                    for v in &mut self.voices {
                        if v.active && v.note == ev.note {
                            v.releasing = true;
                        }
                    }
                }
                ev_idx += 1;
            }

            let active_buf = self.buffer.as_ref().unwrap_or(&self.default_pluck);
            let mut sum_l = 0.0_f32;
            let mut sum_r = 0.0_f32;

            for v in &mut self.voices {
                if !v.active {
                    continue;
                }
                if v.pos_frames >= v.end_frame as f64 {
                    v.active = false;
                    continue;
                }

                if v.releasing {
                    v.env *= rel_coeff;
                    if v.env < 0.0003 {
                        v.active = false;
                        continue;
                    }
                } else if v.env < 1.0 {
                    v.env = (v.env + atk_step).min(1.0);
                }

                let (raw_l, raw_r) = active_buf.sample_interpolated(v.pos_frames);
                v.pos_frames += v.rate;

                // Cytomic ZDF SVF per voice
                let v3_l = raw_l - v.ic2_l;
                let v1_l = a1 * v.ic1_l + a2 * v3_l;
                let flt_l = v.ic2_l + a2 * v.ic1_l + a3 * v3_l;
                v.ic1_l = 2.0 * v1_l - v.ic1_l;
                v.ic2_l = 2.0 * flt_l - v.ic2_l;

                let v3_r = raw_r - v.ic2_r;
                let v1_r = a1 * v.ic1_r + a2 * v3_r;
                let flt_r = v.ic2_r + a2 * v.ic1_r + a3 * v3_r;
                v.ic1_r = 2.0 * v1_r - v.ic1_r;
                v.ic2_r = 2.0 * flt_r - v.ic2_r;

                let gain = v.env * v.velocity * out_gain * 0.75;
                sum_l += flt_l * gain;
                sum_r += flt_r * gain;
            }

            left[frame] += sum_l;
            right[frame] += sum_r;
        }
    }
}
