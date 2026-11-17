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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiCcTarget {
    PitchBend,
    ModWheel,
    Expression,
    FilterBrightness,
    SustainPedal,
}

impl MidiCcTarget {
    pub const ALL: &'static [MidiCcTarget] = &[
        MidiCcTarget::PitchBend,
        MidiCcTarget::ModWheel,
        MidiCcTarget::Expression,
        MidiCcTarget::FilterBrightness,
        MidiCcTarget::SustainPedal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MidiCcTarget::PitchBend => "Pitch Bend",
            MidiCcTarget::ModWheel => "CC1 Mod Wheel",
            MidiCcTarget::Expression => "CC11 Expression",
            MidiCcTarget::FilterBrightness => "CC74 Brightness",
            MidiCcTarget::SustainPedal => "CC64 Sustain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MidiCcPoint {
    pub beat: f64,
    pub target: MidiCcTarget,
    /// Normalized `0.0 ..= 1.0` (`0.5` = center for PitchBend).
    pub value: f32,
}

/// Evaluates piecewise-linear MIDI CC / Pitch-Bend curve at `beat` inside a MIDI take.
#[inline]
pub fn eval_midi_cc_curve(points: &[MidiCcPoint], target: MidiCcTarget, beat: f64) -> Option<f32> {
    let mut prev: Option<&MidiCcPoint> = None;
    let mut next: Option<&MidiCcPoint> = None;
    for p in points.iter().filter(|p| p.target == target) {
        if p.beat <= beat {
            prev = Some(p);
        } else {
            next = Some(p);
            break;
        }
    }
    match (prev, next) {
        (None, None) => None,
        (Some(a), None) => Some(a.value),
        (None, Some(b)) => Some(b.value),
        (Some(a), Some(b)) => {
            let span = (b.beat - a.beat).max(1e-6);
            let t = ((beat - a.beat) / span).clamp(0.0, 1.0) as f32;
            Some(a.value + (b.value - a.value) * t)
        }
    }
}

/// Shared, cheaply-cloneable planar stereo audio buffer so Undo/Redo snapshots
/// and Real-Time audio engine clones never copy megabytes of PCM data!
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub left: Arc<Vec<f32>>,
    pub right: Arc<Vec<f32>>,
    pub peaks: Arc<Vec<(f32, f32)>>,
    pub source_path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct SerializedAudioMeta {
    sample_rate: u32,
    source_path: Option<PathBuf>,
    inline_left: Option<Vec<f32>>,
    inline_right: Option<Vec<f32>>,
}

impl Serialize for AudioBuffer {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let keep_inline = self.source_path.is_none() && self.left.len() <= 48_000 * 10;
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

    /// Energy-flux onset / transient detector for 1-click Drum Slicing & Warp Marker creation!
    pub fn detect_transients(&self, max_slices: usize) -> Vec<usize> {
        let len = self.len_frames();
        if len == 0 || max_slices == 0 {
            return Vec::new();
        }
        let hop = (self.sample_rate as usize / 100).max(128); // ~10ms hop
        let min_gap = (self.sample_rate as usize / 8).max(hop * 4); // >= 125ms between slices

        let mut slices = vec![0usize];
        let mut slow_env = 0.001_f32;
        let mut last_onset = 0usize;

        let mut pos = hop;
        while pos + hop < len && slices.len() < max_slices {
            let mut rms = 0.0_f32;
            for i in pos..(pos + hop) {
                let m = 0.5 * (self.left[i] + self.right[i]);
                rms += m * m;
            }
            rms = (rms / hop as f32).sqrt();
            let ratio = rms / slow_env.max(0.003);
            if rms > 0.025 && ratio > 2.35 && pos.saturating_sub(last_onset) >= min_gap {
                slices.push(pos);
                last_onset = pos;
                slow_env = rms;
            } else {
                slow_env = slow_env * 0.88 + rms * 0.12;
            }
            pos += hop;
        }
        slices
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
        // New content: no longer backed by the original file, so the next
        // save must write it out instead of keeping the stale reference.
        Self::from_stereo(self.sample_rate, left, right, None)
    }

