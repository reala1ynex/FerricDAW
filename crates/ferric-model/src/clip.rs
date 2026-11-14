use crate::fx::FxSlot;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub type ClipId = Uuid;
pub type TakeId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MidiNote {
    pub id: Uuid,
    pub pitch: u8,
    pub velocity: u8, // 1..=127
    pub start_beat: f64,
    pub length_beats: f64,
    pub channel: u8,
}

impl MidiNote {
    pub fn new(pitch: u8, velocity: u8, start_beat: f64, length_beats: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            pitch: pitch.min(127),
            velocity: velocity.clamp(1, 127),
            start_beat: start_beat.max(0.0),
            length_beats: length_beats.max(0.03125),
            channel: 0,
        }
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.length_beats
    }
}

/// Shared, cheaply-cloneable planar stereo audio buffer so Undo/Redo snapshots
/// and Real-Time audio engine clones never copy megabytes of PCM data!
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub left: Arc<Vec<f32>>,
    pub right: Arc<Vec<f32>>,
    /// Precomputed min/max overview peaks (256 frames/bucket) for instant 120 FPS waveform drawing.
    pub peaks: Arc<Vec<(f32, f32)>>,
    pub source_path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct SerializedAudioMeta {
    sample_rate: u32,
    source_path: Option<PathBuf>,
    /// Inline compact PCM only when there is no external file or for short generated clips (< 6s).
    inline_left: Option<Vec<f32>>,
    inline_right: Option<Vec<f32>>,
}

impl Serialize for AudioBuffer {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let keep_inline = self.source_path.is_none() && self.left.len() <= 48_000 * 8;
        let meta = SerializedAudioMeta {
            sample_rate: self.sample_rate,
            source_path: self.source_path.clone(),
            inline_left: if keep_inline {
                Some((*self.left).clone())
            } else {
                None
            },
            inline_right: if keep_inline {
                Some((*self.right).clone())
            } else {
                None
            },
        };
        meta.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AudioBuffer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let meta = SerializedAudioMeta::deserialize(deserializer)?;
        let left = meta.inline_left.unwrap_or_default();
        let right = meta.inline_right.unwrap_or_else(|| left.clone());
        Ok(AudioBuffer::from_stereo(
            meta.sample_rate.max(8_000),
            left,
            right,
            meta.source_path,
        ))
    }
}

impl AudioBuffer {
    pub const PEAK_BUCKET_SIZE: usize = 256;

    pub fn from_stereo(
        sample_rate: u32,
        left: Vec<f32>,
        mut right: Vec<f32>,
        source_path: Option<PathBuf>,
    ) -> Self {
        if right.len() < left.len() {
            right.resize(left.len(), 0.0);
        }
        let peaks = Self::compute_peaks(&left, &right);
        Self {
            sample_rate,
            left: Arc::new(left),
            right: Arc::new(right),
            peaks: Arc::new(peaks),
            source_path,
        }
    }

    pub fn compute_peaks(left: &[f32], right: &[f32]) -> Vec<(f32, f32)> {
        let len = left.len().min(right.len());
        if len == 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(len.div_ceil(Self::PEAK_BUCKET_SIZE));
        let mut i = 0;
        while i < len {
            let end = (i + Self::PEAK_BUCKET_SIZE).min(len);
            let mut min_v = 0.0_f32;
            let mut max_v = 0.0_f32;
            for idx in i..end {
                let mono = 0.5 * (left[idx] + right[idx]);
                if mono < min_v {
                    min_v = mono;
                }
                if mono > max_v {
                    max_v = mono;
                }
            }
            out.push((min_v, max_v));
            i = end;
        }
        out
    }

    pub fn len_frames(&self) -> usize {
        self.left.len().min(self.right.len())
    }

    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.len_frames() as f64 / self.sample_rate as f64
        }
    }

    /// Linear-interpolated stereo sample fetch for pitch-shifted / varispeed clip playback.
    #[inline]
    pub fn sample_interpolated(&self, pos_frames: f64) -> (f32, f32) {
        let len = self.len_frames();
        if len == 0 || pos_frames < 0.0 {
            return (0.0, 0.0);
        }
        let idx0 = pos_frames.floor() as usize;
        if idx0 >= len {
            return (0.0, 0.0);
        }
        let idx1 = (idx0 + 1).min(len - 1);
        let frac = (pos_frames - idx0 as f64) as f32;
        let l = self.left[idx0] + (self.left[idx1] - self.left[idx0]) * frac;
        let r = self.right[idx0] + (self.right[idx1] - self.right[idx0]) * frac;
        (l, r)
    }

    pub fn normalized(&self, target_peak_db: f32) -> Self {
        let target_lin = 10.0_f32.powf(target_peak_db / 20.0);
        let max_abs = self
            .left
            .iter()
            .chain(self.right.iter())
            .fold(0.0_f32, |acc, &x| acc.max(x.abs()));
        let scale = if max_abs > 1e-6 {
            target_lin / max_abs
        } else {
            1.0
        };
        let left: Vec<f32> = self.left.iter().map(|&s| s * scale).collect();
        let right: Vec<f32> = self.right.iter().map(|&s| s * scale).collect();
        Self::from_stereo(self.sample_rate, left, right, self.source_path.clone())
    }

    pub fn reversed(&self) -> Self {
        let mut left = (*self.left).clone();
        let mut right = (*self.right).clone();
        left.reverse();
        right.reverse();
        Self::from_stereo(self.sample_rate, left, right, self.source_path.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TakeContent {
    Midi { notes: Vec<MidiNote> },
    Audio { buffer: AudioBuffer },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Take {
    pub id: TakeId,
    pub name: String,
    pub rating: u8, // 0..=5 stars for quick vocal/guitar session grading
    pub content: TakeContent,
}

impl Take {
    pub fn new_midi(name: impl Into<String>, notes: Vec<MidiNote>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            rating: 0,
            content: TakeContent::Midi { notes },
        }
    }

    pub fn new_audio(name: impl Into<String>, buffer: AudioBuffer) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            rating: 0,
            content: TakeContent::Audio { buffer },
        }
    }
}

