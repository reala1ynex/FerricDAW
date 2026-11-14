use crate::biquad::StereoBiquad;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

pub const SPECTRUM_BANDS: usize = 64;
const FFT_SIZE: usize = 1024;

#[derive(Debug, Clone, Copy)]
pub struct MasterMeterSnapshot {
    pub peak_l_db: f32,
    pub peak_r_db: f32,
    pub rms_l_db: f32,
    pub rms_r_db: f32,
    pub momentary_lufs: f32,
    pub short_term_lufs: f32,
    pub phase_correlation: f32,
    pub spectrum_db: [f32; SPECTRUM_BANDS],
}

impl Default for MasterMeterSnapshot {
    fn default() -> Self {
        Self {
            peak_l_db: -96.0,
            peak_r_db: -96.0,
            rms_l_db: -96.0,
            rms_r_db: -96.0,
            momentary_lufs: -70.0,
            short_term_lufs: -70.0,
            phase_correlation: 1.0,
            spectrum_db: [-84.0; SPECTRUM_BANDS],
        }
    }
}

/// Zero-allocation real-time BS.1770 K-Weighted LUFS, Stereo Phase Correlation & 64-band FFT Analyzer.
pub struct LoudnessAndSpectrumAnalyzer {
    k_shelf: StereoBiquad,
    k_rlb: StereoBiquad,
    momentary_ms_energy: f32,
    short_term_ms_energy: f32,
    corr_lr: f32,
    corr_ll: f32,
    corr_rr: f32,
    fft_ring: [f32; FFT_SIZE],
    fft_window: [f32; FFT_SIZE],
    fft_work: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
    fft_plan: Arc<dyn Fft<f32>>,
    write_pos: usize,
    samples_since_fft: usize,
    smoothed_spectrum: [f32; SPECTRUM_BANDS],
    sample_rate: f32,
}

impl LoudnessAndSpectrumAnalyzer {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut k_shelf = StereoBiquad::default();
        let mut k_rlb = StereoBiquad::default();
        // ITU-R BS.1770-4 Stage 1 (+4 dB acoustic head high-shelf @ 1681 Hz) & Stage 2 (RLB HPF @ 38 Hz)
        k_shelf.set_high_shelf(sr, 1681.97, 4.0);
        k_rlb.set_highpass(sr, 38.13, 0.5);

        let mut fft_window = [0.0_f32; FFT_SIZE];
        for (i, w) in fft_window.iter_mut().enumerate() {
            *w = 0.5 * (1.0 - (2.0 * PI * i as f32 / (FFT_SIZE - 1) as f32).cos());
        }

        let mut planner = FftPlanner::<f32>::new();
        let fft_plan = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft_plan.get_inplace_scratch_len();

        Self {
            k_shelf,
            k_rlb,
            momentary_ms_energy: 1e-9,
            short_term_ms_energy: 1e-9,
            corr_lr: 0.0,
            corr_ll: 1e-6,
            corr_rr: 1e-6,
            fft_ring: [0.0; FFT_SIZE],
            fft_window,
            fft_work: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            fft_scratch: vec![Complex::new(0.0, 0.0); scratch_len],
            fft_plan,
            write_pos: 0,
            samples_since_fft: 0,
            smoothed_spectrum: [-84.0; SPECTRUM_BANDS],
            sample_rate: sr,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
        self.k_shelf.set_high_shelf(self.sample_rate, 1681.97, 4.0);
        self.k_rlb.set_highpass(self.sample_rate, 38.13, 0.5);
    }

    pub fn analyze_block(&mut self, left: &[f32], right: &[f32]) -> MasterMeterSnapshot {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return MasterMeterSnapshot::default();
        }

        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;
        let mut sum_l2 = 0.0_f32;
        let mut sum_r2 = 0.0_f32;

        let mom_alpha = 1.0 - (-1.0 / (0.400 * self.sample_rate)).exp();
        let st_alpha = 1.0 - (-1.0 / (3.000 * self.sample_rate)).exp();
        let corr_alpha = 1.0 - (-1.0 / (0.150 * self.sample_rate)).exp();

        for i in 0..frames {
            let l = left[i];
            let r = right[i];
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
            sum_l2 += l * l;
            sum_r2 += r * r;

            // Stereo correlation
            self.corr_lr += corr_alpha * (l * r - self.corr_lr);
            self.corr_ll += corr_alpha * (l * l - self.corr_ll);
            self.corr_rr += corr_alpha * (r * r - self.corr_rr);

            // BS.1770 K-weighted loudness energy
            let (kl, kr) = self.k_shelf.process_stereo(l, r);
            let (kl, kr) = self.k_rlb.process_stereo(kl, kr);
            let k_power = kl * kl + kr * kr;
            self.momentary_ms_energy += mom_alpha * (k_power - self.momentary_ms_energy);
            self.short_term_ms_energy += st_alpha * (k_power - self.short_term_ms_energy);

            self.fft_ring[self.write_pos] = 0.5 * (l + r);
            self.write_pos = (self.write_pos + 1) & (FFT_SIZE - 1);
        }

