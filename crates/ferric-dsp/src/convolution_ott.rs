//! 1) Zero-Latency Uniformly Partitioned FFT Convolution Reverb & Guitar Cabinet IR Engine (`ConvolutionIrProcessor`)
//! 2) 3-Band Linkwitz-Riley Upward + Downward Multiband ("OTT") Compressor (`MultibandOttProcessor`)

use crate::biquad::StereoBiquad;
use crate::metering::db_to_lin;
use ferric_model::AudioBuffer;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::f32::consts::TAU;
use std::sync::Arc;

const PART_LEN: usize = 256;       // Partition block B = 256 samples (~5.3ms)
const FFT_LEN: usize = PART_LEN * 2; // 2B = 512-point overlap-add FFT
const DIRECT_HEAD: usize = 64;     // Zero-latency direct FIR head for ultra-crisp cabinet transients!
const MAX_PARTITIONS: usize = 320; // Up to 320 * 256 = 81,920 samples (~1.71s true stereo IR tail @ 48kHz)

/// Zero-Latency Hybrid Direct-FIR + Uniformly-Partitioned Overlap-Add FFT Convolution Engine!
pub struct ConvolutionIrProcessor {
    fft_fwd: Arc<dyn Fft<f32>>,
    fft_inv: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    // Direct time-domain head (`0..DIRECT_HEAD`) for 0-sample latency on 4x12 Guitar Cab IRs!
    head_ir_l: [f32; DIRECT_HEAD],
    head_ir_r: [f32; DIRECT_HEAD],
    head_hist_l: [f32; DIRECT_HEAD],
    head_hist_r: [f32; DIRECT_HEAD],
    head_pos: usize,
    // Partitioned FFT tail (`DIRECT_HEAD ..`)
    ir_parts_l: Vec<Vec<Complex<f32>>>,
    ir_parts_r: Vec<Vec<Complex<f32>>>,
    fdl_l: Vec<Vec<Complex<f32>>>,
    fdl_r: Vec<Vec<Complex<f32>>>,
    fdl_pos: usize,
    num_parts: usize,
    in_acc_l: [f32; PART_LEN],
    in_acc_r: [f32; PART_LEN],
    out_ready_l: [f32; PART_LEN],
    out_ready_r: [f32; PART_LEN],
    overlap_l: [f32; PART_LEN],
    overlap_r: [f32; PART_LEN],
    block_cursor: usize,
    fft_buf_l: Vec<Complex<f32>>,
    fft_buf_r: Vec<Complex<f32>>,
    accum_l: Vec<Complex<f32>>,
    accum_r: Vec<Complex<f32>>,
    predelay_l: Vec<f32>,
    predelay_r: Vec<f32>,
    predelay_pos: usize,
    low_cut: StereoBiquad,
    high_damp: StereoBiquad,
    active_space_mode: i32,
    active_stretch_pct: i32,
    custom_ir: Option<AudioBuffer>,
    sample_rate: f32,
}