    pub fn reversed(&self) -> Self {
        let mut left = (*self.left).clone();
        let mut right = (*self.right).clone();
        left.reverse();
        right.reverse();
        Self::from_stereo(self.sample_rate, left, right, None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TakeContent {
    Midi {
        notes: Vec<MidiNote>,
        #[serde(default)]
        cc_points: Vec<MidiCcPoint>,
    },
    Audio {
        buffer: AudioBuffer,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Take {
    pub id: TakeId,
    pub name: String,
    pub rating: u8,
    pub content: TakeContent,
}

impl Take {
    pub fn new_midi(name: impl Into<String>, notes: Vec<MidiNote>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            rating: 0,
            content: TakeContent::Midi {
                notes,
                cc_points: Vec::new(),
            },
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

/// Audio time-stretching & pitch-warping algorithm per clip!
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeStretchMode {
    /// Independent Tempo & Pitch via Dual-Window WSOLA (no chipmunk effect!).
    #[default]
    ElastiqueWsola,
    /// Analog Tape Varispeed (pitch and playback speed coupled).
    TapeVarispeed,
}

impl TimeStretchMode {
    pub fn label(self) -> &'static str {
        match self {
            TimeStretchMode::ElastiqueWsola => "WSOLA Warp (Keep Pitch)",
            TimeStretchMode::TapeVarispeed => "Tape Varispeed",
        }
    }
}

/// Non-destructive REAPER/Ableton Stretch/Warp Marker anchoring a clip-local beat to a source audio second!
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StretchMarker {
    pub id: Uuid,
    pub clip_beat: f64,
    pub source_sec: f64,
}

/// Melodyne / VariAudio / ReaTune style Visual Pitch Blob detected inside a monophonic Audio Clip!
/// Lets users drag individual sung/played notes up/down on a pitch grid, flatten vibrato/drift,
/// OR convert any hummed/guitar audio take directly into a playable MIDI clip in 1 click!
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotePitchBlob {
    pub id: Uuid,
    pub start_beat: f64,
    pub end_beat: f64,
    /// Original detected MIDI pitch center (e.g. `64.18` = slightly sharp E4).
    pub detected_midi: f32,
    /// User-edited target MIDI note (`0..=127`, defaults to `detected_midi.round()`).
    pub target_midi: f32,
    /// Fine pitch offset in cents (`-100 ..= +100`).
    pub fine_cents: f32,
    /// `0.0` = preserve 100% original drift/vibrato, `1.0` = hard pitch-center lock!
    pub drift_flatten: f32,
    /// Normalized RMS amplitude (`0.0 ..= 1.0`) for visual blob thickness AND MIDI velocity!
    pub rms_norm: f32,
    /// Sub-blob pitch contour offsets in semitones relative to `detected_midi` (up to 16 points) for drawing the exact vocal pitch curve!
    #[serde(default)]
    pub contour_semitones: Vec<f32>,
}

impl NotePitchBlob {
    /// Net pitch shift in semitones applied by the WSOLA audio engine across `[start_beat .. end_beat]`.
    #[inline]
    pub fn net_pitch_shift_semitones(&self) -> f32 {
        let center_shift = (self.target_midi + self.fine_cents * 0.01) - self.detected_midi;
        center_shift.clamp(-24.0, 24.0)
    }

    pub fn to_midi_note(&self) -> MidiNote {
        let pitch = (self.target_midi + self.fine_cents * 0.01)
            .round()
            .clamp(0.0, 127.0) as u8;
        let vel = (38.0 + self.rms_norm.clamp(0.0, 1.0) * 89.0).round() as u8;
        MidiNote::new(
            pitch,
            vel,
            self.start_beat,
            (self.end_beat - self.start_beat).max(0.125),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineClip {
    pub id: ClipId,
    pub name: String,
    pub color_override: Option<[u8; 3]>,
    pub start_beat: f64,
    pub length_beats: f64,
    pub slip_offset_beats: f64,
    pub loop_enabled: bool,
    pub loop_length_beats: f64,
    pub muted: bool,
    pub gain_db: f32,
    pub pitch_semitones: f32,
    /// Playback speed / stretch ratio (`1.0` = original, `0.5` = half-time, `2.0` = double-time).
    #[serde(default = "default_one_f32")]
    pub stretch_rate: f32,
    #[serde(default)]
    pub stretch_mode: TimeStretchMode,
    #[serde(default)]
    pub warp_markers: Vec<StretchMarker>,
    #[serde(default)]
    pub pitch_blobs: Vec<NotePitchBlob>,
    pub fade_in_beats: f64,
    pub fade_out_beats: f64,
    pub fade_shape: FadeShape,
    pub takes: Vec<Take>,
    pub active_take: usize,
    pub comp_segments: Vec<CompSegment>,
    pub show_take_lanes: bool,
    pub clip_fx: Vec<FxSlot>,
}

fn default_one_f32() -> f32 {
    1.0
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
            name,
            color_override: None,
            start_beat,
            length_beats: length_beats.max(0.25),
            slip_offset_beats: 0.0,
            loop_enabled: true,
            loop_length_beats: length_beats.max(0.25),
            muted: false,
            gain_db: 0.0,
            pitch_semitones: 0.0,
            stretch_rate: 1.0,
            stretch_mode: TimeStretchMode::ElastiqueWsola,
            warp_markers: Vec::new(),
            pitch_blobs: Vec::new(),
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
            stretch_rate: 1.0,
            stretch_mode: TimeStretchMode::ElastiqueWsola,
            warp_markers: Vec::new(),
            pitch_blobs: Vec::new(),
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

    /// Evaluates total pitch-shift in semitones at `local_beat`, combining clip-level transpose
    /// with any Melodyne-style per-note `NotePitchBlob` edit!
    #[inline]
    pub fn effective_pitch_semitones_at(&self, local_beat: f64) -> f32 {
        let mut semi = self.pitch_semitones;
        for blob in &self.pitch_blobs {
            if local_beat >= blob.start_beat && local_beat <= blob.end_beat {
                semi += blob.net_pitch_shift_semitones();
                break;
            }
        }
        semi.clamp(-36.0, 36.0)
    }

    pub fn pitch_blobs_to_midi_notes(&self) -> Vec<MidiNote> {
        self.pitch_blobs.iter().map(|b| b.to_midi_note()).collect()
    }

    pub fn is_midi(&self) -> bool {
        self.takes
            .get(self.active_take)
            .map(|t| matches!(t.content, TakeContent::Midi { .. }))
            .unwrap_or(false)
    }

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

    #[inline]
    pub fn content_beat(&self, timeline_beat: f64) -> Option<(f64, f64)> {
        let local = timeline_beat - self.start_beat;
        if local < 0.0 || local >= self.length_beats {
            return None;
        }
        let raw = local * (self.stretch_rate.clamp(0.125, 8.0) as f64) + self.slip_offset_beats;
        let inner = if self.loop_enabled && self.loop_length_beats > 0.03125 {
            raw.rem_euclid(self.loop_length_beats)
        } else {
            raw
        };
        Some((local, inner))
    }

    /// Maps a clip's `inner_beat` to exact `source_seconds` throughpiecewise-linear `warp_markers`!
    #[inline]
    pub fn map_beat_to_source_seconds(&self, inner_beat: f64, spb: f64) -> f64 {
        if self.warp_markers.len() < 2 {
            return inner_beat * spb;
        }
        if inner_beat <= self.warp_markers[0].clip_beat {
            let delta_b = inner_beat - self.warp_markers[0].clip_beat;
            return (self.warp_markers[0].source_sec + delta_b * spb).max(0.0);
        }
        for pair in self.warp_markers.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            if inner_beat >= a.clip_beat && inner_beat <= b.clip_beat {
                let db = (b.clip_beat - a.clip_beat).max(1e-6);
                let t = (inner_beat - a.clip_beat) / db;
                return a.source_sec + t * (b.source_sec - a.source_sec);
            }
        }
        let last = self.warp_markers.last().unwrap();
        last.source_sec + (inner_beat - last.clip_beat) * spb
    }

    /// Auto-populates Warp/Stretch Markers from transient detections on the active audio take!
    pub fn generate_transient_warp_markers(&mut self, bpm: f64) {
        let spb = 60.0 / bpm.max(20.0);
        let idx = self.active_take.min(self.takes.len().saturating_sub(1));
        let Some(TakeContent::Audio { buffer }) = self.takes.get(idx).map(|t| &t.content) else {
            return;
        };
        let slices = buffer.detect_transients(16);
        let mut markers = Vec::with_capacity(slices.len() + 1);
        for frame in slices {
            let sec = frame as f64 / buffer.sample_rate.max(1) as f64;
            let beat = sec / spb;
            if beat <= self.length_beats {
                markers.push(StretchMarker {
                    id: Uuid::new_v4(),
                    clip_beat: beat,
                    source_sec: sec,
                });
            }
        }
        let end_sec = buffer.duration_secs();
        markers.push(StretchMarker {
            id: Uuid::new_v4(),
            clip_beat: self.length_beats,
            source_sec: end_sec,
        });
        markers.sort_by(|a, b| a.clip_beat.total_cmp(&b.clip_beat));
        self.warp_markers = markers;
    }

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
        next.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));

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
            content: TakeContent::Midi {
                notes,
                cc_points: Vec::new(),
            },
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
            TakeContent::Midi { notes, cc_points } => {
                let mut c = TimelineClip::new_midi(
                    self.name.clone(),
                    start_beat,
                    duration_beats,
                    notes.clone(),
                );
                if let Some(Take {
                    content: TakeContent::Midi { cc_points: dst, .. },
                    ..
                }) = c.takes.first_mut()
                {
                    *dst = cc_points.clone();
                }
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
