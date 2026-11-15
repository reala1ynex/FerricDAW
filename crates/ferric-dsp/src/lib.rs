pub mod biquad;
pub mod convolution_ott;
pub mod effects;
pub mod electric_piano;
pub mod instruments;
pub mod organ;
pub mod metering;
pub mod sampler;
pub mod stem_splitter;
pub mod tempo_arp;
pub mod tuner;
pub mod warp_pitch;
pub mod wavetable;

pub use biquad::StereoBiquad;
pub use convolution_ott::{ConvolutionIrProcessor, MultibandOttProcessor};
pub use effects::{
    AmpCabProcessor, CompressorProcessor, NoiseGateProcessor, ParametricEqProcessor,
    PlateReverbProcessor, StereoDelayProcessor, TapeSaturatorProcessor,
};
pub use instruments::{
    DrumKitProcessor, MidiEvent, MonoBassProcessor, PolySynthProcessor, PrismSynthProcessor,
};
pub use metering::{
    db_to_lin, lin_to_db, master_normalize_and_limit_true_peak, measure_integrated_lufs,
    measure_true_peak_dbtp, LoudnessAndSpectrumAnalyzer, MasterMeterSnapshot, TpdfDither,
    SPECTRUM_BANDS, VECTORSCOPE_POINTS,
};
pub use sampler::SamplePlayerProcessor;
pub use electric_piano::ElectricPianoProcessor;
pub use organ::OrganProcessor;
pub use stem_splitter::{split_audio_to_4_stems, FourStemSplit};
pub use tempo_arp::{ArpRuntime, MidiExpression, OnsetBpmDetector};
pub use tuner::{ChromaticTuner, TunerReading};
pub use warp_pitch::{
    analyze_audio_pitch_blobs, sample_wsola_decoupled, DualGrainPitchShifter, VocalPitchProcessor,
    VocalPitchTelemetry,
};
pub use wavetable::{WavetableSynthProcessor, WT_FRAMES, WT_SAMPLES};

use ferric_model::{AudioBuffer, FxId, FxKind, LfoShape, ModulatorId, ModulatorKind};
use std::f32::consts::TAU;

/// Trait implemented by external C-ABI plugin hosts (such as our `CLAP` host in `ferric-engine`)
/// so third-party plugins integrate cleanly into the zero-alloc real-time FX chain & PDC!
pub trait ExternalPluginAdapter: Send {
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        bpm: f64,
        midi_events: &[MidiEvent],
    );
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self) {}
}

struct PassthroughExternalPlugin;
impl ExternalPluginAdapter for PassthroughExternalPlugin {
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        _bpm: f64,
        _midi_events: &[MidiEvent],
    ) {
        let trim_db = params.get(4).copied().unwrap_or(0.0);
        let g = db_to_lin(trim_db);
        for s in left.iter_mut() {
            *s *= g;
        }
        for s in right.iter_mut() {
            *s *= g;
        }
    }
}