impl ConvolutionIrProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut planner = FftPlanner::<f32>::new();
        let fft_fwd = planner.plan_fft_forward(FFT_LEN);
        let fft_inv = planner.plan_fft_inverse(FFT_LEN);
        let scratch_len = fft_fwd
            .get_inplace_scratch_len()
            .max(fft_inv.get_inplace_scratch_len());

        let mut proc = Self {
            fft_fwd,
            fft_inv,
            scratch: vec![Complex::new(0.0, 0.0); scratch_len],
            head_ir_l: [0.0; DIRECT_HEAD],
            head_ir_r: [0.0; DIRECT_HEAD],
            head_hist_l: [0.0; DIRECT_HEAD],
            head_hist_r: [0.0; DIRECT_HEAD],
            head_pos: 0,
            ir_parts_l: (0..MAX_PARTITIONS)
                .map(|_| vec![Complex::new(0.0, 0.0); FFT_LEN])
                .collect(),
            ir_parts_r: (0..MAX_PARTITIONS)
                .map(|_| vec![Complex::new(0.0, 0.0); FFT_LEN])
                .collect(),
            fdl_l: (0..MAX_PARTITIONS)
                .map(|_| vec![Complex::new(0.0, 0.0); FFT_LEN])
                .collect(),
            fdl_r: (0..MAX_PARTITIONS)
                .map(|_| vec![Complex::new(0.0, 0.0); FFT_LEN])
                .collect(),
            fdl_pos: 0,
            num_parts: 1,
            in_acc_l: [0.0; PART_LEN],
            in_acc_r: [0.0; PART_LEN],
            out_ready_l: [0.0; PART_LEN],
            out_ready_r: [0.0; PART_LEN],
            overlap_l: [0.0; PART_LEN],
            overlap_r: [0.0; PART_LEN],
            block_cursor: 0,
            fft_buf_l: vec![Complex::new(0.0, 0.0); FFT_LEN],
            fft_buf_r: vec![Complex::new(0.0, 0.0); FFT_LEN],
            accum_l: vec![Complex::new(0.0, 0.0); FFT_LEN],
            accum_r: vec![Complex::new(0.0, 0.0); FFT_LEN],
            predelay_l: vec![0.0; (sr as usize / 4).max(12_000)],
            predelay_r: vec![0.0; (sr as usize / 4).max(12_000)],
            predelay_pos: 0,
            low_cut: StereoBiquad::default(),
            high_damp: StereoBiquad::default(),
            active_space_mode: -1,
            active_stretch_pct: 100,
            custom_ir: None,
            sample_rate: sr,
        };
        proc.rebuild_ir(0, 100);
        proc
    }

    pub fn set_custom_ir(&mut self, ir: Option<AudioBuffer>) {
        self.custom_ir = ir;
        self.active_space_mode = -1; // Force IR partition recompile
    }

    /// Compiles any stereo IR (`ir_l`, `ir_r`) into the zero-latency direct FIR head + FFT partitions!
    pub fn load_raw_ir(&mut self, ir_l: &[f32], ir_r: &[f32]) {
        let max_smp = DIRECT_HEAD + MAX_PARTITIONS * PART_LEN;
        let len = ir_l.len().min(ir_r.len()).min(max_smp);
        self.head_ir_l.fill(0.0);
        self.head_ir_r.fill(0.0);

        let head_n = len.min(DIRECT_HEAD);
        self.head_ir_l[..head_n].copy_from_slice(&ir_l[..head_n]);
        self.head_ir_r[..head_n].copy_from_slice(&ir_r[..head_n]);

        if len <= PART_LEN {
            // Short IR (e.g. Guitar Cabinet IR!) — pad tail cleanly
            self.num_parts = 1;
            self.ir_parts_l[0].fill(Complex::new(0.0, 0.0));
            self.ir_parts_r[0].fill(Complex::new(0.0, 0.0));
            if len > DIRECT_HEAD {
                for i in DIRECT_HEAD..len {
                    self.ir_parts_l[0][i] = Complex::new(ir_l[i], 0.0);
                    self.ir_parts_r[0][i] = Complex::new(ir_r[i], 0.0);
                }
                self.fft_fwd
                    .process_with_scratch(&mut self.ir_parts_l[0], &mut self.scratch);
                self.fft_fwd
                    .process_with_scratch(&mut self.ir_parts_r[0], &mut self.scratch);
            }
            return;
        }

        // Partition IR in `PART_LEN` blocks, zeroing `[0..DIRECT_HEAD]` in partition 0 since `head_ir` handles it at 0 latency!
        let parts = len.div_ceil(PART_LEN).clamp(1, MAX_PARTITIONS);
        self.num_parts = parts;
        for p in 0..parts {
            self.ir_parts_l[p].fill(Complex::new(0.0, 0.0));
            self.ir_parts_r[p].fill(Complex::new(0.0, 0.0));
            let base = p * PART_LEN;
            for k in 0..PART_LEN {
                let idx = base + k;
                if idx >= DIRECT_HEAD && idx < len {
                    self.ir_parts_l[p][k] = Complex::new(ir_l[idx], 0.0);
                    self.ir_parts_r[p][k] = Complex::new(ir_r[idx], 0.0);
                }
            }
            self.fft_fwd
                .process_with_scratch(&mut self.ir_parts_l[p], &mut self.scratch);
            self.fft_fwd
                .process_with_scratch(&mut self.ir_parts_r[p], &mut self.scratch);
        }
        for p in 0..parts {
            self.fdl_l[p].fill(Complex::new(0.0, 0.0));
            self.fdl_r[p].fill(Complex::new(0.0, 0.0));
        }
        self.overlap_l.fill(0.0);
        self.overlap_r.fill(0.0);
    }

    fn rebuild_ir(&mut self, mode: i32, stretch_pct: i32) {
        self.active_space_mode = mode;
        self.active_stretch_pct = stretch_pct;
        let stretch = (stretch_pct as f32 * 0.01).clamp(0.5, 2.0);
        let sr = self.sample_rate;

        if mode == 4 {
            if let Some(buf) = self.custom_ir.clone() {
                let ratio = (buf.sample_rate as f64 / sr as f64) / (stretch as f64);
                let out_len = ((buf.len_frames() as f64) / ratio)
                    .ceil()
                    .min((MAX_PARTITIONS * PART_LEN) as f64) as usize;
                let mut il = Vec::with_capacity(out_len);
                let mut ir = Vec::with_capacity(out_len);
                for i in 0..out_len {
                    let (l, r) = buf.sample_interpolated(i as f64 * ratio);
                    il.push(l * 0.18);
                    ir.push(r * 0.18);
                }
                self.load_raw_ir(&il, &ir);
                return;
            }
        }

        let (dur_s, decay_rate, is_cab) = match mode {
            0 => (1.45 * stretch, 3.8 / stretch, false), // Lush Symphony Hall IR
            1 => (1.10 * stretch, 5.2 / stretch, false), // Dense Vintage EMT 140 Plate IR
            2 => (0.85 * stretch, 6.8 / stretch, false), // Dispersive Studio Spring Tank IR
            _ => (0.045, 95.0, true),                    // 4x12 Celestion Close-Mic Speaker Cab IR!
        };

        let len = ((dur_s * sr) as usize).clamp(128, MAX_PARTITIONS * PART_LEN);
        let mut ir_l = vec![0.0_f32; len];
        let mut ir_r = vec![0.0_f32; len];
        let mut rng_l = 0x1357_9BDF_u32;
        let mut rng_r = 0x2468_ACE1_u32;

        if is_cab {
            // Realistic minimum-phase multi-resonance 4x12 Celestion V30 speaker impulse response!
            for i in 0..len {
                let t = i as f32 / sr;
                let env = (-t * 145.0).exp();
                let cone = (TAU * 118.0 * t).sin() * (-t * 65.0).exp() * 0.45
                    + (TAU * 820.0 * t).sin() * env * 0.35
                    + (TAU * 2380.0 * t).sin() * env * 0.65
                    + (TAU * 3650.0 * t).sin() * (-t * 210.0).exp() * 0.42
                    + (if i == 1 { 0.85 } else { 0.0 });
                ir_l[i] = cone * 0.28;
                ir_r[i] = cone * 0.28;
            }
        } else {
            // Early acoustic reflections + exponentially decaying velvet-noise diffuse tail
            let early_taps_ms: &[(f32, f32, f32)] = &[
                (7.3, 0.55, 0.20),
                (13.1, 0.22, 0.52),
                (19.7, 0.44, 0.35),
                (28.4, 0.30, 0.46),
                (37.9, 0.38, 0.28),
            ];
            for &(ms, gl, gr) in early_taps_ms {
                let idx = ((ms * 0.001 * sr) as usize).min(len - 1);
                ir_l[idx] += gl * 0.14;
                ir_r[idx] += gr * 0.14;
            }

            let mut lp_l = 0.0_f32;
            let mut lp_r = 0.0_f32;
            for i in 0..len {
                let t = i as f32 / sr;
                rng_l ^= rng_l << 13;
                rng_l ^= rng_l >> 17;
                rng_l ^= rng_l << 5;
                rng_r ^= rng_r << 13;
                rng_r ^= rng_r >> 17;
                rng_r ^= rng_r << 5;
                let nl = (rng_l as i32 as f32) / (i32::MAX as f32);
                let nr = (rng_r as i32 as f32) / (i32::MAX as f32);

                let hf_damp = (0.18 + 0.75 * (-t * 3.2).exp()).clamp(0.1, 0.95);
                lp_l += hf_damp * (nl - lp_l);
                lp_r += hf_damp * (nr - lp_r);

                let spring_chirp = if mode == 2 {
                    1.0 + 0.55 * (TAU * (180.0 + 900.0 * (-t * 14.0).exp()) * t).sin()
                } else {
                    1.0
                };
                let env = (-t * decay_rate).exp() * (t / 0.006).min(1.0);
                ir_l[i] += lp_l * env * spring_chirp * 0.028;
                ir_r[i] += lp_r * env * spring_chirp * 0.028;
            }
        }

        self.load_raw_ir(&ir_l, &ir_r);
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        let mode = params.first().copied().unwrap_or(0.0).round().clamp(0.0, 4.0) as i32;
        let predelay_ms = params.get(1).copied().unwrap_or(12.0).clamp(0.0, 120.0);
        let stretch_pct = params.get(2).copied().unwrap_or(100.0).round().clamp(50.0, 200.0) as i32;
        let low_cut_hz = params.get(3).copied().unwrap_or(90.0);
        let high_damp_hz = params.get(4).copied().unwrap_or(11_000.0);
        let out_gain = db_to_lin(params.get(5).copied().unwrap_or(0.0));

        if mode != self.active_space_mode || stretch_pct != self.active_stretch_pct {
            self.rebuild_ir(mode, stretch_pct);
        }

        self.low_cut.set_highpass(self.sample_rate, low_cut_hz, 0.707);
        self.high_damp
            .set_lowpass(self.sample_rate, high_damp_hz, 0.707);

        let pd_cap = self.predelay_l.len();
        let pd_smp = ((predelay_ms * 0.001 * self.sample_rate) as usize).min(pd_cap - 2);
        let head_mask = DIRECT_HEAD - 1;

        let frames = left.len().min(right.len());
        for i in 0..frames {
            self.predelay_l[self.predelay_pos] = left[i];
            self.predelay_r[self.predelay_pos] = right[i];
            let pd_read = (self.predelay_pos + pd_cap - pd_smp) % pd_cap;
            let in_l = self.predelay_l[pd_read];
            let in_r = self.predelay_r[pd_read];
            self.predelay_pos = (self.predelay_pos + 1) % pd_cap;

            // 1. Zero-latency Direct FIR Head (`0..64` samples)
            self.head_hist_l[self.head_pos] = in_l;
            self.head_hist_r[self.head_pos] = in_r;
            let mut sum_l = 0.0_f32;
            let mut sum_r = 0.0_f32;
            for k in 0..DIRECT_HEAD {
                let h_idx = (self.head_pos + DIRECT_HEAD - k) & head_mask;
                sum_l += self.head_hist_l[h_idx] * self.head_ir_l[k];
                sum_r += self.head_hist_r[h_idx] * self.head_ir_r[k];
            }
            self.head_pos = (self.head_pos + 1) & head_mask;

            // 2. Partitioned FFT Tail
            let c = self.block_cursor;
            let tail_l = self.out_ready_l[c];
            let tail_r = self.out_ready_r[c];
            self.in_acc_l[c] = in_l;
            self.in_acc_r[c] = in_r;
            self.block_cursor += 1;

            if self.block_cursor == PART_LEN {
                self.block_cursor = 0;
                self.process_fft_partition();
            }

            let (fl, fr) = self.low_cut.process_stereo(sum_l + tail_l, sum_r + tail_r);
            let (fl, fr) = self.high_damp.process_stereo(fl, fr);
            left[i] = fl * out_gain;
            right[i] = fr * out_gain;
        }
    }

    fn process_fft_partition(&mut self) {
        // Transform current `PART_LEN` input block padded with `PART_LEN` zeros -> `FFT_LEN`
        for k in 0..PART_LEN {
            self.fft_buf_l[k] = Complex::new(self.in_acc_l[k], 0.0);
            self.fft_buf_r[k] = Complex::new(self.in_acc_r[k], 0.0);
            self.fft_buf_l[PART_LEN + k] = Complex::new(0.0, 0.0);
            self.fft_buf_r[PART_LEN + k] = Complex::new(0.0, 0.0);
        }
        self.fft_fwd
            .process_with_scratch(&mut self.fft_buf_l, &mut self.scratch);
        self.fft_fwd
            .process_with_scratch(&mut self.fft_buf_r, &mut self.scratch);

        self.fdl_l[self.fdl_pos].copy_from_slice(&self.fft_buf_l);
        self.fdl_r[self.fdl_pos].copy_from_slice(&self.fft_buf_r);

        self.accum_l.fill(Complex::new(0.0, 0.0));
        self.accum_r.fill(Complex::new(0.0, 0.0));

        let n_parts = self.num_parts;
        for p in 0..n_parts {
            let fdl_idx = (self.fdl_pos + n_parts - p) % n_parts;
            let xl = &self.fdl_l[fdl_idx];
            let xr = &self.fdl_r[fdl_idx];
            let hl = &self.ir_parts_l[p];
            let hr = &self.ir_parts_r[p];
            for bin in 0..FFT_LEN {
                self.accum_l[bin] += xl[bin] * hl[bin];
                self.accum_r[bin] += xr[bin] * hr[bin];
            }
        }
        self.fdl_pos = (self.fdl_pos + 1) % n_parts.max(1);

        self.fft_inv
            .process_with_scratch(&mut self.accum_l, &mut self.scratch);
        self.fft_inv
            .process_with_scratch(&mut self.accum_r, &mut self.scratch);

        let inv_scale = 1.0 / FFT_LEN as f32;
        for k in 0..PART_LEN {
            self.out_ready_l[k] = self.accum_l[k].re * inv_scale + self.overlap_l[k];
            self.out_ready_r[k] = self.accum_r[k].re * inv_scale + self.overlap_r[k];
            self.overlap_l[k] = self.accum_l[PART_LEN + k].re * inv_scale;
            self.overlap_r[k] = self.accum_r[PART_LEN + k].re * inv_scale;
        }
    }
}

