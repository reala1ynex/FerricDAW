use crate::biquad::StereoBiquad;
use crate::tuner::ChromaticTuner;
use ferric_model::{AudioBuffer, MusicalScale, NotePitchBlob};
use std::f32::consts::PI;
use uuid::Uuid;

const PITCH_RING_LEN: usize = 8192; // Power of 2 (~170ms @ 48kHz)
const GRAIN_LEN_SAMPLES: f32 = 1536.0; // ~32ms Hann grain for clean vocal/guitar pitch shifting

/// Zero-allocation Dual-Grain Hann-Windowed Pitch Shifter (real-time streaming).
/// Shifts pitch by `pitch_ratio` (`0.5 ..= 2.0`) with constant latency = `GRAIN_LEN_SAMPLES / 2`!
pub struct DualGrainPitchShifter {
    ring_l: [f32; PITCH_RING_LEN],
    ring_r: [f32; PITCH_RING_LEN],
    write_pos: usize,
    phasor: f32,
}

impl Default for DualGrainPitchShifter {
    fn default() -> Self {
        Self::new()
    }
}

impl DualGrainPitchShifter {
    pub fn new() -> Self {
        Self {
            ring_l: [0.0; PITCH_RING_LEN],
            ring_r: [0.0; PITCH_RING_LEN],
            write_pos: 0,
            phasor: 0.0,
        }
    }

    pub const fn latency_samples() -> usize {
        (GRAIN_LEN_SAMPLES * 0.5) as usize
    }

    #[inline]
    fn read_interp(ring: &[f32; PITCH_RING_LEN], delay_samples: f32) -> f32 {
        let mask = PITCH_RING_LEN - 1;
        let idx0 = delay_samples.floor() as usize;
        let frac = delay_samples - idx0 as f32;
        let a = ring[idx0 & mask];
        let b = ring[(idx0 + 1) & mask];
        a + (b - a) * frac
    }

    #[inline]
    pub fn process_sample(&mut self, in_l: f32, in_r: f32, pitch_ratio: f32) -> (f32, f32) {
        let mask = PITCH_RING_LEN - 1;
        self.ring_l[self.write_pos] = in_l;
        self.ring_r[self.write_pos] = in_r;

        let ratio = pitch_ratio.clamp(0.5, 2.0);
        if (ratio - 1.0).abs() < 0.0005 {
            self.write_pos = (self.write_pos + 1) & mask;
            return (in_l, in_r);
        }

        // Advance grain phasor cleanly
        let step = (1.0 - ratio) / GRAIN_LEN_SAMPLES;
        self.phasor = (self.phasor + step).rem_euclid(1.0);
        let ph_a = self.phasor;
        let ph_b = (self.phasor + 0.5).rem_euclid(1.0);

        // Hann complementary crossfade windows: sin^2(pi*ph_a) + sin^2(pi*ph_b) == 1.0!
        let win_a = (PI * ph_a).sin();
        let win_a = win_a * win_a;
        let win_b = (PI * ph_b).sin();
        let win_b = win_b * win_b;

        let del_a = ph_a * GRAIN_LEN_SAMPLES + 4.0;
        let del_b = ph_b * GRAIN_LEN_SAMPLES + 4.0;

        let pos_a = (self.write_pos as f32 + PITCH_RING_LEN as f32 - del_a)
            .rem_euclid(PITCH_RING_LEN as f32);
        let pos_b = (self.write_pos as f32 + PITCH_RING_LEN as f32 - del_b)
            .rem_euclid(PITCH_RING_LEN as f32);

        let out_l = Self::read_interp(&self.ring_l, pos_a) * win_a
            + Self::read_interp(&self.ring_l, pos_b) * win_b;
        let out_r = Self::read_interp(&self.ring_r, pos_a) * win_a
            + Self::read_interp(&self.ring_r, pos_b) * win_b;

        self.write_pos = (self.write_pos + 1) & mask;
        (out_l, out_r)
    }
}

