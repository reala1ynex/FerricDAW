use crate::instruments::MidiEvent;
use ferric_model::{ArpMode, TrackArpeggiator};

/// Continuous per-block MIDI Performance Expression evaluated from Piano Roll CC lanes
/// AND live hardware MIDI Pitch-Bend / Mod-Wheel / Expression controllers!
#[derive(Debug, Clone, Copy)]
pub struct MidiExpression {
    /// Pitch-bend offset in semitones (default `0.0`, typically `-2.0 ..= +2.0`).
    pub pitch_bend_semitones: f32,
    /// CC1 Mod Wheel (`0.0 ..= 1.0`) -> adds smooth 5.5 Hz vibrato LFO.
    pub mod_wheel: f32,
    /// CC11 Expression (`0.0 ..= 1.0`, default `1.0`) -> continuous amplitude scaling.
    pub expression: f32,
    /// CC74 Filter Brightness (`-1.0 ..= +1.0`, default `0.0`) -> ±2 octave filter offset.
    pub brightness_offset: f32,
}

impl Default for MidiExpression {
    fn default() -> Self {
        Self {
            pitch_bend_semitones: 0.0,
            mod_wheel: 0.0,
            expression: 1.0,
            brightness_offset: 0.0,
        }
    }
}

/// Zero-allocation Real-Time Track Arpeggiator / Note-Repeat engine.
/// Works during timeline playback and while transport is stopped (live USB MIDI jamming).
#[derive(Debug, Clone)]
pub struct ArpRuntime {
    held_vel: [u8; 128],
    active_arp_note: Option<u8>,
    gate_off_beat: f64,
    last_step_index: i64,
    free_beat_clock: f64,
    ordered_pool: [u8; 64],
}

impl Default for ArpRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ArpRuntime {
    pub fn new() -> Self {
        Self {
            held_vel: [0; 128],
            active_arp_note: None,
            gate_off_beat: 0.0,
            last_step_index: -1,
            free_beat_clock: 0.0,
            ordered_pool: [0; 64],
        }
    }

    pub fn reset(&mut self) {
        self.held_vel = [0; 128];
        self.active_arp_note = None;
        self.last_step_index = -1;
    }

    pub fn process_midi_block(
        &mut self,
        cfg: &TrackArpeggiator,
        frames: usize,
        sample_rate: f32,
        bpm: f64,
        is_playing: bool,
        block_start_beat: f64,
        in_out_events: &mut Vec<MidiEvent>,
    ) {
        if !cfg.enabled {
            if let Some(prev) = self.active_arp_note.take() {
                in_out_events.push(MidiEvent {
                    sample_offset: 0,
                    note: prev,
                    velocity: 0,
                    is_note_on: false,
                });
            }
            self.held_vel = [0; 128];
            return;
        }

        // Absorb incoming chord Note-On / Note-Off events into held note table
        for ev in in_out_events.drain(..) {
            let idx = (ev.note as usize) & 127;
            if ev.is_note_on && ev.velocity > 0 {
                self.held_vel[idx] = ev.velocity;
            } else {
                self.held_vel[idx] = 0;
            }
        }

        // Build expanded pitch list across octaves
        let octs = cfg.octaves.clamp(1, 3);
        let mut pool_len = 0usize;
        let mut max_vel = 100u8;
        for o in 0..octs {
            for pitch in 0..128u8 {
                let v = self.held_vel[pitch as usize];
                if v > 0 {
                    max_vel = max_vel.max(v);
                    let shifted = pitch.saturating_add(o * 12);
                    if shifted <= 127 && pool_len < self.ordered_pool.len() {
                        self.ordered_pool[pool_len] = shifted;
                        pool_len += 1;
                    }
                }
            }
        }

        let beats_per_sample = (bpm.max(20.0) / 60.0) / sample_rate.max(8_000.0) as f64;
        let start_b = if is_playing {
            block_start_beat
        } else {
            let cur = self.free_beat_clock;
            self.free_beat_clock += beats_per_sample * frames as f64;
            cur
        };

        if pool_len == 0 {
            if let Some(prev) = self.active_arp_note.take() {
                in_out_events.push(MidiEvent {
                    sample_offset: 0,
                    note: prev,
                    velocity: 0,
                    is_note_on: false,
                });
            }
            self.last_step_index = -1;
            return;
        }

        let rate = (cfg.rate_beats as f64).clamp(0.0625, 4.0);
        let gate_len = rate * (cfg.gate.clamp(0.1, 0.98) as f64);

        for i in 0..frames {
            let beat = start_b + i as f64 * beats_per_sample;

            if let Some(cur_note) = self.active_arp_note {
                if beat >= self.gate_off_beat {
                    in_out_events.push(MidiEvent {
                        sample_offset: i,
                        note: cur_note,
                        velocity: 0,
                        is_note_on: false,
                    });
                    self.active_arp_note = None;
                }
            }

            let step_idx = (beat / rate).floor() as i64;
            if step_idx != self.last_step_index {
                self.last_step_index = step_idx;
                if let Some(prev) = self.active_arp_note.take() {
                    in_out_events.push(MidiEvent {
                        sample_offset: i,
                        note: prev,
                        velocity: 0,
                        is_note_on: false,
                    });
                }

                if cfg.mode == ArpMode::ChordGate {
                    // Rhythmic polyphonic chord trance-gate!
                    for k in 0..pool_len {
                        let p = self.ordered_pool[k];
                        in_out_events.push(MidiEvent {
                            sample_offset: i,
                            note: p,
                            velocity: max_vel,
                            is_note_on: true,
                        });
                    }
                    self.active_arp_note = Some(self.ordered_pool[0]);
                    self.gate_off_beat = beat + gate_len;
                } else {
                    let s = step_idx.rem_euclid(10_000) as usize;
                    let pick = match cfg.mode {
                        ArpMode::Up => self.ordered_pool[s % pool_len],
                        ArpMode::Down => self.ordered_pool[(pool_len - 1) - (s % pool_len)],
                        ArpMode::UpDown => {
                            if pool_len == 1 {
                                self.ordered_pool[0]
                            } else {
                                let cycle = pool_len * 2 - 2;
                                let pos = s % cycle;
                                if pos < pool_len {
                                    self.ordered_pool[pos]
                                } else {
                                    self.ordered_pool[cycle - pos]
                                }
                            }
                        }
                        ArpMode::ChordGate => self.ordered_pool[0],
                    };
                    in_out_events.push(MidiEvent {
                        sample_offset: i,
                        note: pick,
                        velocity: max_vel,
                        is_note_on: true,
                    });
                    self.active_arp_note = Some(pick);
                    self.gate_off_beat = beat + gate_len;
                }
            }
        }
    }
}