        self.samples_since_fft += frames;
        if self.samples_since_fft >= 512 {
            self.samples_since_fft = 0;
            self.compute_fft_bands();
        }

        let rms_l = (sum_l2 / frames as f32).sqrt();
        let rms_r = (sum_r2 / frames as f32).sqrt();
        let denom = (self.corr_ll * self.corr_rr).sqrt();
        let phase_corr = if denom > 1e-7 {
            (self.corr_lr / denom).clamp(-1.0, 1.0)
        } else {
            1.0
        };

        let momentary_lufs =
            (-0.691 + 10.0 * self.momentary_ms_energy.max(1e-10).log10()).clamp(-70.0, 6.0);
        let short_term_lufs =
            (-0.691 + 10.0 * self.short_term_ms_energy.max(1e-10).log10()).clamp(-70.0, 6.0);

        MasterMeterSnapshot {
            peak_l_db: lin_to_db(peak_l),
            peak_r_db: lin_to_db(peak_r),
            rms_l_db: lin_to_db(rms_l),
            rms_r_db: lin_to_db(rms_r),
            momentary_lufs,
            short_term_lufs,
            phase_correlation: phase_corr,
            spectrum_db: self.smoothed_spectrum,
        }
    }

    fn compute_fft_bands(&mut self) {
        for i in 0..FFT_SIZE {
            let s = self.fft_ring[(self.write_pos + i) & (FFT_SIZE - 1)];
            self.fft_work[i] = Complex::new(s * self.fft_window[i], 0.0);
        }
        self.fft_plan
            .process_with_scratch(&mut self.fft_work, &mut self.fft_scratch);

        let nyquist_bins = FFT_SIZE / 2;
        let bin_hz = self.sample_rate / FFT_SIZE as f32;
        let norm = 2.0 / FFT_SIZE as f32;

        for b in 0..SPECTRUM_BANDS {
            let t0 = b as f32 / SPECTRUM_BANDS as f32;
            let t1 = (b + 1) as f32 / SPECTRUM_BANDS as f32;
            let f0 = 20.0 * (1000.0_f32).powf(t0);
            let f1 = 20.0 * (1000.0_f32).powf(t1);
            let bin0 = ((f0 / bin_hz).floor() as usize).clamp(1, nyquist_bins - 1);
            let bin1 = ((f1 / bin_hz).ceil() as usize)
                .max(bin0 + 1)
                .min(nyquist_bins);

            let mut max_mag = 0.0_f32;
            for k in bin0..bin1 {
                let c = self.fft_work[k];
                let mag = (c.re * c.re + c.im * c.im).sqrt() * norm;
                if mag > max_mag {
                    max_mag = mag;
                }
            }
            // Slight +3 dB/oct visual tilt like modern FabFilter/Bitwig analyzers
            let tilt_db = ((0.5 * (f0 + f1)) / 1000.0).log2() * 2.5;
            let db = (lin_to_db(max_mag) + tilt_db).clamp(-84.0, 6.0);
            self.smoothed_spectrum[b] = if db > self.smoothed_spectrum[b] {
                self.smoothed_spectrum[b] * 0.35 + db * 0.65
            } else {
                self.smoothed_spectrum[b] * 0.82 + db * 0.18
            };
        }
    }
}

#[inline]
pub fn lin_to_db(x: f32) -> f32 {
    if x <= 1e-5 {
        -96.0
    } else {
        (20.0 * x.log10()).clamp(-96.0, 24.0)
    }
}

#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    if db <= -90.0 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