/// Evaluates an `AudioBuffer` at decoupled `(source_sec, pitch_semitones)` using Dual-Window WSOLA
/// grain crossfading directly against the random-access clip buffer — zero chipmunk speed change!
#[inline]
pub fn sample_wsola_decoupled(
    buffer: &AudioBuffer,
    source_sec: f64,
    pitch_semitones: f32,
) -> (f32, f32) {
    let sr = buffer.sample_rate.max(8_000) as f64;
    let base_frame = source_sec * sr;
    if pitch_semitones.abs() < 0.01 {
        return buffer.sample_interpolated(base_frame);
    }

    let ratio = 2.0_f64.powf(pitch_semitones as f64 / 12.0);
    let grain_frames = 1536.0_f64; // ~32ms grain
    let ph_a = ((base_frame * (ratio - 1.0).abs()) / grain_frames).rem_euclid(1.0);
    let ph_b = (ph_a + 0.5).rem_euclid(1.0);

    let sign = if ratio >= 1.0 { 1.0 } else { -1.0 };
    let offset_a = (ph_a - 0.5) * grain_frames * sign;
    let offset_b = (ph_b - 0.5) * grain_frames * sign;

    let wa = (std::f64::consts::PI * ph_a).sin();
    let wa = (wa * wa) as f32;
    let wb = (std::f64::consts::PI * ph_b).sin();
    let wb = (wb * wb) as f32;

    let (la, ra) = buffer.sample_interpolated((base_frame + offset_a).max(0.0));
    let (lb, rb) = buffer.sample_interpolated((base_frame + offset_b).max(0.0));
    (la * wa + lb * wb, ra * wa + rb * wb)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VocalPitchTelemetry {
    pub active: bool,
    pub input_midi: f32,
    pub target_midi: f32,
    pub correction_cents: f32,
}

/// Scale-Aware Real-Time Vocal Auto-Pitch & Transpose Processor (ReaTune / Auto-Tune style).
pub struct VocalPitchProcessor {
    detector: ChromaticTuner,
    shifter: DualGrainPitchShifter,
    warmth_eq: StereoBiquad,
    smoothed_ratio: f32,
    sustained_frames: usize,
    last_target_note: i32,
    sample_rate: f32,
    pub telemetry: VocalPitchTelemetry,
}

impl VocalPitchProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        Self {
            detector: ChromaticTuner::new(sr),
            shifter: DualGrainPitchShifter::new(),
            warmth_eq: StereoBiquad::default(),
            smoothed_ratio: 1.0,
            sustained_frames: 0,
            last_target_note: 69,
            sample_rate: sr,
            telemetry: VocalPitchTelemetry::default(),
        }
    }

    pub const fn latency_samples() -> usize {
        DualGrainPitchShifter::latency_samples()
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        let root_pc = params.first().copied().unwrap_or(9.0).round().clamp(0.0, 11.0) as u8;
        let scale_idx = params.get(1).copied().unwrap_or(2.0).round().clamp(0.0, 7.0) as usize;
        let retune_ms = params.get(2).copied().unwrap_or(22.0).clamp(0.0, 200.0);
        let transpose_st = params.get(3).copied().unwrap_or(0.0).clamp(-12.0, 12.0);
        let humanize = (params.get(4).copied().unwrap_or(25.0) * 0.01).clamp(0.0, 1.0);
        let warmth = (params.get(5).copied().unwrap_or(30.0) * 0.01).clamp(0.0, 1.0);

        let scale = MusicalScale::ALL
            .get(scale_idx)
            .copied()
            .unwrap_or(MusicalScale::NaturalMinor);

        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }

        // Feed audio into sub-cent YIN pitch detector
        self.detector.push_block(&left[..frames], &right[..frames]);
        let reading = self.detector.last_reading;

        let desired_ratio = if reading.active && reading.confidence > 0.55 {
            let exact_midi = 69.0 + 12.0 * (reading.freq_hz / 440.0).log2();
            let snapped_midi = scale.snap_continuous_midi(root_pc, exact_midi);
            let snapped_i = snapped_midi.round() as i32;
            if snapped_i == self.last_target_note {
                self.sustained_frames = self.sustained_frames.saturating_add(frames);
            } else {
                self.last_target_note = snapped_i;
                self.sustained_frames = 0;
            }

            // Humanize preserves natural vibrato on sustained vocal notes (> 140ms)
            let vibrato_relax = if self.sustained_frames as f32 > 0.14 * self.sample_rate {
                humanize * 0.65
            } else {
                0.0
            };
            let effective_target =
                snapped_midi * (1.0 - vibrato_relax) + exact_midi * vibrato_relax + transpose_st;
            let delta_semi = (effective_target - exact_midi).clamp(-12.0, 12.0);

            self.telemetry = VocalPitchTelemetry {
                active: true,
                input_midi: exact_midi,
                target_midi: snapped_midi + transpose_st,
                correction_cents: delta_semi * 100.0,
            };
            2.0_f32.powf(delta_semi / 12.0)
        } else {
            self.telemetry.active = false;
            2.0_f32.powf(transpose_st / 12.0)
        };

        let retune_alpha = if retune_ms < 0.5 {
            1.0 // Instant 0ms robotic snap!
        } else {
            (1.0 - (-1.0 / (retune_ms * 0.001 * self.sample_rate).max(1.0)).exp()).clamp(0.0002, 1.0)
        };

        self.warmth_eq
            .set_peaking(self.sample_rate, 320.0, 0.9, warmth * 2.2);

        for i in 0..frames {
            self.smoothed_ratio += (desired_ratio - self.smoothed_ratio) * retune_alpha;
            let (mut l, mut r) =
                self.shifter
                    .process_sample(left[i], right[i], self.smoothed_ratio);
            if warmth > 0.02 {
                (l, r) = self.warmth_eq.process_stereo(l, r);
                let drv = 1.0 + warmth * 0.45;
                l = (l * drv).tanh() / drv.sqrt();
                r = (r * drv).tanh() / drv.sqrt();
            }
            left[i] = l;
            right[i] = r;
        }
    }
}