pub enum DspProcessor {
    PolySynth(PolySynthProcessor),
    MonoBass(MonoBassProcessor),
    PrismSynth(PrismSynthProcessor),
    WavetableSynth(WavetableSynthProcessor),
    DrumKit(DrumKitProcessor),
    SamplePlayer(SamplePlayerProcessor),
    DrawbarOrgan(OrganProcessor),
    ElectricPiano(ElectricPianoProcessor),
    ChromaticTuner(ChromaticTuner),
    VocalPitch(VocalPitchProcessor),
    NoiseGate(NoiseGateProcessor),
    ParametricEq(ParametricEqProcessor),
    Compressor(CompressorProcessor),
    MultibandOtt(MultibandOttProcessor),
    AmpCabSim(AmpCabProcessor),
    TapeSaturator(TapeSaturatorProcessor),
    StereoDelay(StereoDelayProcessor),
    PlateHallReverb(PlateReverbProcessor),
    ConvolutionIr(ConvolutionIrProcessor),
    ExternalClap(Box<dyn ExternalPluginAdapter>),
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
            FxKind::MonoBass => DspProcessor::MonoBass(MonoBassProcessor::new(sample_rate)),
            FxKind::PrismSynth | FxKind::HostDna => {
                DspProcessor::PrismSynth(PrismSynthProcessor::new(sample_rate))
            }
            FxKind::WavetableSynth => {
                DspProcessor::WavetableSynth(WavetableSynthProcessor::new(sample_rate))
            }
            FxKind::DrumKit => DspProcessor::DrumKit(DrumKitProcessor::new(sample_rate)),
            FxKind::SamplePlayer => {
                DspProcessor::SamplePlayer(SamplePlayerProcessor::new(sample_rate))
            }
            FxKind::DrawbarOrgan => DspProcessor::DrawbarOrgan(OrganProcessor::new(sample_rate)),
            FxKind::ElectricPiano => DspProcessor::ElectricPiano(ElectricPianoProcessor::new(sample_rate)),
            FxKind::ChromaticTuner => {
                DspProcessor::ChromaticTuner(ChromaticTuner::new(sample_rate))
            }
            FxKind::VocalPitch => DspProcessor::VocalPitch(VocalPitchProcessor::new(sample_rate)),
            FxKind::NoiseGate => DspProcessor::NoiseGate(NoiseGateProcessor::new(sample_rate)),
            FxKind::ParametricEq => {
                DspProcessor::ParametricEq(ParametricEqProcessor::new(sample_rate))
            }
            FxKind::Compressor => DspProcessor::Compressor(CompressorProcessor::new(sample_rate)),
            FxKind::MultibandOtt => {
                DspProcessor::MultibandOtt(MultibandOttProcessor::new(sample_rate))
            }
            FxKind::AmpCabSim => DspProcessor::AmpCabSim(AmpCabProcessor::new(sample_rate)),
            FxKind::TapeSaturator => {
                DspProcessor::TapeSaturator(TapeSaturatorProcessor::new(sample_rate))
            }
            FxKind::StereoDelay => {
                DspProcessor::StereoDelay(StereoDelayProcessor::new(sample_rate))
            }
            FxKind::PlateHallReverb => {
                DspProcessor::PlateHallReverb(PlateReverbProcessor::new(sample_rate))
            }
            FxKind::ConvolutionIr => {
                DspProcessor::ConvolutionIr(ConvolutionIrProcessor::new(sample_rate))
            }
            FxKind::ExternalClap => {
                DspProcessor::ExternalClap(Box::new(PassthroughExternalPlugin))
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

    pub fn new_external_clap(
        id: FxId,
        adapter: Box<dyn ExternalPluginAdapter>,
        max_block: usize,
    ) -> Self {
        Self {
            id,
            kind: FxKind::ExternalClap,
            processor: DspProcessor::ExternalClap(adapter),
            dry_l: vec![0.0; max_block.max(2048)],
            dry_r: vec![0.0; max_block.max(2048)],
        }
    }

    pub fn set_sampler_data(&mut self, buffer: Option<AudioBuffer>, slices: Vec<usize>) {
        match &mut self.processor {
            DspProcessor::SamplePlayer(sp) => sp.set_sample(buffer, slices),
            DspProcessor::WavetableSynth(wt) => {
                if let Some(ref b) = buffer {
                    wt.load_custom_wavetable(b);
                }
            }
            DspProcessor::ConvolutionIr(conv) => {
                conv.set_custom_ir(buffer);
            }
            _ => {}
        }
    }

    /// Exact processing latency in samples for Automatic Plugin Delay Compensation (PDC)!
    pub fn latency_samples(&self) -> usize {
        match &self.processor {
            DspProcessor::VocalPitch(_) => VocalPitchProcessor::latency_samples(),
            DspProcessor::ExternalClap(ext) => ext.latency_samples(),
            _ => 0,
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
        self.process_block_full(
            left,
            right,
            params,
            wet_mix,
            bpm,
            midi_events,
            MidiExpression::default(),
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_block_with_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        wet_mix: f32,
        bpm: f64,
        midi_events: &[MidiEvent],
        sidechain_key: Option<(&[f32], &[f32])>,
    ) {
        self.process_block_full(
            left,
            right,
            params,
            wet_mix,
            bpm,
            midi_events,
            MidiExpression::default(),
            sidechain_key,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_block_full(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        wet_mix: f32,
        bpm: f64,
        midi_events: &[MidiEvent],
        midi_expr: MidiExpression,
        sidechain_key: Option<(&[f32], &[f32])>,
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
            DspProcessor::PolySynth(p) => {
                p.process_with_expression(left, right, params, midi_events, midi_expr)
            }
            DspProcessor::MonoBass(p) => {
                p.process_with_expression(left, right, params, midi_events, midi_expr)
            }
            DspProcessor::PrismSynth(p) => {
                p.process_with_expression(left, right, params, midi_events, midi_expr)
            }
            DspProcessor::WavetableSynth(p) => {
                p.process_with_expression(left, right, params, midi_events, midi_expr)
            }
            DspProcessor::DrumKit(p) => p.process(left, right, params, midi_events),
            DspProcessor::SamplePlayer(p) => {
                p.process_with_expression(left, right, params, midi_events, midi_expr)
            }
            DspProcessor::DrawbarOrgan(p) => p.process(left, right, params, midi_events, midi_expr),
            DspProcessor::ElectricPiano(p) => p.process(left, right, params, midi_events, midi_expr),
            DspProcessor::ChromaticTuner(t) => {
                t.ref_a4_hz = params.first().copied().unwrap_or(440.0);
                t.push_block(&left[..frames], &right[..frames]);
                if params.get(1).copied().unwrap_or(0.0) >= 0.5 {
                    left[..frames].fill(0.0);
                    right[..frames].fill(0.0);
                }
            }
            DspProcessor::VocalPitch(p) => p.process(left, right, params),
            DspProcessor::NoiseGate(p) => {
                p.process_with_sidechain(left, right, params, sidechain_key)
            }
            DspProcessor::ParametricEq(p) => p.process(left, right, params),
            DspProcessor::Compressor(p) => {
                p.process_with_sidechain(left, right, params, sidechain_key)
            }
            DspProcessor::MultibandOtt(p) => p.process(left, right, params),
            DspProcessor::AmpCabSim(p) => p.process(left, right, params),
            DspProcessor::TapeSaturator(p) => p.process(left, right, params),
            DspProcessor::StereoDelay(p) => p.process(left, right, params, bpm),
            DspProcessor::PlateHallReverb(p) => p.process(left, right, params),
            DspProcessor::ConvolutionIr(p) => p.process(left, right, params),
            DspProcessor::ExternalClap(ext) => ext.process(left, right, params, bpm, midi_events),
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

    pub fn vocal_pitch_telemetry(&self) -> Option<VocalPitchTelemetry> {
        match &self.processor {
            DspProcessor::VocalPitch(v) => Some(v.telemetry),
            _ => None,
        }
    }
}

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
                    LfoShape::Square => {
                        if p < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
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
    fn new_instruments_render_through_runtime_with_sample_accurate_midi() {
        for kind in [FxKind::DrawbarOrgan, FxKind::ElectricPiano] {
            let slot = ferric_model::FxSlot::new(kind);
            let params: Vec<f32> = kind.param_specs().iter().map(|spec| spec.default).collect();
            let mut runtime = FxRuntimeInstance::new(slot.id, kind, 48_000.0, 1024);
            let mut left = vec![0.0; 1024];
            let mut right = left.clone();
            runtime.process_block(&mut left, &mut right, &params, 1.0, 120.0, &[
                MidiEvent { sample_offset: 37, note: 69, velocity: 110, is_note_on: true },
            ]);
            assert!(left[..37].iter().all(|sample| *sample == 0.0), "{kind:?}");
            assert!(left[37..].iter().any(|sample| sample.abs() > 0.001), "{kind:?}");
            assert!(left.iter().chain(&right).all(|sample| sample.is_finite()), "{kind:?}");
            assert_eq!(left, right, "{kind:?}");
        }
    }

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
        assert!(tuner.last_reading.cents.abs() < 0.8);
    }

    #[test]
    fn vocal_pitch_corrector_and_sampler_produce_clean_output() {
        let sr = 48_000.0;
        let mut vp = VocalPitchProcessor::new(sr);
        // Feed a slightly sharp A4 (448 Hz) into A Minor auto-pitch
        let mut l: Vec<f32> = (0..4096)
            .map(|i| 0.4 * (TAU * 448.0 * (i as f32 / sr)).sin())
            .collect();
        let mut r = l.clone();
        vp.process(&mut l, &mut r, &[9.0, 2.0, 0.0, 0.0, 0.0, 20.0]);
        assert!(vp.telemetry.active);
        assert!(l.iter().all(|x| x.is_finite()));

        // Test Sampler chromatic voice playback
        let mut sampler = SamplePlayerProcessor::new(sr);
        let mut sl = vec![0.0_f32; 512];
        let mut sr_buf = vec![0.0_f32; 512];
        let params: Vec<f32> = FxKind::SamplePlayer
            .param_specs()
            .iter()
            .map(|s| s.default)
            .collect();
        sampler.process(
            &mut sl,
            &mut sr_buf,
            &params,
            &[MidiEvent {
                sample_offset: 0,
                note: 60,
                velocity: 110,
                is_note_on: true,
            }],
        );
        let energy: f32 = sl.iter().map(|x| x * x).sum();
        assert!(energy > 0.001);
    }

    #[test]
    fn onset_bpm_detector_and_arpeggiator_work_accurately() {
        let sr = 48_000.0;
        let mut det = OnsetBpmDetector::new(sr);
        // Synthesize 4.5 seconds of crisp 120.0 BPM quarter-note kicks (every 0.5s = 24,000 samples)
        let total = (4.5 * sr) as usize;
        let mut l = vec![0.0_f32; total];
        for beat in 0..9 {
            let s0 = beat * 24_000;
            for k in 0..1200 {
                if s0 + k < total {
                    let env = (-(k as f32) / 220.0).exp();
                    l[s0 + k] = 0.8 * env * (TAU * 90.0 * (k as f32 / sr)).sin();
                }
            }
        }
        det.push_block(&l, &l);
        let bpm = det.detected_bpm.expect("should detect 120 BPM");
        assert!(
            (bpm - 120.0).abs() <= 1.5,
            "expected ~120 BPM, got {}",
            bpm
        );

        // Test Track Arpeggiator expanding a C-major chord into stepped notes
        let mut arp = ArpRuntime::new();
        let cfg = ferric_model::TrackArpeggiator {
            enabled: true,
            mode: ferric_model::ArpMode::Up,
            rate_beats: 0.25,
            octaves: 2,
            gate: 0.75,
        };
        let mut events = vec![
            MidiEvent {
                sample_offset: 0,
                note: 60,
                velocity: 100,
                is_note_on: true,
            },
            MidiEvent {
                sample_offset: 0,
                note: 64,
                velocity: 100,
                is_note_on: true,
            },
        ];
        arp.process_midi_block(&cfg, 24_000, sr, 120.0, true, 0.0, &mut events);
        let note_ons: Vec<u8> = events
            .iter()
            .filter(|e| e.is_note_on)
            .map(|e| e.note)
            .collect();
        assert!(note_ons.len() >= 4);
        assert_eq!(note_ons[0], 60);
        assert_eq!(note_ons[1], 64);
    }

    #[test]
    fn milestone8_wavetable_convolution_ott_and_melodyne_blobs_verified() {
        let sr = 48_000.0;

        // 1. Test FerricWave Wavetable + Shimmer Granular Cloud + Custom WAV Import
        let mut wt = WavetableSynthProcessor::new(sr);
        let custom_samples: Vec<f32> = (0..4096)
            .map(|i| (TAU * (i as f32 / 128.0)).sin() * 0.7 + (TAU * 3.0 * (i as f32 / 128.0)).sin() * 0.25)
            .collect();
        let custom_buf =
            AudioBuffer::from_stereo(48_000, custom_samples.clone(), custom_samples, None);
        wt.load_custom_wavetable(&custom_buf);

        let mut wt_params: Vec<f32> = FxKind::WavetableSynth
            .param_specs()
            .iter()
            .map(|s| s.default)
            .collect();
        wt_params[0] = 3.0; // Bank 3 = Custom WAV!
        wt_params[2] = 6.0; // 6x Stereo Unison
        wt_params[4] = 45.0; // 45% Shimmer Granular Cloud
        let mut wl = vec![0.0_f32; 4096];
        let mut wr = vec![0.0_f32; 4096];
        wt.process_with_expression(
            &mut wl,
            &mut wr,
            &wt_params,
            &[MidiEvent {
                sample_offset: 0,
                note: 60,
                velocity: 112,
                is_note_on: true,
            }],
            MidiExpression::default(),
        );
        let wt_energy: f32 = wl.iter().zip(&wr).map(|(a, b)| a * a + b * b).sum();
        assert!(wt_energy > 0.05, "FerricWave should produce clean stereo audio");
        assert!(wl.iter().chain(&wr).all(|v| v.is_finite()));

        // 2. Test Partitioned FFT Convolution IR Engine (Direct FIR head + FFT tail)
        let mut conv = ConvolutionIrProcessor::new(sr);
        let mut ir_samples = vec![0.0_f32; 1024];
        ir_samples[10] = 1.0; // Direct FIR tap (< 64)
        ir_samples[400] = 0.8; // Partitioned FFT tail tap (> 256)
        let ir_buf = AudioBuffer::from_stereo(48_000, ir_samples.clone(), ir_samples, None);
        conv.set_custom_ir(Some(ir_buf));

        let conv_params = [4.0, 0.0, 100.0, 20.0, 18000.0, 0.0]; // Custom IR, 0ms pre-delay, flat EQ
        let mut cl = vec![0.0_f32; 1024];
        let mut cr = vec![0.0_f32; 1024];
        cl[0] = 1.0;
        cr[0] = 1.0;
        conv.process(&mut cl, &mut cr, &conv_params);
        let conv_energy: f32 = cl.iter().map(|v| v * v).sum();
        assert!(conv_energy > 0.001, "Convolution IR must output impulse energy");
        assert!(cl[10].abs() > 0.02, "Direct zero-latency FIR tap at sample 10 must fire");
        assert!(cl.iter().chain(&cr).all(|v| v.is_finite()));

        // 3. Test 3-Band Linkwitz-Riley Upward/Downward ("OTT") Multiband Compressor
        let mut ott = MultibandOttProcessor::new(sr);
        let mut ott_params: Vec<f32> = FxKind::MultibandOtt
            .param_specs()
            .iter()
            .map(|s| s.default)
            .collect();
        ott_params[0] = 100.0; // 100% OTT Depth
        ott_params[1] = 25.0; // Fast Time Scale
        // Feed a quiet (-48 dBFS) 440 Hz tone for 24,000 samples (0.5s) and verify upward lift
        let quiet_amp = 0.004_f32;
        let mut ql: Vec<f32> = (0..24_000)
            .map(|i| quiet_amp * (TAU * 440.0 * (i as f32 / sr)).sin())
            .collect();
        let mut qr = ql.clone();
        let in_rms = (ql[20_000..].iter().map(|x| x * x).sum::<f32>() / 4_000.0).sqrt();
        ott.process(&mut ql, &mut qr, &ott_params);
        let out_rms = (ql[20_000..].iter().map(|x| x * x).sum::<f32>() / 4_000.0).sqrt();
        assert!(
            out_rms > in_rms * 1.5,
            "OTT upward compression should lift quiet signal (in_rms={}, out_rms={})",
            in_rms,
            out_rms
        );
        // Also verify hot (+0 dBFS) signal gets downward-compressed cleanly
        let mut hl: Vec<f32> = (0..12_000)
            .map(|i| 0.95 * (TAU * 440.0 * (i as f32 / sr)).sin())
            .collect();
        let mut hr = hl.clone();
        let hot_in = (hl[8_000..].iter().map(|x| x * x).sum::<f32>() / 4_000.0).sqrt();
        ott.process(&mut hl, &mut hr, &ott_params);
        let hot_out = (hl[8_000..].iter().map(|x| x * x).sum::<f32>() / 4_000.0).sqrt();
        assert!(
            hot_out < hot_in * 0.75,
            "OTT downward compression should tame hot signal (hot_in={}, hot_out={})",
            hot_in,
            hot_out
        );
        assert!(ql.iter().chain(&qr).chain(&hl).all(|v| v.is_finite()));

        // 4. Test Melodyne-style Audio-to-Pitch-Blobs + 1-Click Audio-to-MIDI Conversion!
        // Synthesize a 3-note sung/lead line at 120 BPM (0.5s per beat): C4 (261.63Hz), E4 (329.63Hz), G4 (392.00Hz)
        let note_freqs = [261.6256_f32, 329.6276_f32, 391.9954_f32];
        let samples_per_beat = 24_000usize;
        let mut melody = vec![0.0_f32; samples_per_beat * 3];
        for (n_idx, &hz) in note_freqs.iter().enumerate() {
            let base = n_idx * samples_per_beat;
            // Leave a tiny 40ms breath gap at the end of each beat so segmenter cleanly splits notes
            let voiced_len = samples_per_beat - 2_400;
            for k in 0..voiced_len {
                melody[base + k] = 0.55 * (TAU * hz * (k as f32 / sr)).sin();
            }
        }
        let mel_buf = AudioBuffer::from_stereo(48_000, melody.clone(), melody, None);
        let blobs = analyze_audio_pitch_blobs(&mel_buf, 120.0, 3.0);
        assert_eq!(blobs.len(), 3, "Expected 3 detected Melodyne pitch blobs, got {}", blobs.len());

        let mut clip = ferric_model::TimelineClip::new_audio("VocalLead", 0.0, 3.0, mel_buf);
        clip.pitch_blobs = blobs;
        let extracted_midi = clip.pitch_blobs_to_midi_notes();
        let pitches: Vec<u8> = extracted_midi.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60, 64, 67], "Audio->MIDI must extract C4 (60), E4 (64), G4 (67)");
    }
}
