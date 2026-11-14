use std::f32::consts::{PI, SQRT_2};

/// Stereo Transposed Direct Form II Biquad filter with denormal protection
/// and exact frequency-response evaluation for live UI curve rendering.
#[derive(Debug, Clone, Copy)]
pub struct StereoBiquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1_l: f32,
    z2_l: f32,
    z1_r: f32,
    z2_r: f32,
}

impl Default for StereoBiquad {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1_l: 0.0,
            z2_l: 0.0,
            z1_r: 0.0,
            z2_r: 0.0,
        }
    }
}

impl StereoBiquad {
    pub fn reset(&mut self) {
        self.z1_l = 0.0;
        self.z2_l = 0.0;
        self.z1_r = 0.0;
        self.z2_r = 0.0;
    }

    fn set_normalized(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        let inv_a0 = if a0.abs() > 1e-12 { 1.0 / a0 } else { 1.0 };
        self.b0 = b0 * inv_a0;
        self.b1 = b1 * inv_a0;
        self.b2 = b2 * inv_a0;
        self.a1 = a1 * inv_a0;
        self.a2 = a2 * inv_a0;
    }

    pub fn set_lowpass(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let f = freq_hz.clamp(15.0, sample_rate * 0.48);
        let q = q.max(0.1);
        let w0 = 2.0 * PI * f / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = (1.0 - cos_w0) * 0.5;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    pub fn set_highpass(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let f = freq_hz.clamp(15.0, sample_rate * 0.48);
        let q = q.max(0.1);
        let w0 = 2.0 * PI * f / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = (1.0 + cos_w0) * 0.5;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    pub fn set_peaking(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) {
        let f = freq_hz.clamp(20.0, sample_rate * 0.48);
        let q = q.max(0.15);
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    pub fn set_low_shelf(&mut self, sample_rate: f32, freq_hz: f32, gain_db: f32) {
        let f = freq_hz.clamp(20.0, sample_rate * 0.45);
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / 2.0 * SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    pub fn set_high_shelf(&mut self, sample_rate: f32, freq_hz: f32, gain_db: f32) {
        let f = freq_hz.clamp(40.0, sample_rate * 0.47);
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / 2.0 * SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha;
        self.set_normalized(b0, b1, b2, a0, a1, a2);
    }

    #[inline]
    pub fn process_stereo(&mut self, l: f32, r: f32) -> (f32, f32) {
        let out_l = self.b0 * l + self.z1_l;
        self.z1_l = self.b1 * l - self.a1 * out_l + self.z2_l;
        self.z2_l = self.b2 * l - self.a2 * out_l;

        let out_r = self.b0 * r + self.z1_r;
        self.z1_r = self.b1 * r - self.a1 * out_r + self.z2_r;
        self.z2_r = self.b2 * r - self.a2 * out_r;

        // Flush subnormals
        if self.z1_l.abs() < 1e-20 {
            self.z1_l = 0.0;
        }
        if self.z2_l.abs() < 1e-20 {
            self.z2_l = 0.0;
        }
        if self.z1_r.abs() < 1e-20 {
            self.z1_r = 0.0;
        }
        if self.z2_r.abs() < 1e-20 {
            self.z2_r = 0.0;
        }
        (out_l, out_r)
    }

    /// Evaluates exact filter magnitude in dB at `freq_hz` for rendering interactive EQ curves.
    pub fn magnitude_db(&self, sample_rate: f32, freq_hz: f32) -> f32 {
        let w = 2.0 * PI * freq_hz / sample_rate;
        let cos_w = w.cos();
        let cos_2w = (2.0 * w).cos();
        let sin_w = w.sin();
        let sin_2w = (2.0 * w).sin();

        let num_re = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_im = -(self.b1 * sin_w + self.b2 * sin_2w);
        let den_re = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_im = -(self.a1 * sin_w + self.a2 * sin_2w);

        let num_mag2 = num_re * num_re + num_im * num_im;
        let den_mag2 = (den_re * den_re + den_im * den_im).max(1e-18);
        10.0 * (num_mag2 / den_mag2).max(1e-18).log10()
    }
}
