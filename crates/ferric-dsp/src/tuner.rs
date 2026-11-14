/// Real-time zero-allocation YIN-inspired Chromatic Strobe Tuner.
/// Essential for home guitar, bass, vocal, and synth calibration.

const TUNER_BUF_LEN: usize = 4096;
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Debug, Clone, Copy, Default)]
pub struct TunerReading {
    pub active: bool,
    pub freq_hz: f32,
    pub midi_note: u8,
    pub cents: f32,
    pub confidence: f32,
}

impl TunerReading {
    pub fn note_label(&self) -> String {
        if !self.active {
            return "--".to_string();
        }
        let name = NOTE_NAMES[(self.midi_note as usize) % 12];
        let octave = (self.midi_note as i32 / 12) - 1;
        format!("{}{}", name, octave)
    }
}

pub struct ChromaticTuner {
    ring: [f32; TUNER_BUF_LEN],
    diff: [f32; TUNER_BUF_LEN / 2],
    write_pos: usize,
    samples_since_eval: usize,
    sample_rate: f32,
    pub ref_a4_hz: f32,
    pub last_reading: TunerReading,
}

impl ChromaticTuner {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            ring: [0.0; TUNER_BUF_LEN],
            diff: [0.0; TUNER_BUF_LEN / 2],
            write_pos: 0,
            samples_since_eval: 0,
            sample_rate: sample_rate.max(8_000.0),
            ref_a4_hz: 440.0,
            last_reading: TunerReading::default(),
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr.max(8_000.0);
    }

    #[inline]
    pub fn push_block(&mut self, left: &[f32], right: &[f32]) {
        let len = left.len().min(right.len());
        for i in 0..len {
            self.ring[self.write_pos] = 0.5 * (left[i] + right[i]);
            self.write_pos = (self.write_pos + 1) & (TUNER_BUF_LEN - 1);
        }
        self.samples_since_eval += len;
        if self.samples_since_eval >= 1024 {
            self.samples_since_eval = 0;
            self.evaluate_pitch();
        }
    }

    fn evaluate_pitch(&mut self) {
        let half = TUNER_BUF_LEN / 2;
        // Check signal RMS first
        let mut energy = 0.0_f32;
        for i in 0..half {
            let idx = (self.write_pos + TUNER_BUF_LEN - half + i) & (TUNER_BUF_LEN - 1);
            let s = self.ring[idx];
            energy += s * s;
        }
        let rms = (energy / half as f32).sqrt();
        if rms < 0.004 {
            self.last_reading.active = false;
            self.last_reading.confidence = 0.0;
            return;
        }

        let min_tau = (self.sample_rate / 1200.0).floor() as usize; // up to ~1200 Hz
        let max_tau = ((self.sample_rate / 36.0).ceil() as usize).min(half - 2); // down to Low D1/B0 (~36 Hz)

        self.diff[0] = 1.0;
        let mut running_sum = 0.0_f32;
        let start = (self.write_pos + TUNER_BUF_LEN - 2 * half) & (TUNER_BUF_LEN - 1);

        // YIN Cumulative Mean Normalized Difference Function (strided by 2 for tiny CPU < 0.05%)
        for tau in 1..=max_tau {
            let mut sum = 0.0_f32;
            let mut j = 0;
            while j < half {
                let a = self.ring[(start + j) & (TUNER_BUF_LEN - 1)];
                let b = self.ring[(start + j + tau) & (TUNER_BUF_LEN - 1)];
                let d = a - b;
                sum += d * d;
                j += 2;
            }
            running_sum += sum;
            self.diff[tau] = if running_sum > 1e-12 {
                sum * (tau as f32) / running_sum
            } else {
                1.0
            };
        }

        // Find first deep minimum below threshold
        let threshold = 0.14_f32;
        let mut best_tau = None;
        let mut tau = min_tau.max(2);
        while tau < max_tau {
            if self.diff[tau] < threshold {
                while tau + 1 < max_tau && self.diff[tau + 1] < self.diff[tau] {
                    tau += 1;
                }
                best_tau = Some(tau);
                break;
            }
            tau += 1;
        }

        // Fallback to global minimum if reasonably periodic
        if best_tau.is_none() {
            let mut min_val = 1.0_f32;
            let mut min_idx = 0usize;
            for t in min_tau.max(2)..max_tau {
                if self.diff[t] < min_val {
                    min_val = self.diff[t];
                    min_idx = t;
                }
            }
            if min_val < 0.32 {
                best_tau = Some(min_idx);
            }
        }

        let Some(t0) = best_tau else {
            self.last_reading.active = false;
            return;
        };

        // Parabolic interpolation for sub-cent accuracy
        let y0 = self.diff[t0 - 1];
        let y1 = self.diff[t0];
        let y2 = self.diff[t0 + 1];
        let denom = 2.0 * (2.0 * y1 - y2 - y0);
        let refined_tau = if denom.abs() > 1e-9 {
            t0 as f32 + (y2 - y0) / denom
        } else {
            t0 as f32
        };

        let freq = self.sample_rate / refined_tau.max(1.0);
        let exact_midi = 69.0 + 12.0 * (freq / self.ref_a4_hz.max(400.0)).log2();
        let nearest = exact_midi.round();
        let cents = (exact_midi - nearest) * 100.0;

        self.last_reading = TunerReading {
            active: true,
            freq_hz: freq,
            midi_note: nearest.clamp(0.0, 127.0) as u8,
            cents: cents.clamp(-50.0, 50.0),
            confidence: (1.0 - y1).clamp(0.0, 1.0),
        };
    }
}