/// Computes exact ITU-R BS.1770-4 / EBU R128 Gated Integrated Loudness (LUFS) across a full stereo buffer!
/// Uses 400 ms gating blocks with 75% overlap (100 ms step), `-70.0 LUFS` absolute gate, and `-10.0 LU` relative gate.
pub fn measure_integrated_lufs(sample_rate: u32, left: &[f32], right: &[f32]) -> f32 {
    let frames = left.len().min(right.len());
    if frames == 0 || sample_rate == 0 {
        return -70.0;
    }
    let sr = sample_rate.max(8_000) as f32;
    let mut k_shelf = StereoBiquad::default();
    let mut k_rlb = StereoBiquad::default();
    k_shelf.set_high_shelf(sr, 1681.97, 4.0);
    k_rlb.set_highpass(sr, 38.13, 0.5);

    let step = (sample_rate as usize / 10).max(1); // 100 ms sub-blocks
    let mut sub_energies: Vec<f64> = Vec::with_capacity(frames / step + 1);
    let mut cur_sum = 0.0_f64;
    let mut cur_cnt = 0usize;

    for i in 0..frames {
        let (kl, kr) = k_shelf.process_stereo(left[i], right[i]);
        let (kl, kr) = k_rlb.process_stereo(kl, kr);
        cur_sum += (kl as f64) * (kl as f64) + (kr as f64) * (kr as f64);
        cur_cnt += 1;
        if cur_cnt == step {
            sub_energies.push(cur_sum / step as f64);
            cur_sum = 0.0;
            cur_cnt = 0;
        }
    }
    if cur_cnt > step / 2 && sub_energies.is_empty() {
        sub_energies.push(cur_sum / cur_cnt as f64);
    }
    if sub_energies.is_empty() {
        return -70.0;
    }

    // Build 400 ms gating blocks (average of 4 consecutive 100 ms sub-blocks)
    let mut block_energies: Vec<f64> = Vec::new();
    if sub_energies.len() < 4 {
        let avg = sub_energies.iter().sum::<f64>() / sub_energies.len() as f64;
        block_energies.push(avg);
    } else {
        for win in sub_energies.windows(4) {
            block_energies.push((win[0] + win[1] + win[2] + win[3]) * 0.25);
        }
    }

    // Stage 1: Absolute threshold gate at -70.0 LUFS => z_abs = 10^((-70 + 0.691) / 10)
    let z_abs = 10.0_f64.powf((-70.0 + 0.691) / 10.0);
    let above_abs: Vec<f64> = block_energies
        .into_iter()
        .filter(|&z| z > z_abs)
        .collect();
    if above_abs.is_empty() {
        return -70.0;
    }

    let mean_abs = above_abs.iter().sum::<f64>() / above_abs.len() as f64;
    let gamma_abs_lufs = -0.691 + 10.0 * mean_abs.log10();

    // Stage 2: Relative threshold gate at (gamma_abs_lufs - 10.0 LU)
    let z_rel = 10.0_f64.powf((gamma_abs_lufs - 10.0 + 0.691) / 10.0);
    let above_rel: Vec<f64> = above_abs.into_iter().filter(|&z| z > z_rel).collect();
    if above_rel.is_empty() {
        return gamma_abs_lufs as f32;
    }

    let mean_rel = above_rel.iter().sum::<f64>() / above_rel.len() as f64;
    (-0.691 + 10.0 * mean_rel.log10()).clamp(-70.0, 12.0) as f32
}

/// Evaluates 4x Hermite/Catmull-Rom inter-sample True Peak (dBTP) across a stereo buffer!
pub fn measure_true_peak_dbtp(left: &[f32], right: &[f32]) -> f32 {
    let max_tp = measure_true_peak_linear(left).max(measure_true_peak_linear(right));
    lin_to_db(max_tp)
}

pub fn measure_true_peak_linear(samples: &[f32]) -> f32 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    let mut peak = 0.0_f32;
    for &s in samples {
        peak = peak.max(s.abs());
    }
    if n < 4 {
        return peak;
    }
    for i in 1..(n - 2) {
        let y0 = samples[i - 1];
        let y1 = samples[i];
        let y2 = samples[i + 1];
        let y3 = samples[i + 2];
        // Check 4x oversampled phases t = 0.25, 0.50, 0.75 via Catmull-Rom cubic interpolation
        for &t in &[0.25_f32, 0.50, 0.75] {
            let c0 = y1;
            let c1 = 0.5 * (y2 - y0);
            let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
            let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
            let interp = ((c3 * t + c2) * t + c1) * t + c0;
            peak = peak.max(interp.abs());
        }
    }
    peak
}

