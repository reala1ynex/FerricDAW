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