/// REAPER-style Swipe-Comping segment: maps a clip-local beat interval `[start_beat, end_beat)`
/// to a specific `take_index`, automatically crossfaded at boundaries by the audio engine!
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CompSegment {
    pub start_beat: f64,
    pub end_beat: f64,
    pub take_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FadeShape {
    Linear,
    EqualPower,
    FastStart,
    SlowStart,
}

impl FadeShape {
    #[inline]
    pub fn eval(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            FadeShape::Linear => t,
            FadeShape::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
            FadeShape::FastStart => t.sqrt(),
            FadeShape::SlowStart => t * t,
        }
    }
}

/// A universal non-destructive timeline item ("Media Item" in REAPER).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineClip {
    pub id: ClipId,
    pub name: String,
    pub color_override: Option<[u8; 3]>,
    pub start_beat: f64,
    pub length_beats: f64,
    /// Slip-edit offset into the underlying take content (in beats).
    pub slip_offset_beats: f64,
    pub loop_enabled: bool,
    pub loop_length_beats: f64,
    pub muted: bool,
    pub gain_db: f32,
    pub pitch_semitones: f32,
    pub fade_in_beats: f64,
    pub fade_out_beats: f64,
    pub fade_shape: FadeShape,
    /// Multi-take container (Take 1, Take 2, Take 3...).
    pub takes: Vec<Take>,
    pub active_take: usize,
    /// Optional swipe-comping map; when non-empty, overrides `active_take` per beat region!
    pub comp_segments: Vec<CompSegment>,
    pub show_take_lanes: bool,
    /// REAPER signature feature: Per-Clip FX chain!
    pub clip_fx: Vec<FxSlot>,
}

impl TimelineClip {
    pub fn new_midi(
        name: impl Into<String>,
        start_beat: f64,
        length_beats: f64,
        notes: Vec<MidiNote>,
    ) -> Self {
        let name = name.into();
        Self {
            id: Uuid::new_v4(),
            name: name.clone(),
            color_override: None,
            start_beat,
            length_beats: length_beats.max(0.25),
            slip_offset_beats: 0.0,
            loop_enabled: true,
            loop_length_beats: length_beats.max(0.25),
            muted: false,
            gain_db: 0.0,
            pitch_semitones: 0.0,
            fade_in_beats: 0.0,
            fade_out_beats: 0.0,
            fade_shape: FadeShape::EqualPower,
            takes: vec![Take::new_midi("Take 1", notes)],
            active_take: 0,
            comp_segments: Vec::new(),
            show_take_lanes: false,
            clip_fx: Vec::new(),
        }
    }

    pub fn new_audio(
        name: impl Into<String>,
        start_beat: f64,
        length_beats: f64,
        buffer: AudioBuffer,
    ) -> Self {
        let name = name.into();
        Self {
            id: Uuid::new_v4(),
            name,
            color_override: None,
            start_beat,
            length_beats: length_beats.max(0.25),
            slip_offset_beats: 0.0,
            loop_enabled: false,
            loop_length_beats: length_beats.max(0.25),
            muted: false,
            gain_db: 0.0,
            pitch_semitones: 0.0,
            fade_in_beats: 0.04,
            fade_out_beats: 0.04,
            fade_shape: FadeShape::EqualPower,
            takes: vec![Take::new_audio("Take 1", buffer)],
            active_take: 0,
            comp_segments: Vec::new(),
            show_take_lanes: false,
            clip_fx: Vec::new(),
        }
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.length_beats
    }

    pub fn is_midi(&self) -> bool {
        self.takes
            .get(self.active_take)
            .map(|t| matches!(t.content, TakeContent::Midi { .. }))
            .unwrap_or(false)
    }

    /// Resolves which `take_index` is active at `local_beat` inside the clip.
    #[inline]
    pub fn take_at_local_beat(&self, local_beat: f64) -> usize {
        if !self.comp_segments.is_empty() {
            for seg in &self.comp_segments {
                if local_beat >= seg.start_beat && local_beat < seg.end_beat {
                    return seg.take_index.min(self.takes.len().saturating_sub(1));
                }
            }
        }
        self.active_take.min(self.takes.len().saturating_sub(1))
    }

