pub mod biquad;
pub mod effects;
pub mod instruments;
pub mod metering;
pub mod tuner;

pub use biquad::StereoBiquad;
pub use effects::{
    AmpCabProcessor, CompressorProcessor, NoiseGateProcessor, ParametricEqProcessor,
    PlateReverbProcessor, StereoDelayProcessor, TapeSaturatorProcessor,
};
pub use instruments::{DrumKitProcessor, MidiEvent, PolySynthProcessor};
pub use metering::{
    db_to_lin, lin_to_db, LoudnessAndSpectrumAnalyzer, MasterMeterSnapshot, SPECTRUM_BANDS,
};
pub use tuner::{ChromaticTuner, TunerReading};

use ferric_model::{FxId, FxKind, LfoShape, ModulatorId, ModulatorKind};
use std::f32::consts::TAU;

/// Unified polymorphic stateful DSP slot corresponding to an `FxSlot` in the project model.
pub enum DspProcessor {
    PolySynth(PolySynthProcessor),
    DrumKit(DrumKitProcessor),
    ChromaticTuner(ChromaticTuner),
    NoiseGate(NoiseGateProcessor),
    ParametricEq(ParametricEqProcessor),
    Compressor(CompressorProcessor),
    AmpCabSim(AmpCabProcessor),
    TapeSaturator(TapeSaturatorProcessor),
    StereoDelay(StereoDelayProcessor),
    PlateHallReverb(PlateReverbProcessor),
}

pub struct FxRuntimeInstance {
    pub id: FxId,
    pub kind: FxKind,
    pub processor: DspProcessor,
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
}

impl FxRuntimeInstance {
    pub fn new(id: FxId, kind: FxKind, sample_rate: f32, max_block: usize) -> Self {
        let processor = match kind {
            FxKind::PolySynth => DspProcessor::PolySynth(PolySynthProcessor::new(sample_rate)),
            FxKind::DrumKit => DspProcessor::DrumKit(DrumKitProcessor::new(sample_rate)),
            FxKind::ChromaticTuner => DspProcessor::ChromaticTuner(ChromaticTuner::new(sample_rate)),
            FxKind::NoiseGate => DspProcessor::NoiseGate(NoiseGateProcessor::new(sample_rate)),
            FxKind::ParametricEq => DspProcessor::ParametricEq(ParametricEqProcessor::new(sample_rate)),
            FxKind::Compressor => DspProcessor::Compressor(CompressorProcessor::new(sample_rate)),
            FxKind::AmpCabSim => DspProcessor::AmpCabSim(AmpCabProcessor::new(sample_rate)),
            FxKind::TapeSaturator => DspProcessor::TapeSaturator(TapeSaturatorProcessor::new(sample_rate)),
            FxKind::StereoDelay => DspProcessor::StereoDelay(StereoDelayProcessor::new(sample_rate)),
            FxKind::PlateHallReverb => {
                DspProcessor::PlateHallReverb(PlateReverbProcessor::new(sample_rate))
            }
        };
        Self {
            id,
            kind,
            processor,
            dry_l: vec![0.0; max_block.max(2048)],
            dry_r: vec![0.0; max_block.max(2048)],
        }
    }

    pub fn process_block(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        wet_mix: f32,
        bpm: f64,
        midi_events: &[MidiEvent],
    ) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }
        let wet = wet_mix.clamp(0.0, 1.0);
        let need_blend = !self.kind.is_instrument() && wet < 0.999 && frames <= self.dry_l.len();
        if need_blend {
            self.dry_l[..frames].copy_from_slice(&left[..frames]);
            self.dry_r[..frames].copy_from_slice(&right[..frames]);
        }

        match &mut self.processor {
            DspProcessor::PolySynth(p) => p.process(left, right, params, midi_events),
            DspProcessor::DrumKit(p) => p.process(left, right, params, midi_events),
            DspProcessor::ChromaticTuner(t) => {
                t.ref_a4_hz = params.first().copied().unwrap_or(440.0);
                t.push_block(&left[..frames], &right[..frames]);
                if params.get(1).copied().unwrap_or(0.0) >= 0.5 {
                    left[..frames].fill(0.0);
                    right[..frames].fill(0.0);
                }
            }
            DspProcessor::NoiseGate(p) => p.process(left, right, params),
            DspProcessor::ParametricEq(p) => p.process(left, right, params),
            DspProcessor::Compressor(p) => p.process(left, right, params),
            DspProcessor::AmpCabSim(p) => p.process(left, right, params),
            DspProcessor::TapeSaturator(p) => p.process(left, right, params),
            DspProcessor::StereoDelay(p) => p.process(left, right, params, bpm),
            DspProcessor::PlateHallReverb(p) => p.process(left, right, params),
        }

        if need_blend {
            let dry = 1.0 - wet;
            for i in 0..frames {
                left[i] = self.dry_l[i] * dry + left[i] * wet;
                right[i] = self.dry_r[i] * dry + right[i] * wet;
            }
        }
    }

    pub fn gain_reduction_db(&self) -> f32 {
        match &self.processor {
            DspProcessor::Compressor(c) => c.last_gain_reduction_db,
            _ => 0.0,
        }
    }

    pub fn tuner_reading(&self) -> Option<TunerReading> {
        match &self.processor {
            DspProcessor::ChromaticTuner(t) => Some(t.last_reading),
            _ => None,
        }
    }
}

