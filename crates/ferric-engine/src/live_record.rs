//! Live-recording transport between the audio callback and the UI (audit H3/H4).
//!
//! The real-time side ([`LiveRecorder`]) never allocates: every recorded
//! frame and MIDI event is pushed into a lock-free ring tagged with the take's
//! sequence number. A background collector thread ([`RecCollector`]) drains
//! the rings into growable buffers, so takes have no length cap. When a take
//! finishes, the callback sends only metadata (sequence number and frame
//! count) and the UI side picks the collected audio up from the collector.

use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::StampedMidi;

/// Which signal a recorded frame belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RecSource {
    /// Hardware line-in / microphone.
    Input,
    /// Post-fader output of a track armed for ResampleTrackOutput.
    Resample,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RecFrame {
    seq: u32,
    src: RecSource,
    l: f32,
    r: f32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RecMidi {
    seq: u32,
    ev: StampedMidi,
}

/// Seconds of audio the ring can hold before the collector must drain it.
pub(crate) const REC_RING_SECONDS: usize = 8;
const REC_MIDI_RING: usize = 16_384;

/// Real-time side. All methods are allocation-free and never block.
pub(crate) struct LiveRecorder {
    audio_tx: Producer<RecFrame>,
    midi_tx: Producer<RecMidi>,
    seq: u32,
    pub(crate) input_frames: usize,
    pub(crate) resample_frames: usize,
    pub(crate) midi_events: usize,
    pub(crate) input_peak: f32,
    lost_frames: Arc<AtomicU64>,
}

impl LiveRecorder {
    /// Start a new take; anything pushed afterwards belongs to it.
    #[inline]
    pub(crate) fn begin(&mut self) {
        self.seq = self.seq.wrapping_add(1);
        self.input_frames = 0;
        self.resample_frames = 0;
        self.midi_events = 0;
        self.input_peak = 0.0;
    }

    #[inline]
    pub(crate) fn seq(&self) -> u32 {
        self.seq
    }

    #[inline]
    pub(crate) fn push_input(&mut self, l: f32, r: f32) {
        let frame = RecFrame {
            seq: self.seq,
            src: RecSource::Input,
            l,
            r,
        };
        if self.audio_tx.push(frame).is_ok() {
            self.input_frames += 1;
            self.input_peak = self.input_peak.max(l.abs()).max(r.abs());
        } else {
            self.lost_frames.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn push_resample(&mut self, l: &[f32], r: &[f32]) {
        for (&a, &b) in l.iter().zip(r) {
            let frame = RecFrame {
                seq: self.seq,
                src: RecSource::Resample,
                l: a,
                r: b,
            };
            if self.audio_tx.push(frame).is_ok() {
                self.resample_frames += 1;
            } else {
                self.lost_frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[inline]
    pub(crate) fn push_midi(&mut self, ev: StampedMidi) {
        if self.midi_tx.push(RecMidi { seq: self.seq, ev }).is_ok() {
            self.midi_events += 1;
        } else {
            self.lost_frames.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
struct Collected {
    audio: HashMap<(u32, RecSource), (Vec<f32>, Vec<f32>)>,
    midi: HashMap<u32, Vec<StampedMidi>>,
    newest_seq: u32,
}

struct Rings {
    audio_rx: Consumer<RecFrame>,
    midi_rx: Consumer<RecMidi>,
}

/// Non-real-time side: drains the rings on a background thread.
pub(crate) struct RecCollector {
    collected: Arc<Mutex<Collected>>,
    rings: Arc<Mutex<Rings>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    lost_frames: Arc<AtomicU64>,
    reported_lost: u64,
}

/// Create a connected recorder / collector pair. `spawn_thread` is false only
/// in tests that drain manually.
pub(crate) fn live_record_pair(sample_rate: u32, spawn_thread: bool) -> (LiveRecorder, RecCollector) {
    let cap = (sample_rate.max(8_000) as usize) * REC_RING_SECONDS;
    let (audio_tx, audio_rx) = RingBuffer::<RecFrame>::new(cap);
    let (midi_tx, midi_rx) = RingBuffer::<RecMidi>::new(REC_MIDI_RING);
    let lost_frames = Arc::new(AtomicU64::new(0));
    let recorder = LiveRecorder {
        audio_tx,
        midi_tx,
        seq: 0,
        input_frames: 0,
        resample_frames: 0,
        midi_events: 0,
        input_peak: 0.0,
        lost_frames: lost_frames.clone(),
    };
    let mut collector = RecCollector {
        collected: Arc::new(Mutex::new(Collected::default())),
        rings: Arc::new(Mutex::new(Rings { audio_rx, midi_rx })),
        stop: Arc::new(AtomicBool::new(false)),
        thread: None,
        lost_frames,
        reported_lost: 0,
    };
    if spawn_thread {
        let collected = collector.collected.clone();
        let rings = collector.rings.clone();
        let stop = collector.stop.clone();
        collector.thread = std::thread::Builder::new()
            .name("ferric-rec-collector".into())
            .spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    drain(&rings, &collected);
                    std::thread::sleep(Duration::from_millis(5));
                }
                drain(&rings, &collected);
            })
            .ok();
    }
    (recorder, collector)
}

fn drain(rings: &Mutex<Rings>, collected: &Mutex<Collected>) {
    let Ok(mut rings) = rings.lock() else { return };
    let n_audio = rings.audio_rx.slots();
    let n_midi = rings.midi_rx.slots();
    if n_audio == 0 && n_midi == 0 {
        return;
    }
    let Ok(mut c) = collected.lock() else { return };
    if n_audio > 0 {
        if let Ok(chunk) = rings.audio_rx.read_chunk(n_audio) {
            let (a, b) = chunk.as_slices();
            let c = &mut *c;
            // Frames arrive in runs of the same take/source; copy a run at a time.
            let frames = a.iter().chain(b);
            let mut run_key: Option<(u32, RecSource)> = None;
            let mut run: (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
            let flush = |key: Option<(u32, RecSource)>, run: &mut (Vec<f32>, Vec<f32>), c: &mut Collected| {
                if let Some(key) = key {
                    if wrapping_newer(key.0, c.newest_seq) {
                        c.newest_seq = key.0;
                    }
                    let e = c.audio.entry(key).or_default();
                    e.0.append(&mut run.0);
                    e.1.append(&mut run.1);
                }
            };
            for f in frames {
                let key = (f.seq, f.src);
                if run_key != Some(key) {
                    flush(run_key, &mut run, c);
                    run_key = Some(key);
                }
                run.0.push(f.l);
                run.1.push(f.r);
            }
            flush(run_key, &mut run, c);
            chunk.commit_all();
        }
    }
    if n_midi > 0 {
        if let Ok(chunk) = rings.midi_rx.read_chunk(n_midi) {
            let (a, b) = chunk.as_slices();
            for m in a.iter().chain(b) {
                if wrapping_newer(m.seq, c.newest_seq) {
                    c.newest_seq = m.seq;
                }
                c.midi.entry(m.seq).or_default().push(m.ev);
            }
            chunk.commit_all();
        }
    }
    // Takes more than a few sequence numbers old were abandoned (e.g. record
    // re-armed before any audio was finished); free their memory.
    let newest = c.newest_seq;
    c.audio.retain(|(s, _), _| newest.wrapping_sub(*s) <= 4);
    c.midi.retain(|s, _| newest.wrapping_sub(*s) <= 4);
}

#[inline]
fn wrapping_newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < u32::MAX / 2
}

impl RecCollector {
    /// Drain the rings now (used by tests and before claiming a take).
    pub(crate) fn drain_now(&self) {
        drain(&self.rings, &self.collected);
    }

    /// Collected frame count for a take.
    pub(crate) fn audio_len(&self, seq: u32, src: RecSource) -> usize {
        self.collected
            .lock()
            .map(|c| c.audio.get(&(seq, src)).map(|v| v.0.len()).unwrap_or(0))
            .unwrap_or(0)
    }

    /// Remove and return a take's audio.
    pub(crate) fn take_audio(&self, seq: u32, src: RecSource) -> (Vec<f32>, Vec<f32>) {
        self.collected
            .lock()
            .ok()
            .and_then(|mut c| c.audio.remove(&(seq, src)))
            .unwrap_or_default()
    }

    pub(crate) fn midi_len(&self, seq: u32) -> usize {
        self.collected
            .lock()
            .map(|c| c.midi.get(&seq).map(Vec::len).unwrap_or(0))
            .unwrap_or(0)
    }

    pub(crate) fn take_midi(&self, seq: u32) -> Vec<StampedMidi> {
        self.collected
            .lock()
            .ok()
            .and_then(|mut c| c.midi.remove(&seq))
            .unwrap_or_default()
    }

    /// Frames/events dropped because the rings were full since the last call.
    pub(crate) fn take_new_lost(&mut self) -> u64 {
        let total = self.lost_frames.load(Ordering::Relaxed);
        let new = total.saturating_sub(self.reported_lost);
        self.reported_lost = total;
        new
    }
}

impl Drop for RecCollector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_hundred_second_take_is_captured_in_full() {
        // 200 s at 8 kHz exceeds the old 180 s cap and the ring (8 s) many times over.
        let sr = 8_000u32;
        let (mut rec, collector) = live_record_pair(sr, false);
        rec.begin();
        let total = sr as usize * 200;
        let block = 256;
        let mut pushed = 0usize;
        while pushed < total {
            let n = block.min(total - pushed);
            for i in 0..n {
                let v = ((pushed + i) % 1000) as f32 / 1000.0;
                rec.push_input(v, -v);
            }
            pushed += n;
            if pushed % (block * 16) == 0 {
                collector.drain_now();
            }
        }
        collector.drain_now();
        assert_eq!(rec.input_frames, total);
        assert_eq!(collector.audio_len(rec.seq(), RecSource::Input), total);
        let (l, r) = collector.take_audio(rec.seq(), RecSource::Input);
        assert_eq!(l.len(), total);
        assert_eq!(r[1234], -l[1234]);
        assert_eq!(l[total - 1], ((total - 1) % 1000) as f32 / 1000.0);
    }

    #[test]
    fn full_ring_counts_lost_frames_without_blocking() {
        let sr = 8_000u32;
        let (mut rec, mut collector) = live_record_pair(sr, false);
        rec.begin();
        let cap = sr as usize * REC_RING_SECONDS;
        let started = std::time::Instant::now();
        for _ in 0..cap + 500 {
            rec.push_input(0.1, 0.1);
        }
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(rec.input_frames, cap);
        assert_eq!(collector.take_new_lost(), 500);
        assert_eq!(collector.take_new_lost(), 0);
    }

    #[test]
    fn takes_are_kept_apart_and_background_thread_collects() {
        let (mut rec, collector) = live_record_pair(8_000, true);
        rec.begin();
        let first = rec.seq();
        rec.push_resample(&[0.5; 100], &[0.25; 100]);
        rec.push_midi(StampedMidi {
            frame_counter: 1,
            beat_estimate: 0.0,
            note: 60,
            velocity: 100,
            is_note_on: true,
        });
        rec.begin();
        rec.push_input(0.1, 0.1);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while collector.audio_len(first, RecSource::Resample) < 100 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(collector.audio_len(first, RecSource::Resample), 100);
        assert_eq!(collector.audio_len(first, RecSource::Input), 0);
        assert_eq!(collector.midi_len(first), 1);
        assert_eq!(collector.take_midi(first).len(), 1);
        while collector.audio_len(rec.seq(), RecSource::Input) < 1 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(collector.take_audio(rec.seq(), RecSource::Input).0, vec![0.1]);
    }
}
