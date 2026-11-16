use crate::biquad::StereoBiquad;
use crate::metering::db_to_lin;
use std::f32::consts::TAU;

/// Hysteresis Studio Noise Gate (separate Open & Close thresholds eliminate vocal/guitar chatter).
pub struct NoiseGateProcessor {
    env: f32,
    gain: f32,
    is_open: bool,
    hold_samples_left: usize,
    sample_rate: f32,
}

impl NoiseGateProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            env: 0.0,
            gain: 1.0,
            is_open: false,
            hold_samples_left: 0,
            sample_rate: sample_rate.max(8_000.0),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        self.process_with_sidechain(left, right, params, None);
    }

    pub fn process_with_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        sidechain_key: Option<(&[f32], &[f32])>,
    ) {
        let open_db = params.first().copied().unwrap_or(-42.0);
        let hyst_db = params.get(1).copied().unwrap_or(6.0);
        let attack_ms = params.get(2).copied().unwrap_or(1.5);
        let hold_ms = params.get(3).copied().unwrap_or(45.0);
        let release_ms = params.get(4).copied().unwrap_or(120.0);
        let floor_db = params.get(5).copied().unwrap_or(-72.0);

        let open_lin = 10.0_f32.powf(open_db / 20.0);
        let close_lin = 10.0_f32.powf((open_db - hyst_db) / 20.0);
        let floor_lin = if floor_db <= -89.0 {
            0.0
        } else {
            10.0_f32.powf(floor_db / 20.0)
        };
        let atk_c = (-1.0 / (attack_ms * 0.001 * self.sample_rate).max(1.0)).exp();
        let rel_c = (-1.0 / (release_ms * 0.001 * self.sample_rate).max(1.0)).exp();
        let hold_frames = (hold_ms * 0.001 * self.sample_rate) as usize;
        let det_rel = (-1.0 / (0.015 * self.sample_rate)).exp();

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let peak = if let Some((sc_l, sc_r)) = sidechain_key {
                sc_l.get(i)
                    .copied()
                    .unwrap_or(0.0)
                    .abs()
                    .max(sc_r.get(i).copied().unwrap_or(0.0).abs())
            } else {
                left[i].abs().max(right[i].abs())
            };
            self.env = if peak > self.env {
                peak
            } else {
                self.env * det_rel + peak * (1.0 - det_rel)
            };

            if self.env >= open_lin {
                self.is_open = true;
                self.hold_samples_left = hold_frames;
            } else if self.is_open {
                if self.hold_samples_left > 0 {
                    self.hold_samples_left -= 1;
                } else if self.env < close_lin {
                    self.is_open = false;
                }
            }

            let target = if self.is_open { 1.0 } else { floor_lin };
            let coeff = if target > self.gain { atk_c } else { rel_c };
            self.gain = target + (self.gain - target) * coeff;

            left[i] *= self.gain;
            right[i] *= self.gain;
        }
    }
}

/// 4-Band Studio Parametric EQ + High-Pass Filter with live UI frequency-response helper.
pub struct ParametricEqProcessor {
    hpf: StereoBiquad,
    low_shelf: StereoBiquad,
    mid1: StereoBiquad,
    mid2: StereoBiquad,
    high_shelf: StereoBiquad,
    sample_rate: f32,
    out_gain: f32,
}