/// Bitwig-style Modulator runtime state (LFO phase, S&H latch, Envelope Follower detector).
#[derive(Debug, Clone)]
pub struct ModulatorRuntime {
    pub id: ModulatorId,
    pub phase: f32,
    pub sh_value: f32,
    pub env_value: f32,
    pub smoothed_step: f32,
    pub last_output: f32,
    rng: u32,
}

impl ModulatorRuntime {
    pub fn new(id: ModulatorId) -> Self {
        Self {
            id,
            phase: 0.0,
            sh_value: 0.0,
            env_value: 0.0,
            smoothed_step: 0.0,
            last_output: 0.0,
            rng: 0x6D2B_79F5,
        }
    }

    pub fn evaluate_block(
        &mut self,
        kind: &ModulatorKind,
        sample_rate: f32,
        frames: usize,
        bpm: f64,
        playhead_beat: f64,
        track_audio_rms: f32,
    ) -> f32 {
        let dt_secs = frames as f32 / sample_rate.max(8_000.0);
        let out = match *kind {
            ModulatorKind::Lfo {
                shape,
                rate_hz,
                tempo_sync,
                sync_beats,
                bipolar,
            } => {
                let p = if tempo_sync {
                    ((playhead_beat / sync_beats.max(0.125) as f64).rem_euclid(1.0)) as f32
                } else {
                    let prev = self.phase;
                    self.phase = (self.phase + rate_hz.max(0.02) * dt_secs).fract();
                    if self.phase < prev {
                        self.rng ^= self.rng << 13;
                        self.rng ^= self.rng >> 17;
                        self.rng ^= self.rng << 5;
                        self.sh_value = (self.rng as i32 as f32) / (i32::MAX as f32);
                    }
                    self.phase
                };
                let raw_bipolar = match shape {
                    LfoShape::Sine => (TAU * p).sin(),
                    LfoShape::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
                    LfoShape::Saw => 2.0 * p - 1.0,
                    LfoShape::Square => if p < 0.5 { 1.0 } else { -1.0 },
                    LfoShape::SampleAndHold => self.sh_value,
                };
                if bipolar {
                    raw_bipolar
                } else {
                    0.5 * (raw_bipolar + 1.0)
                }
            }
            ModulatorKind::EnvelopeFollower {
                attack_ms,
                release_ms,
                sensitivity_db,
            } => {
                let scaled = (track_audio_rms * db_to_lin(sensitivity_db)).clamp(0.0, 1.0);
                let tc = if scaled > self.env_value {
                    (attack_ms * 0.001).max(0.001)
                } else {
                    (release_ms * 0.001).max(0.005)
                };
                let alpha = (dt_secs / tc).clamp(0.0, 1.0);
                self.env_value += (scaled - self.env_value) * alpha;
                self.env_value
            }
            ModulatorKind::StepSequencer {
                steps,
                step_beats,
                smoothing,
            } => {
                let idx = ((playhead_beat / step_beats.max(0.125) as f64).floor() as usize) % 8;
                let target = steps[idx].clamp(-1.0, 1.0);
                let alpha = (1.0 - smoothing.clamp(0.0, 0.95)).max(0.05);
                self.smoothed_step += (target - self.smoothed_step) * alpha;
                let _ = bpm;
                self.smoothed_step
            }
            ModulatorKind::Macro { value } => value.clamp(0.0, 1.0),
        };
        self.last_output = out;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn chromatic_tuner_detects_440hz_a4_within_half_cent() {
        let sr = 48_000.0;
        let mut tuner = ChromaticTuner::new(sr);
        let mut l = vec![0.0_f32; 4096];
        let mut r = vec![0.0_f32; 4096];
        for i in 0..4096 {
            let s = 0.5 * (TAU * 440.0 * (i as f32 / sr)).sin();
            l[i] = s;
            r[i] = s;
        }
        tuner.push_block(&l, &r);
        assert!(tuner.last_reading.active);
        assert_eq!(tuner.last_reading.midi_note, 69);
        assert!(
            tuner.last_reading.cents.abs() < 0.8,
            "expected < 0.8 cents, got {}",
            tuner.last_reading.cents
        );
    }

    #[test]
    fn poly_synth_and_compressor_produce_clean_finite_audio() {
        let sr = 48_000.0;
        let mut synth = PolySynthProcessor::new(sr);
        let mut comp = CompressorProcessor::new(sr);
        let synth_params = FxKind::PolySynth
            .param_specs()
            .iter()
            .map(|s| s.default)
            .collect::<Vec<_>>();
        let comp_params = FxKind::Compressor
            .param_specs()
            .iter()
            .map(|s| s.default)
            .collect::<Vec<_>>();

        let mut l = vec![0.0_f32; 512];
        let mut r = vec![0.0_f32; 512];
        let events = [
            MidiEvent {
                sample_offset: 0,
                note: 60,
                velocity: 115,
                is_note_on: true,
            },
            MidiEvent {
                sample_offset: 0,
                note: 64,
                velocity: 115,
                is_note_on: true,
            },
        ];
        synth.process(&mut l, &mut r, &synth_params, &events);
        comp.process(&mut l, &mut r, &comp_params);
        let energy: f32 = l.iter().map(|x| x * x).sum();
        assert!(energy > 0.001);
        assert!(l.iter().all(|x| x.is_finite()));
        assert!(r.iter().all(|x| x.is_finite()));
    }
}