/// Applies transparent Mastering Loudness Normalization (`target_lufs`) followed by a
/// 2.5 ms Lookahead True-Peak Brickwall Ceiling Limiter (`ceiling_dbtp`, e.g. `-1.0 dBTP` for Spotify/Apple/YouTube)!
pub fn master_normalize_and_limit_true_peak(
    sample_rate: u32,
    left: &mut [f32],
    right: &mut [f32],
    target_lufs: Option<f32>,
    ceiling_dbtp: f32,
) -> (f32, f32) {
    let frames = left.len().min(right.len());
    if frames == 0 {
        return (-70.0, -96.0);
    }

    // 1. Apply BS.1770-4 Integrated LUFS Gain Adjustment if enabled
    if let Some(tgt_lufs) = target_lufs {
        let current_lufs = measure_integrated_lufs(sample_rate, &left[..frames], &right[..frames]);
        if current_lufs > -65.0 {
            // Cap max upward gain boost at +24 dB so near-silent tails aren't over-amplified
            let delta_db = (tgt_lufs - current_lufs).clamp(-30.0, 24.0);
            let gain = db_to_lin(delta_db);
            for i in 0..frames {
                left[i] *= gain;
                right[i] *= gain;
            }
        }
    }

    // 2. 2.5 ms Lookahead Smooth Gain-Envelope Brickwall True-Peak Limiter
    let ceiling_lin = db_to_lin(ceiling_dbtp.clamp(-18.0, 0.0)).max(0.05);
    // Slight safety margin (0.985) absorbs inter-sample Catmull-Rom overshoot cleanly
    let safe_ceiling = ceiling_lin * 0.985;
    let lookahead = ((0.0025 * sample_rate as f32).round() as usize).clamp(16, 512);
    let release_coeff = (-1.0 / (0.085 * sample_rate.max(8_000) as f32)).exp();

    // Compute instantaneous required attenuation per sample
    let mut inst_peak = vec![0.0_f32; frames];
    for i in 0..frames {
        let p = left[i].abs().max(right[i].abs());
        inst_peak[i] = p;
    }
    // Incorporate 4x inter-sample peaks into detector
    if frames >= 4 {
        for i in 1..(frames - 2) {
            for ch in [&left[..frames], &right[..frames]] {
                let (y0, y1, y2, y3) = (ch[i - 1], ch[i], ch[i + 1], ch[i + 2]);
                let mid = -0.0625 * y0 + 0.5625 * y1 + 0.5625 * y2 - 0.0625 * y3; // t = 0.5
                inst_peak[i] = inst_peak[i].max(mid.abs());
            }
        }
    }

    let mut gain_env = 1.0_f32;
    let atk_step_inv = 1.0 / lookahead as f32;

    for i in 0..frames {
        let win_end = (i + lookahead).min(frames);
        let mut future_max = inst_peak[i];
        // Scan short lookahead window
        for &pk in &inst_peak[i..win_end] {
            if pk > future_max {
                future_max = pk;
            }
        }
        let target_gain = if future_max > safe_ceiling {
            safe_ceiling / future_max
        } else {
            1.0
        };

        if target_gain < gain_env {
            let max_drop = (gain_env - target_gain) * atk_step_inv.max(0.18);
            gain_env = (gain_env - max_drop).max(target_gain);
        } else {
            gain_env = target_gain + (gain_env - target_gain) * release_coeff;
        }
        // Guarantee strict sample ceiling
        let cur_pk = inst_peak[i] * gain_env;
        if cur_pk > safe_ceiling {
            gain_env *= safe_ceiling / cur_pk;
        }

        left[i] *= gain_env;
        right[i] *= gain_env;
    }

    let final_lufs = measure_integrated_lufs(sample_rate, &left[..frames], &right[..frames]);
    let final_dbtp = measure_true_peak_dbtp(&left[..frames], &right[..frames]);
    (final_lufs, final_dbtp)
}

/// Studio TPDF (Triangular Probability Density Function) Dither Generator for 16-bit & 24-bit integer export!
/// Eliminates quantization distortion on quiet reverb tails with zero tonal coloration.
#[derive(Debug, Clone)]
pub struct TpdfDither {
    rng: u64,
}

impl Default for TpdfDither {
    fn default() -> Self {
        Self {
            rng: 0x853C_49E6_748F_EA9B,
        }
    }
}

impl TpdfDither {
    #[inline]
    fn next_uniform(&mut self) -> f32 {
        // Xorshift64* fast PRNG -> [-0.5, +0.5]
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((v >> 40) as f32 / ((1u32 << 24) as f32)) - 0.5
    }

    /// Quantizes `sample` (`-1.0 ..= +1.0`) to signed `bits`-bit integer (`16` or `24`) with optional 1-LSB TPDF dither!
    #[inline]
    pub fn quantize_sample(&mut self, sample: f32, bits: u8, enable_dither: bool) -> i32 {
        let max_pos = ((1i64 << (bits - 1)) - 1) as f32;
        let min_neg = -(1i64 << (bits - 1)) as f32;
        let scaled = sample.clamp(-1.0, 1.0) * max_pos;
        let dither = if enable_dither {
            // Sum of two independent uniform [-0.5, +0.5] RVs = [-1.0, +1.0] triangular PDF!
            self.next_uniform() + self.next_uniform()
        } else {
            0.0
        };
        (scaled + dither).round().clamp(min_neg, max_pos) as i32
    }
}