impl ParametricEqProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            hpf: StereoBiquad::default(),
            low_shelf: StereoBiquad::default(),
            mid1: StereoBiquad::default(),
            mid2: StereoBiquad::default(),
            high_shelf: StereoBiquad::default(),
            sample_rate: sample_rate.max(8_000.0),
            out_gain: 1.0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    pub fn update_coefficients(&mut self, params: &[f32]) {
        let hpf_hz = params.first().copied().unwrap_or(35.0);
        let low_hz = params.get(1).copied().unwrap_or(110.0);
        let low_db = params.get(2).copied().unwrap_or(0.0);
        let m1_hz = params.get(3).copied().unwrap_or(650.0);
        let m1_db = params.get(4).copied().unwrap_or(0.0);
        let m1_q = params.get(5).copied().unwrap_or(1.2);
        let m2_hz = params.get(6).copied().unwrap_or(2800.0);
        let m2_db = params.get(7).copied().unwrap_or(0.0);
        let m2_q = params.get(8).copied().unwrap_or(1.4);
        let hi_hz = params.get(9).copied().unwrap_or(8500.0);
        let hi_db = params.get(10).copied().unwrap_or(0.0);
        let out_db = params.get(11).copied().unwrap_or(0.0);

        self.hpf.set_highpass(self.sample_rate, hpf_hz, 0.707);
        self.low_shelf.set_low_shelf(self.sample_rate, low_hz, low_db);
        self.mid1.set_peaking(self.sample_rate, m1_hz, m1_q, m1_db);
        self.mid2.set_peaking(self.sample_rate, m2_hz, m2_q, m2_db);
        self.high_shelf.set_high_shelf(self.sample_rate, hi_hz, hi_db);
        self.out_gain = 10.0_f32.powf(out_db / 20.0);
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        self.update_coefficients(params);
        let hpf_active = params.first().copied().unwrap_or(20.0) > 21.0;
        let frames = left.len().min(right.len());
        for i in 0..frames {
            let (mut l, mut r) = (left[i], right[i]);
            if hpf_active {
                (l, r) = self.hpf.process_stereo(l, r);
            }
            (l, r) = self.low_shelf.process_stereo(l, r);
            (l, r) = self.mid1.process_stereo(l, r);
            (l, r) = self.mid2.process_stereo(l, r);
            (l, r) = self.high_shelf.process_stereo(l, r);
            left[i] = l * self.out_gain;
            right[i] = r * self.out_gain;
        }
    }

    /// Evaluates composite EQ curve in dB at `freq_hz` for UI rendering.
    pub fn response_db_at(sample_rate: f32, params: &[f32], freq_hz: f32) -> f32 {
        let mut tmp = Self::new(sample_rate);
        tmp.update_coefficients(params);
        let mut db = params.get(11).copied().unwrap_or(0.0);
        if params.first().copied().unwrap_or(20.0) > 21.0 {
            db += tmp.hpf.magnitude_db(sample_rate, freq_hz);
        }
        db += tmp.low_shelf.magnitude_db(sample_rate, freq_hz);
        db += tmp.mid1.magnitude_db(sample_rate, freq_hz);
        db += tmp.mid2.magnitude_db(sample_rate, freq_hz);
        db += tmp.high_shelf.magnitude_db(sample_rate, freq_hz);
        db
    }
}

/// Soft-Knee Studio Opto/VCA Compressor with real-time Gain Reduction readout.
pub struct CompressorProcessor {
    env_db: f32,
    pub last_gain_reduction_db: f32,
    sample_rate: f32,
}

impl CompressorProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            env_db: -96.0,
            last_gain_reduction_db: 0.0,
            sample_rate: sample_rate.max(8_000.0),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        self.process_with_sidechain(left, right, params, None);
    }

    pub fn process_with_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        params: &[f32],
        sidechain_key: Option<(&[f32], &[f32])>,
    ) {
        let thresh_db = params.first().copied().unwrap_or(-18.0);
        let ratio = params.get(1).copied().unwrap_or(4.0).max(1.0);
        let attack_ms = params.get(2).copied().unwrap_or(8.0);
        let release_ms = params.get(3).copied().unwrap_or(110.0);
        let knee_db = params.get(4).copied().unwrap_or(4.0).max(0.0);
        let makeup_db = params.get(5).copied().unwrap_or(3.0);

        let atk_c = (-1.0 / (attack_ms * 0.001 * self.sample_rate).max(1.0)).exp();
        let rel_c = (-1.0 / (release_ms * 0.001 * self.sample_rate).max(1.0)).exp();
        let slope = 1.0 - 1.0 / ratio;
        let mut max_gr = 0.0_f32;

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let det = if let Some((sc_l, sc_r)) = sidechain_key {
                sc_l.get(i)
                    .copied()
                    .unwrap_or(0.0)
                    .abs()
                    .max(sc_r.get(i).copied().unwrap_or(0.0).abs())
                    .max(1e-6)
            } else {
                left[i].abs().max(right[i].abs()).max(1e-6)
            };
            let in_db = 20.0 * det.log10();
            let over = in_db - thresh_db;

            let target_gr = if knee_db > 0.1 && over > -knee_db * 0.5 && over < knee_db * 0.5 {
                let x = over + knee_db * 0.5;
                slope * (x * x) / (2.0 * knee_db)
            } else if over >= knee_db * 0.5 {
                over * slope
            } else {
                0.0
            };

            let coeff = if target_gr > self.env_db { atk_c } else { rel_c };
            self.env_db = target_gr + (self.env_db - target_gr) * coeff;
            if self.env_db > max_gr {
                max_gr = self.env_db;
            }

            let gain_lin = 10.0_f32.powf((makeup_db - self.env_db) / 20.0);
            left[i] *= gain_lin;
            right[i] *= gain_lin;
        }
        self.last_gain_reduction_db = max_gr;
    }
}

