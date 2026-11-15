//! Lightweight additive drawbar organ with polyphonic MIDI and click-free envelopes.
use crate::{MidiEvent, MidiExpression};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    held: bool,
    note: u8,
    frequency: f32,
    phase: f32,
    level: f32,
    velocity: f32,
    age: u64,
}

pub struct OrganProcessor {
    voices: [Voice; 16],
    sample_rate: f32,
    age: u64,
}

impl OrganProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [Voice::default(); 16],
            sample_rate: sample_rate.max(8000.0),
            age: 0,
        }
    }
    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            voice.held = false;
        }
    }
    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        events: &[MidiEvent],
        expression: MidiExpression,
    ) {
        let drawbars = [
            params.first().copied().unwrap_or(100.0),
            params.get(1).copied().unwrap_or(45.0),
            params.get(2).copied().unwrap_or(22.0),
            params.get(3).copied().unwrap_or(10.0),
        ]
        .map(|v| v.clamp(0.0, 100.0) * 0.01);
        let attack =
            (params.get(4).copied().unwrap_or(8.0).clamp(1.0, 500.0) * 0.001 * self.sample_rate)
                .max(1.0);
        let release =
            (params.get(5).copied().unwrap_or(80.0).clamp(5.0, 2000.0) * 0.001 * self.sample_rate)
                .max(1.0);
        let gain = 10.0_f32.powf(params.get(6).copied().unwrap_or(-6.0).clamp(-24.0, 12.0) / 20.0)
            * expression.expression.clamp(0.0, 1.5)
            * 0.12;
        let mut event_index = 0;
        let pitch_ratio = 2.0_f32.powf(expression.pitch_bend_semitones / 12.0);
        for (frame, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            while event_index < events.len() && events[event_index].sample_offset <= frame {
                let e = events[event_index];
                if e.is_note_on && e.velocity > 0 {
                    self.age = self.age.wrapping_add(1);
                    let i = self
                        .voices
                        .iter()
                        .position(|v| v.active && v.note == e.note)
                        .or_else(|| self.voices.iter().position(|v| !v.active))
                        .unwrap_or_else(|| {
                            self.voices
                                .iter()
                                .enumerate()
                                .min_by_key(|(_, v)| v.age)
                                .map(|(i, _)| i)
                                .unwrap_or(0)
                        });
                    self.voices[i] = Voice {
                        active: true,
                        held: true,
                        note: e.note,
                        frequency: 440.0 * 2.0_f32.powf((e.note as f32 - 69.0) / 12.0),
                        phase: 0.0,
                        level: 0.0,
                        velocity: e.velocity as f32 / 127.0,
                        age: self.age,
                    };
                } else {
                    for v in &mut self.voices {
                        if v.active && v.note == e.note {
                            v.held = false;
                        }
                    }
                }
                event_index += 1;
            }
            let mut sum = 0.0;
            for v in &mut self.voices {
                if !v.active {
                    continue;
                }
                if v.held {
                    v.level = (v.level + 1.0 / attack).min(1.0);
                } else {
                    v.level = (v.level - 1.0 / release).max(0.0);
                    if v.level == 0.0 {
                        v.active = false;
                        continue;
                    }
                }
                let freq = v.frequency * pitch_ratio;
                let mut tone = 0.0;
                for (i, weight) in drawbars.iter().enumerate() {
                    let harmonic = (i + 1) as f32;
                    if freq * harmonic < self.sample_rate * 0.45 {
                        tone += (TAU * v.phase * harmonic).sin() * weight;
                    }
                }
                v.phase = (v.phase + freq / self.sample_rate).fract();
                sum += tone * v.level * v.velocity * gain;
            }
            *l += sum;
            *r += sum;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn organ_obeys_midi_offset_and_releases() {
        let mut organ = OrganProcessor::new(48000.0);
        let mut l = vec![0.0; 2048];
        let mut r = l.clone();
        organ.process(
            &mut l,
            &mut r,
            &[],
            &[MidiEvent {
                sample_offset: 64,
                note: 60,
                velocity: 100,
                is_note_on: true,
            }],
            MidiExpression::default(),
        );
        assert!(l[..64].iter().all(|x| *x == 0.0));
        assert!(l[64..].iter().any(|x| x.abs() > 0.001));
        assert!(l.iter().all(|x| x.is_finite()));
        assert_eq!(l, r);
        organ.all_notes_off();
        for _ in 0..100 {
            l.fill(0.0);
            r.fill(0.0);
            organ.process(&mut l, &mut r, &[], &[], MidiExpression::default());
        }
        assert!(l.iter().all(|x| x.abs() < 1e-7));
    }
}