/// 3-Band Linkwitz-Riley (24 dB/oct phase-coherent) Upward + Downward ("OTT") Multiband Compressor!
pub struct MultibandOttProcessor {
    lp_low_1: StereoBiquad,
    lp_low_2: StereoBiquad,
    hp_low_1: StereoBiquad,
    hp_low_2: StereoBiquad,
    lp_hi_1: StereoBiquad,
    lp_hi_2: StereoBiquad,
    hp_hi_1: StereoBiquad,
    hp_hi_2: StereoBiquad,
    env_db: [f32; 3],
    pub band_gain_db: [f32; 3],
    sample_rate: f32,
}

impl MultibandOttProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            lp_low_1: StereoBiquad::default(),
            lp_low_2: StereoBiquad::default(),
            hp_low_1: StereoBiquad::default(),
            hp_low_2: StereoBiquad::default(),
            lp_hi_1: StereoBiquad::default(),
            lp_hi_2: StereoBiquad::default(),
            hp_hi_1: StereoBiquad::default(),
            hp_hi_2: StereoBiquad::default(),
            env_db: [-36.0; 3],
            band_gain_db: [0.0; 3],
            sample_rate: sample_rate.max(8_000.0),
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        let depth = (params.first().copied().unwrap_or(55.0) * 0.01).clamp(0.0, 1.0);
        let time_scale = (params.get(1).copied().unwrap_or(100.0) * 0.01).clamp(0.1, 3.0);
        let x_low = params.get(2).copied().unwrap_or(120.0).clamp(50.0, 500.0);
        let x_hi = params
            .get(3)
            .copied()
            .unwrap_or(2500.0)
            .clamp(x_low + 400.0, 10_000.0);
        let g_low = params.get(4).copied().unwrap_or(0.5);
        let g_mid = params.get(5).copied().unwrap_or(0.0);
        let g_hi = params.get(6).copied().unwrap_or(1.5);
        let out_db = params.get(7).copied().unwrap_or(0.0);

        // Linkwitz-Riley 4th-order = two cascaded Butterworth (Q = 0.7071) filters!
        let sr = self.sample_rate;
        self.lp_low_1.set_lowpass(sr, x_low, 0.7071);
        self.lp_low_2.set_lowpass(sr, x_low, 0.7071);
        self.hp_low_1.set_highpass(sr, x_low, 0.7071);
        self.hp_low_2.set_highpass(sr, x_low, 0.7071);

        self.lp_hi_1.set_lowpass(sr, x_hi, 0.7071);
        self.lp_hi_2.set_lowpass(sr, x_hi, 0.7071);
        self.hp_hi_1.set_highpass(sr, x_hi, 0.7071);
        self.hp_hi_2.set_highpass(sr, x_hi, 0.7071);

        let atk_c = (-1.0 / (0.012 * time_scale * sr).max(1.0)).exp();
        let rel_c = (-1.0 / (0.115 * time_scale * sr).max(1.0)).exp();

        // Classic OTT Upward & Downward thresholds per band (Low, Mid, High)
        let down_thresh = [-14.0_f32, -16.0, -18.0];
        let up_thresh = [-34.0_f32, -36.0, -38.0];
        let band_trim = [g_low, g_mid, g_hi];
        let out_lin = db_to_lin(out_db);

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let (in_l, in_r) = (left[i], right[i]);

            // Split Low vs Mid+High
            let (l1, r1) = self.lp_low_1.process_stereo(in_l, in_r);
            let (low_l, low_r) = self.lp_low_2.process_stereo(l1, r1);

            let (mh1_l, mh1_r) = self.hp_low_1.process_stereo(in_l, in_r);
            let (mh_l, mh_r) = self.hp_low_2.process_stereo(mh1_l, mh1_r);

            // Split Mid vs High
            let (m1_l, m1_r) = self.lp_hi_1.process_stereo(mh_l, mh_r);
            let (mid_l, mid_r) = self.lp_hi_2.process_stereo(m1_l, m1_r);

            let (h1_l, h1_r) = self.hp_hi_1.process_stereo(mh_l, mh_r);
            let (hi_l, hi_r) = self.hp_hi_2.process_stereo(h1_l, h1_r);

            let mut bands = [(low_l, low_r), (mid_l, mid_r), (hi_l, hi_r)];

            for b in 0..3 {
                let pk = bands[b].0.abs().max(bands[b].1.abs()).max(1e-5);
                let lvl_db = 20.0 * pk.log10();
                let coeff = if lvl_db > self.env_db[b] { atk_c } else { rel_c };
                self.env_db[b] = lvl_db + (self.env_db[b] - lvl_db) * coeff;

                let mut delta_db = 0.0_f32;
                if self.env_db[b] > down_thresh[b] {
                    // 4:1 Downward Compression above threshold
                    delta_db = -(self.env_db[b] - down_thresh[b]) * 0.75;
                } else if self.env_db[b] < up_thresh[b] && self.env_db[b] > -72.0 {
                    // Upward Compression lifts quiet details & shimmer!
                    delta_db = ((up_thresh[b] - self.env_db[b]) * 0.62).min(16.0);
                }

                let applied_db = delta_db * depth + band_trim[b];
                self.band_gain_db[b] = applied_db;
                let g = db_to_lin(applied_db);
                bands[b].0 *= g;
                bands[b].1 *= g;
            }

            left[i] = (bands[0].0 + bands[1].0 + bands[2].0) * out_lin;
            right[i] = (bands[0].1 + bands[1].1 + bands[2].1) * out_lin;
        }
    }
}