/// Ferric Tube Amp & Resonant Speaker Cabinet Modeler for plugging guitars/basses straight in!
pub struct AmpCabProcessor {
    in_hp: StereoBiquad,
    bass_eq: StereoBiquad,
    mid_eq: StereoBiquad,
    treble_eq: StereoBiquad,
    cab_hp: StereoBiquad,
    cab_res: StereoBiquad,
    cab_lp: StereoBiquad,
    dc_block: StereoBiquad,
    sample_rate: f32,
}

impl AmpCabProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let mut dc = StereoBiquad::default();
        dc.set_highpass(sr, 25.0, 0.707);
        Self {
            in_hp: StereoBiquad::default(),
            bass_eq: StereoBiquad::default(),
            mid_eq: StereoBiquad::default(),
            treble_eq: StereoBiquad::default(),
            cab_hp: StereoBiquad::default(),
            cab_res: StereoBiquad::default(),
            cab_lp: StereoBiquad::default(),
            dc_block: dc,
            sample_rate: sr,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
        self.dc_block.set_highpass(self.sample_rate, 25.0, 0.707);
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        let drive_db = params.first().copied().unwrap_or(14.0);
        let cab_model = params.get(1).copied().unwrap_or(1.0).round() as i32;
        let bass_db = params.get(2).copied().unwrap_or(1.5);
        let mid_db = params.get(3).copied().unwrap_or(1.0);
        let treble_db = params.get(4).copied().unwrap_or(2.0);
        let presence = params.get(5).copied().unwrap_or(45.0) * 0.01;
        let out_db = params.get(6).copied().unwrap_or(-3.0);

        let sr = self.sample_rate;
        self.in_hp.set_highpass(sr, 85.0, 0.707); // Tightens low strings before tube stage
        self.bass_eq.set_low_shelf(sr, 130.0, bass_db);
        self.mid_eq.set_peaking(sr, 680.0, 1.1, mid_db);
        self.treble_eq
            .set_high_shelf(sr, 2600.0, treble_db + presence * 4.5);

        // Speaker cabinet acoustic resonance models
        match cab_model {
            0 => {
                // 1x12 Vintage Tweed
                self.cab_hp.set_highpass(sr, 95.0, 0.9);
                self.cab_res.set_peaking(sr, 2100.0, 1.6, 3.8);
                self.cab_lp.set_lowpass(sr, 4800.0, 0.85);
            }
            1 => {
                // 2x12 British Greenback
                self.cab_hp.set_highpass(sr, 82.0, 1.05);
                self.cab_res.set_peaking(sr, 2450.0, 1.8, 4.5);
                self.cab_lp.set_lowpass(sr, 5200.0, 0.9);
            }
            2 => {
                // 4x12 Modern Closed-Back
                self.cab_hp.set_highpass(sr, 72.0, 1.25);
                self.cab_res.set_peaking(sr, 2850.0, 2.0, 4.2);
                self.cab_lp.set_lowpass(sr, 5600.0, 0.85);
            }
            _ => {
                // Studio Bass DI + 8x10 Ampeg-style warmth
                self.cab_hp.set_highpass(sr, 38.0, 0.85);
                self.cab_res.set_peaking(sr, 950.0, 1.2, 2.5);
                self.cab_lp.set_lowpass(sr, 6800.0, 0.707);
            }
        }

        let drive_lin = 10.0_f32.powf(drive_db / 20.0);
        let out_lin = 10.0_f32.powf(out_db / 20.0) / drive_lin.powf(0.35);

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let (mut l, mut r) = self.in_hp.process_stereo(left[i], right[i]);
            // Asymmetric Class-A triode tube saturation (adds rich 2nd-order even harmonics!)
            l = Self::tube_stage(l * drive_lin);
            r = Self::tube_stage(r * drive_lin);
            (l, r) = self.dc_block.process_stereo(l, r);
            (l, r) = self.bass_eq.process_stereo(l, r);
            (l, r) = self.mid_eq.process_stereo(l, r);
            (l, r) = self.treble_eq.process_stereo(l, r);
            (l, r) = self.cab_hp.process_stereo(l, r);
            (l, r) = self.cab_res.process_stereo(l, r);
            (l, r) = self.cab_lp.process_stereo(l, r);
            left[i] = l * out_lin;
            right[i] = r * out_lin;
        }
    }

    #[inline]
    fn tube_stage(x: f32) -> f32 {
        let biased = x + 0.12;
        if biased >= 0.0 {
            biased / (1.0 + biased.abs())
        } else {
            (biased * 0.85).tanh()
        }
    }
}