    /// Evaluates clip gain * fade-in * fade-out envelope at `local_beat` (`0..length_beats`).
    #[inline]
    pub fn gain_envelope_at(&self, local_beat: f64) -> f32 {
        if local_beat < 0.0 || local_beat > self.length_beats {
            return 0.0;
        }
        let mut env = 10.0_f32.powf(self.gain_db / 20.0);
        if self.fade_in_beats > 1e-5 && local_beat < self.fade_in_beats {
            env *= self.fade_shape.eval((local_beat / self.fade_in_beats) as f32);
        }
        let rem = self.length_beats - local_beat;
        if self.fade_out_beats > 1e-5 && rem < self.fade_out_beats {
            env *= self.fade_shape.eval((rem / self.fade_out_beats) as f32);
        }
        env
    }

    /// Maps a timeline beat inside this clip to the wrapped internal source beat.
    #[inline]
    pub fn content_beat(&self, timeline_beat: f64) -> Option<(f64, f64)> {
        let local = timeline_beat - self.start_beat;
        if local < 0.0 || local >= self.length_beats {
            return None;
        }
        let raw = local + self.slip_offset_beats;
        let inner = if self.loop_enabled && self.loop_length_beats > 0.03125 {
            raw.rem_euclid(self.loop_length_beats)
        } else {
            raw
        };
        Some((local, inner))
    }

    /// REAPER-style Swipe Comping: assigns `[start_local, end_local]` to `take_index`,
    /// cleanly splitting/merging existing comp segments.
    pub fn set_comp_region(&mut self, start_local: f64, end_local: f64, take_index: usize) {
        if self.takes.is_empty() || end_local <= start_local {
            return;
        }
        let take_index = take_index.min(self.takes.len() - 1);
        let s = start_local.clamp(0.0, self.length_beats);
        let e = end_local.clamp(0.0, self.length_beats);
        if e - s < 1e-4 {
            return;
        }

        if self.comp_segments.is_empty() {
            self.comp_segments.push(CompSegment {
                start_beat: 0.0,
                end_beat: self.length_beats,
                take_index: self.active_take.min(self.takes.len() - 1),
            });
        }

        let mut next = Vec::with_capacity(self.comp_segments.len() + 2);
        for seg in self.comp_segments.drain(..) {
            if seg.end_beat <= s || seg.start_beat >= e {
                next.push(seg);
            } else {
                if seg.start_beat < s {
                    next.push(CompSegment {
                        start_beat: seg.start_beat,
                        end_beat: s,
                        take_index: seg.take_index,
                    });
                }
                if seg.end_beat > e {
                    next.push(CompSegment {
                        start_beat: e,
                        end_beat: seg.end_beat,
                        take_index: seg.take_index,
                    });
                }
            }
        }
        next.push(CompSegment {
            start_beat: s,
            end_beat: e,
            take_index,
        });
        next.sort_by(|a, b| a.start_beat.partial_cmp(&b.start_beat).unwrap());

        // Coalesce adjacent segments with identical take_index
        let mut merged: Vec<CompSegment> = Vec::with_capacity(next.len());
        for seg in next {
            if let Some(last) = merged.last_mut() {
                if last.take_index == seg.take_index && (last.end_beat - seg.start_beat).abs() < 1e-4
                {
                    last.end_beat = seg.end_beat;
                    continue;
                }
            }
            merged.push(seg);
        }
        self.comp_segments = merged;
    }
}

/// Ableton / Bitwig Hybrid Session Clip Launcher cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSlot {
    pub id: Uuid,
    pub name: String,
    pub length_beats: f64,
    pub gain_db: f32,
    pub content: TakeContent,
}

impl SessionSlot {
    pub fn new_midi(name: impl Into<String>, length_beats: f64, notes: Vec<MidiNote>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            length_beats: length_beats.max(0.25),
            gain_db: 0.0,
            content: TakeContent::Midi { notes },
        }
    }

    pub fn new_audio(name: impl Into<String>, length_beats: f64, buffer: AudioBuffer) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            length_beats: length_beats.max(0.25),
            gain_db: 0.0,
            content: TakeContent::Audio { buffer },
        }
    }

    pub fn to_timeline_clip(&self, start_beat: f64, duration_beats: f64) -> TimelineClip {
        match &self.content {
            TakeContent::Midi { notes } => {
                let mut c = TimelineClip::new_midi(
                    self.name.clone(),
                    start_beat,
                    duration_beats,
                    notes.clone(),
                );
                c.loop_enabled = true;
                c.loop_length_beats = self.length_beats;
                c.gain_db = self.gain_db;
                c
            }
            TakeContent::Audio { buffer } => {
                let mut c = TimelineClip::new_audio(
                    self.name.clone(),
                    start_beat,
                    duration_beats,
                    buffer.clone(),
                );
                c.loop_enabled = true;
                c.loop_length_beats = self.length_beats;
                c.gain_db = self.gain_db;
                c
            }
        }
    }
}