const FLUX_RING_LEN: usize = 512; // ~5.12 seconds @ 100 Hz envelope rate

/// Zero-allocation Real-Time Audio Onset-Flux Autocorrelation Tempo Detector!
/// Listens to hardware Line-In and locks exact BPM.
pub struct OnsetBpmDetector {
    flux_ring: [f32; FLUX_RING_LEN],
    flux_pos: usize,
    flux_filled: usize,
    hop_accum_sq: f32,
    hop_count: usize,
    hop_target: usize,
    prev_rms: f32,
    hops_since_eval: usize,
    pub detected_bpm: Option<f32>,
    pub confidence: f32,
}

impl OnsetBpmDetector {
    pub fn new(sample_rate: f32) -> Self {
        let hop = (sample_rate.max(8_000.0) / 100.0).round() as usize;
        Self {
            flux_ring: [0.0; FLUX_RING_LEN],
            flux_pos: 0,
            flux_filled: 0,
            hop_accum_sq: 0.0,
            hop_count: 0,
            hop_target: hop.max(64),
            prev_rms: 0.0,
            hops_since_eval: 0,
            detected_bpm: None,
            confidence: 0.0,
        }
    }

    pub fn push_block(&mut self, left: &[f32], right: &[f32]) {
        let n = left.len().min(right.len());
        for i in 0..n {
            let m = 0.5 * (left[i] + right[i]);
            self.hop_accum_sq += m * m;
            self.hop_count += 1;
            if self.hop_count >= self.hop_target {
                let rms = (self.hop_accum_sq / self.hop_count as f32).sqrt();
                self.hop_accum_sq = 0.0;
                self.hop_count = 0;

                // Half-wave rectified onset flux emphasizes drum/synth transients!
                let flux = (rms - self.prev_rms).max(0.0);
                self.prev_rms = rms * 0.85 + self.prev_rms * 0.15;

                self.flux_ring[self.flux_pos] = flux;
                self.flux_pos = (self.flux_pos + 1) & (FLUX_RING_LEN - 1);
                self.flux_filled = (self.flux_filled + 1).min(FLUX_RING_LEN);
                self.hops_since_eval += 1;

                if self.hops_since_eval >= 50 && self.flux_filled >= 300 {
                    self.hops_since_eval = 0;
                    self.estimate_bpm();
                }
            }
        }
    }

    fn estimate_bpm(&mut self) {
        let win = 320usize; // 3.2 seconds @ 100 Hz
        let mask = FLUX_RING_LEN - 1;
        let start = (self.flux_pos + FLUX_RING_LEN - win) & mask;

        let mut energy = 0.0_f32;
        for i in 0..win {
            let v = self.flux_ring[(start + i) & mask];
            energy += v * v;
        }
        if energy < 1e-6 {
            self.detected_bpm = None;
            self.confidence = 0.0;
            return;
        }

        // Lags 32..=92 at 100 Hz correspond to ~65.2 BPM ..= 187.5 BPM
        let min_lag = 32usize;
        let max_lag = 92usize;
        let mut acf = [0.0_f32; 100];

        for lag in min_lag..=max_lag {
            let mut sum = 0.0_f32;
            for i in 0..(win - max_lag) {
                let a = self.flux_ring[(start + i) & mask];
                let b = self.flux_ring[(start + i + lag) & mask];
                sum += a * b;
            }
            acf[lag] = sum / energy;
        }

        let mut best_lag = min_lag;
        let mut best_val = 0.0_f32;
        for lag in (min_lag + 1)..max_lag {
            if acf[lag] > best_val {
                best_val = acf[lag];
                best_lag = lag;
            }
        }

        if best_val < 0.12 {
            self.confidence = best_val;
            return;
        }

        // Parabolic peak refinement for sub-0.2 BPM accuracy
        let y0 = acf[best_lag - 1];
        let y1 = acf[best_lag];
        let y2 = acf[best_lag + 1];
        let denom = 2.0 * (2.0 * y1 - y2 - y0);
        let refined_lag = if denom.abs() > 1e-6 {
            best_lag as f32 + (y2 - y0) / denom
        } else {
            best_lag as f32
        };

        let env_hz = 100.0_f32;
        let raw_bpm = (60.0 * env_hz) / refined_lag.max(1.0);
        let rounded_half = (raw_bpm * 2.0).round() * 0.5;
        self.detected_bpm = Some(rounded_half.clamp(60.0, 200.0));
        self.confidence = best_val.clamp(0.0, 1.0);
    }
}