/// Oxide Tape Saturator & Brickwall Safety Ceiling Limiter.
pub struct TapeSaturatorProcessor {
    warmth_shelf: StereoBiquad,
    limiter_env: f32,
    sample_rate: f32,
}

impl TapeSaturatorProcessor {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            warmth_shelf: StereoBiquad::default(),
            limiter_env: 1.0,
            sample_rate: sample_rate.max(8_000.0),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        let drive_db = params.first().copied().unwrap_or(5.0);
        let warmth = params.get(1).copied().unwrap_or(42.0) * 0.01;
        let ceiling_db = params.get(2).copied().unwrap_or(-0.3);

        let drive = 10.0_f32.powf(drive_db / 20.0);
        let ceiling = 10.0_f32.powf(ceiling_db / 20.0);
        let comp_scale = 1.0 / drive.tanh().max(0.25);
        self.warmth_shelf
            .set_low_shelf(self.sample_rate, 160.0, warmth * 2.4);
        let rel = (-1.0 / (0.060 * self.sample_rate)).exp();

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let (mut l, mut r) = self.warmth_shelf.process_stereo(left[i], right[i]);
            l = (l * drive).tanh() * comp_scale;
            r = (r * drive).tanh() * comp_scale;

            let peak = l.abs().max(r.abs());
            let needed = if peak > ceiling { ceiling / peak } else { 1.0 };
            self.limiter_env = if needed < self.limiter_env {
                needed // Instant brickwall catch
            } else {
                needed + (self.limiter_env - needed) * rel
            };

            left[i] = (l * self.limiter_env).clamp(-ceiling, ceiling);
            right[i] = (r * self.limiter_env).clamp(-ceiling, ceiling);
        }
    }
}

/// Mono low-band feedback delay for adding a kick-driven sub tail.
pub struct KickRumbleProcessor {
    delay: Vec<f32>,
    write: usize,
    sample_rate: f32,
    delay_samples: f32,
    bass: f32,
    tone: [f32; 2],
}

impl KickRumbleProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(8_000.0);
        Self {
            delay: vec![0.0; (sample_rate * 0.240).ceil() as usize + 2],
            write: 0,
            sample_rate,
            delay_samples: sample_rate * 0.080,
            bass: 0.0,
            tone: [0.0; 2],
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        let param = |i: usize, default: f32, min: f32, max: f32| {
            params.get(i).copied().filter(|v| v.is_finite()).unwrap_or(default).clamp(min, max)
        };
        let delay_target = self.sample_rate * 0.001 * param(0, 80.0, 15.0, 240.0);
        let feedback = param(1, 63.0, 0.0, 85.0) * 0.01;
        let coefficient = |hz: f32| 1.0 - (-TAU * hz / self.sample_rate).exp();
        let bass_coef = coefficient(param(2, 32.0, 20.0, 80.0));
        let tone_coef = coefficient(param(3, 155.0, 80.0, 320.0));
        let glide = coefficient(7.5);
        let drive = db_to_lin(param(4, 8.0, 0.0, 18.0));
        let len = self.delay.len();
        for (l, r) in left.iter_mut().zip(right) {
            self.delay_samples += glide * (delay_target - self.delay_samples);
            let back = self.delay_samples as usize;
            let fraction = self.delay_samples - back as f32;
            let read = (self.write + len - back) % len;
            let tail = self.delay[read] * (1.0 - fraction)
                + self.delay[(read + len - 1) % len] * fraction;

            let input = (*l + *r) * 0.5 + feedback * tail;
            self.bass += bass_coef * (input - self.bass);
            self.tone[0] += tone_coef * (input - self.bass - self.tone[0]);
            self.tone[1] += tone_coef * (self.tone[0] - self.tone[1]);
            let driven = self.tone[1] * drive;
            let shaped = driven / (1.0 + driven.abs());
            // Run at unity or lower gain in the feedback path, even with drive at maximum.
            self.delay[self.write] = shaped / drive;
            self.write = (self.write + 1) % len;
            let wet = tail * drive;
            let wet = wet / (1.0 + wet.abs());
            *l = wet;
            *r = wet;
        }
    }
}

