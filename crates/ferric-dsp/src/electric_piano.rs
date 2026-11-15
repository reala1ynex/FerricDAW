//! Polyphonic two-operator electric piano with velocity-sensitive tine decay.
use crate::{MidiEvent, MidiExpression};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    held: bool,
    note: u8,
    frequency: f32,
    phase: f32,
    amplitude: f32,
    timbre: f32,
    velocity: f32,
    age: u64,
}

pub struct ElectricPianoProcessor {
    voices: [Voice; 16],
    sample_rate: f32,
    age: u64,
}

impl ElectricPianoProcessor {
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
        let brightness = (params.first().copied().unwrap_or(45.0) * 0.01).clamp(0.0, 1.0);
        let decay = params
            .get(1)
            .copied()
            .unwrap_or(1100.0)
            .clamp(100.0, 5000.0)
            * 0.001;
        let release = params.get(2).copied().unwrap_or(300.0).clamp(10.0, 3000.0) * 0.001;
        let gain = 10.0_f32.powf(params.get(3).copied().unwrap_or(-6.0).clamp(-24.0, 12.0) / 20.0)
            * expression.expression.clamp(0.0, 1.5)
            * 0.18;
        let decay_factor = (-1.0 / (self.sample_rate * decay)).exp();
        let release_factor = (-1.0 / (self.sample_rate * release)).exp();
        let timbre_factor = (-1.0 / (self.sample_rate * 0.45)).exp();
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
                        amplitude: 1.0,
                        timbre: 1.0,
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
                let frequency = v.frequency * pitch_ratio;
                let modulation =
                    (TAU * v.phase * 2.0).sin() * brightness * v.timbre * v.velocity * 2.5;
                let tone = (TAU * v.phase + modulation).sin();
                v.phase = (v.phase + frequency / self.sample_rate).fract();
                sum += tone * v.amplitude * v.velocity * gain;
                v.amplitude *= if v.held { decay_factor } else { release_factor };
                v.timbre *= timbre_factor;
                if v.amplitude < 0.0001 {
                    v.active = false;
                }
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
    fn electric_piano_produces_finite_audio_and_releases() {
        let mut piano = ElectricPianoProcessor::new(48000.0);
        let mut l = vec![0.0; 1024];
        let mut r = l.clone();
        piano.process(
            &mut l,
            &mut r,
            &[],
            &[MidiEvent {
                sample_offset: 40,
                note: 60,
                velocity: 100,
                is_note_on: true,
            }],
            MidiExpression::default(),
        );
        assert!(l[..40].iter().all(|x| *x == 0.0));
        assert!(l[40..].iter().any(|x| x.abs() > 0.001));
        assert!(l.iter().all(|x| x.is_finite()));
        piano.all_notes_off();
        for _ in 0..200 {
            l.fill(0.0);
            r.fill(0.0);
            piano.process(&mut l, &mut r, &[], &[], MidiExpression::default());
        }
        assert!(l.iter().all(|x| x.abs() < 0.0001));
    }
}