/// Melodyne / ReaTune Offline Note-Blob Pitch & Rhythm Extractor!
/// Scans any monophonic vocal, guitar, bass, or synth `AudioBuffer` using 25 ms YIN windows,
/// segments contiguous pitched phrases into discrete `NotePitchBlob`s with 16-point vibrato contours,
/// and enables both per-note Melodyne pitch dragging AND 1-click **Audio-to-MIDI** conversion!
pub fn analyze_audio_pitch_blobs(
    buffer: &AudioBuffer,
    bpm: f64,
    clip_len_beats: f64,
) -> Vec<NotePitchBlob> {
    let total_frames = buffer.len_frames();
    let sr = buffer.sample_rate.max(8_000) as f32;
    if total_frames < 1024 {
        return Vec::new();
    }

    let hop = ((0.020 * sr).round() as usize).max(256); // 20 ms hop
    let win = ((0.042 * sr).round() as usize).clamp(512, 4096); // ~42 ms YIN window
    let min_tau = (sr / 1100.0).floor().max(2.0) as usize;
    let max_tau = ((sr / 48.0).ceil() as usize).min(win / 2 - 2);
    let spb = 60.0 / bpm.max(20.0);

    #[derive(Clone, Copy)]
    struct FrameEst {
        beat: f64,
        voiced: bool,
        midi: f32,
        rms: f32,
    }

    let mut frames_est = Vec::with_capacity(total_frames / hop + 1);
    let mut diff = vec![0.0_f32; max_tau + 2];
    let mut max_rms = 0.01_f32;

    let mut pos = 0usize;
    while pos + win <= total_frames {
        let sec = (pos as f64) / (sr as f64);
        let beat = sec / spb;
        if beat > clip_len_beats {
            break;
        }

        let mut energy = 0.0_f32;
        for i in 0..(win / 2) {
            let m = 0.5 * (buffer.left[pos + i] + buffer.right[pos + i]);
            energy += m * m;
        }
        let rms = (energy / (win / 2) as f32).sqrt();
        max_rms = max_rms.max(rms);

        if rms < 0.008 {
            frames_est.push(FrameEst {
                beat,
                voiced: false,
                midi: 0.0,
                rms,
            });
            pos += hop;
            continue;
        }

        diff[0] = 1.0;
        let mut run_sum = 0.0_f32;
        let half = win / 2;
        for tau in 1..=max_tau {
            let mut s = 0.0_f32;
            let mut j = 0;
            while j < half {
                let a = 0.5 * (buffer.left[pos + j] + buffer.right[pos + j]);
                let b = 0.5 * (buffer.left[pos + j + tau] + buffer.right[pos + j + tau]);
                let d = a - b;
                s += d * d;
                j += 2;
            }
            run_sum += s;
            diff[tau] = if run_sum > 1e-12 {
                s * (tau as f32) / run_sum
            } else {
                1.0
            };
        }

        let mut best_tau = None;
        let mut tau = min_tau;
        while tau < max_tau {
            if diff[tau] < 0.18 {
                while tau + 1 < max_tau && diff[tau + 1] < diff[tau] {
                    tau += 1;
                }
                best_tau = Some(tau);
                break;
            }
            tau += 1;
        }
        if best_tau.is_none() {
            let mut mv = 1.0_f32;
            let mut mi = 0usize;
            for (t, &dv) in diff.iter().enumerate().take(max_tau).skip(min_tau) {
                if dv < mv {
                    mv = dv;
                    mi = t;
                }
            }
            if mv < 0.30 {
                best_tau = Some(mi);
            }
        }

        if let Some(t0) = best_tau {
            let (y0, y1, y2) = (diff[t0 - 1], diff[t0], diff[t0 + 1]);
            let den = 2.0 * (2.0 * y1 - y2 - y0);
            let refined = if den.abs() > 1e-9 {
                t0 as f32 + (y2 - y0) / den
            } else {
                t0 as f32
            };
            let freq = sr / refined.max(1.0);
            let midi = 69.0 + 12.0 * (freq / 440.0).log2();
            if (24.0..=108.0).contains(&midi) {
                frames_est.push(FrameEst {
                    beat,
                    voiced: true,
                    midi,
                    rms,
                });
            } else {
                frames_est.push(FrameEst {
                    beat,
                    voiced: false,
                    midi: 0.0,
                    rms,
                });
            }
        } else {
            frames_est.push(FrameEst {
                beat,
                voiced: false,
                midi: 0.0,
                rms,
            });
        }

        pos += hop;
    }

    // Group contiguous voiced frames within ±0.80 semitones (and split on sharp onset jumps!) into `NotePitchBlob`s
    let hop_beats = (hop as f64 / sr as f64) / spb;
    let mut blobs = Vec::new();
    let mut seg: Vec<FrameEst> = Vec::new();

    let flush_seg = |seg: &mut Vec<FrameEst>, blobs: &mut Vec<NotePitchBlob>| {
        if seg.len() < 3 {
            seg.clear();
            return;
        }
        let start_b = seg.first().unwrap().beat;
        let end_b = seg.last().unwrap().beat + hop_beats;
        if end_b - start_b < 0.10 {
            seg.clear();
            return;
        }
        // Median MIDI pitch for vibrato-immune pitch center
        let mut pitches: Vec<f32> = seg.iter().map(|f| f.midi).collect();
        pitches.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_midi = pitches[pitches.len() / 2];
        let avg_rms = seg.iter().map(|f| f.rms).sum::<f32>() / seg.len() as f32;

        // Downsample 16-point contour for Melodyne pitch-curve rendering
        let mut contour = Vec::with_capacity(16);
        for k in 0..16 {
            let idx = (k * (seg.len() - 1)) / 15;
            contour.push((seg[idx].midi - median_midi).clamp(-2.0, 2.0));
        }

        blobs.push(NotePitchBlob {
            id: Uuid::new_v4(),
            start_beat: start_b,
            end_beat: end_b,
            detected_midi: median_midi,
            target_midi: median_midi.round(),
            fine_cents: (median_midi - median_midi.round()) * 100.0, // Starts at 0 net shift until user snaps or drags!
            drift_flatten: 0.0,
            rms_norm: (avg_rms / max_rms).clamp(0.15, 1.0),
            contour_semitones: contour,
        });
        seg.clear();
    };

    for est in frames_est {
        if !est.voiced {
            flush_seg(&mut seg, &mut blobs);
            continue;
        }
        if let Some(first) = seg.first() {
            let prev = seg.last().unwrap();
            let pitch_jump = (est.midi - first.midi).abs() > 0.85;
            let onset_reattack = est.rms > prev.rms * 2.1 && seg.len() >= 4;
            if pitch_jump || onset_reattack {
                flush_seg(&mut seg, &mut blobs);
            }
        }
        seg.push(est);
    }
    flush_seg(&mut seg, &mut blobs);

    blobs
}