/// Tempo-Synced Ping-Pong Tape Delay with Wow/Flutter & High-Cut Damping.
pub struct StereoDelayProcessor {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_pos: usize,
    lfo_phase: f32,
    damp_filter: StereoBiquad,
    sample_rate: f32,
}

impl StereoDelayProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let cap = (sr as usize * 3).max(96_000);
        Self {
            buf_l: vec![0.0; cap],
            buf_r: vec![0.0; cap],
            write_pos: 0,
            lfo_phase: 0.0,
            damp_filter: StereoBiquad::default(),
            sample_rate: sr,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
        let needed = (self.sample_rate as usize * 3).max(96_000);
        if self.buf_l.len() < needed {
            self.buf_l.resize(needed, 0.0);
            self.buf_r.resize(needed, 0.0);
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32], bpm: f64) {
        let free_ms = params.first().copied().unwrap_or(375.0);
        let sync_mode = params.get(1).copied().unwrap_or(2.0).round() as i32;
        let feedback = (params.get(2).copied().unwrap_or(38.0) * 0.01).clamp(0.0, 0.93);
        let ping_pong = (params.get(3).copied().unwrap_or(65.0) * 0.01).clamp(0.0, 1.0);
        let tone_hz = params.get(4).copied().unwrap_or(4200.0);
        let flutter = (params.get(5).copied().unwrap_or(18.0) * 0.01).clamp(0.0, 1.0);

        let spb = 60.0 / bpm.max(30.0) as f32;
        let delay_secs = match sync_mode {
            1 => spb * 0.25, // 1/16
            2 => spb * 0.50, // 1/8
            3 => spb * 0.75, // Dotted 1/8
            4 => spb * 1.00, // 1/4
            _ => free_ms * 0.001,
        };

        self.damp_filter.set_lowpass(self.sample_rate, tone_hz, 0.707);
        let cap = self.buf_l.len();
        let base_samples = (delay_secs * self.sample_rate).clamp(32.0, (cap - 64) as f32);
        let lfo_inc = 1.35 / self.sample_rate;

        let frames = left.len().min(right.len());
        for i in 0..frames {
            self.lfo_phase = (self.lfo_phase + lfo_inc).fract();
            let mod_samples = (TAU * self.lfo_phase).sin() * flutter * 14.0;
            let tap_dist = (base_samples + mod_samples).clamp(8.0, (cap - 8) as f32);

            let read_f = (self.write_pos as f32 + cap as f32 - tap_dist).rem_euclid(cap as f32);
            let idx0 = read_f.floor() as usize % cap;
            let idx1 = (idx0 + 1) % cap;
            let frac = read_f.fract();

            let dl = self.buf_l[idx0] + (self.buf_l[idx1] - self.buf_l[idx0]) * frac;
            let dr = self.buf_r[idx0] + (self.buf_r[idx1] - self.buf_r[idx0]) * frac;
            let ( dl_f, dr_f ) = self.damp_filter.process_stereo(dl, dr);

            let in_l = left[i];
            let in_r = right[i];
            let mono_in = 0.5 * (in_l + in_r);

            let feed_l = in_l * (1.0 - ping_pong) + mono_in * ping_pong
                + (dl_f * (1.0 - ping_pong) + dr_f * ping_pong) * feedback;
            let feed_r = in_r * (1.0 - ping_pong)
                + (dr_f * (1.0 - ping_pong) + dl_f * ping_pong) * feedback;

            self.buf_l[self.write_pos] = feed_l.clamp(-4.0, 4.0);
            self.buf_r[self.write_pos] = feed_r.clamp(-4.0, 4.0);
            self.write_pos = (self.write_pos + 1) % cap;

            // Output pure wet signal; FxInstance handles dry/wet mix!
            left[i] = dl_f;
            right[i] = dr_f;
        }
    }
}

#[derive(Clone)]
struct CombLine {
    buf: Vec<f32>,
    pos: usize,
    filter_store: f32,
}

impl CombLine {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![0.0; size.max(16)],
            pos: 0,
            filter_store: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.filter_store = out * (1.0 - damp) + self.filter_store * damp;
        if self.filter_store.abs() < 1e-20 {
            self.filter_store = 0.0;
        }
        self.buf[self.pos] = input + self.filter_store * feedback;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
        out
    }
}

#[derive(Clone)]
struct AllpassLine {
    buf: Vec<f32>,
    pos: usize,
}

impl AllpassLine {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![0.0; size.max(8)],
            pos: 0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.pos];
        let out = -input + buf_out;
        self.buf[self.pos] = input + buf_out * 0.5;
        self.pos += 1;
        if self.pos >= self.buf.len() {
            self.pos = 0;
        }
        out
    }
}

/// Lush Stereo Plate / Hall Algorithmic Reverb (8 parallel damped combs + 4 series allpasses per ch).
pub struct PlateReverbProcessor {
    predelay_buf: Vec<f32>,
    predelay_pos: usize,
    combs_l: [CombLine; 8],
    combs_r: [CombLine; 8],
    allpass_l: [AllpassLine; 4],
    allpass_r: [AllpassLine; 4],
    low_cut: StereoBiquad,
    sample_rate: f32,
}

impl PlateReverbProcessor {
    const COMB_TUNINGS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
    const ALLPASS_TUNINGS: [usize; 4] = [556, 441, 341, 225];
    const STEREO_SPREAD: usize = 23;

    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        let scale = (sr / 44_100.0) as f64;
        let combs_l = Self::COMB_TUNINGS.map(|t| CombLine::new((t as f64 * scale) as usize));
        let combs_r = Self::COMB_TUNINGS
            .map(|t| CombLine::new(((t + Self::STEREO_SPREAD) as f64 * scale) as usize));
        let allpass_l = Self::ALLPASS_TUNINGS.map(|t| AllpassLine::new((t as f64 * scale) as usize));
        let allpass_r = Self::ALLPASS_TUNINGS
            .map(|t| AllpassLine::new(((t + Self::STEREO_SPREAD) as f64 * scale) as usize));

        Self {
            predelay_buf: vec![0.0; (sr as usize / 4).max(12_000)],
            predelay_pos: 0,
            combs_l,
            combs_r,
            allpass_l,
            allpass_r,
            low_cut: StereoBiquad::default(),
            sample_rate: sr,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        if (sr - self.sample_rate).abs() > 1.0 {
            *self = Self::new(sr);
        }
    }

    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], params: &[f32]) {
        let predelay_ms = params.first().copied().unwrap_or(18.0);
        let decay = (params.get(1).copied().unwrap_or(74.0) * 0.01).clamp(0.1, 0.98);
        let damping = (params.get(2).copied().unwrap_or(38.0) * 0.01).clamp(0.0, 0.95);
        let low_cut_hz = params.get(3).copied().unwrap_or(120.0);
        let width = (params.get(4).copied().unwrap_or(100.0) * 0.01).clamp(0.0, 1.0);

        let feedback = 0.70 + decay * 0.27;
        let damp = damping * 0.55;
        let pd_samples = ((predelay_ms * 0.001 * self.sample_rate) as usize)
            .min(self.predelay_buf.len().saturating_sub(2));
        let pd_cap = self.predelay_buf.len();
        self.low_cut.set_highpass(self.sample_rate, low_cut_hz, 0.707);

        let wet1 = 0.5 * (1.0 + width);
        let wet2 = 0.5 * (1.0 - width);

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let mono = (left[i] + right[i]) * 0.018;
            self.predelay_buf[self.predelay_pos] = mono;
            let read_idx = (self.predelay_pos + pd_cap - pd_samples) % pd_cap;
            let delayed = self.predelay_buf[read_idx];
            self.predelay_pos = (self.predelay_pos + 1) % pd_cap;

            let mut out_l = 0.0_f32;
            let mut out_r = 0.0_f32;
            for c in 0..8 {
                out_l += self.combs_l[c].process(delayed, feedback, damp);
                out_r += self.combs_r[c].process(delayed, feedback, damp);
            }
            for a in 0..4 {
                out_l = self.allpass_l[a].process(out_l);
                out_r = self.allpass_r[a].process(out_r);
            }
            let (fl, fr) = self.low_cut.process_stereo(out_l * 1.8, out_r * 1.8);
            left[i] = fl * wet1 + fr * wet2;
            right[i] = fr * wet1 + fl * wet2;
        }
    }
}
