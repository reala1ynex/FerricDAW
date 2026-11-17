use crate::clip::{AudioBuffer, ClipId, MidiNote, SessionSlot, Take, TakeContent, TimelineClip};
use crate::fx::{FxKind, FxSlot, LfoShape, ModRouting, ModTarget, ModulatorKind, ModulatorSlot};
use crate::theory::{ChordEvent, ChordQuality, MusicalScale, VoicingStyle};
use crate::track::{
    ArpMode, AutomationLane, AutomationMode, AutomationPoint, AutomationTarget, GrooveTemplate,
    InputSource, Track, TrackId, TrackSend,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("RON serialization error: {0}")]
    Ron(String),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Routing would create a feedback cycle between tracks")]
    RoutingCycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl TimeSignature {
    pub fn beats_per_bar(self) -> f64 {
        (self.numerator as f64) * (4.0 / self.denominator.max(1) as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapGrid {
    Off,
    Sixteenth,
    Eighth,
    Quarter,
    Bar,
}

impl SnapGrid {
    pub const ALL: &'static [SnapGrid] = &[
        SnapGrid::Off,
        SnapGrid::Sixteenth,
        SnapGrid::Eighth,
        SnapGrid::Quarter,
        SnapGrid::Bar,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SnapGrid::Off => "Off (Slip)",
            SnapGrid::Sixteenth => "1/16 Beat",
            SnapGrid::Eighth => "1/8 Beat",
            SnapGrid::Quarter => "1/4 Beat",
            SnapGrid::Bar => "1 Bar",
        }
    }

    pub fn step_beats(self, ts: TimeSignature) -> Option<f64> {
        match self {
            SnapGrid::Off => None,
            SnapGrid::Sixteenth => Some(0.25),
            SnapGrid::Eighth => Some(0.5),
            SnapGrid::Quarter => Some(1.0),
            SnapGrid::Bar => Some(ts.beats_per_bar()),
        }
    }

    pub fn snap(self, beat: f64, ts: TimeSignature) -> f64 {
        match self.step_beats(ts) {
            Some(step) if step > 0.0 => ((beat / step).round() * step).max(0.0),
            _ => beat.max(0.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineMarker {
    pub id: Uuid,
    pub name: String,
    pub beat: f64,
    pub color: [u8; 3],
}

/// Conductor Tempo Map node: supports both Instant Step changes AND smooth Linear Accelerando / Ritardando ramps!
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub id: Uuid,
    pub beat: f64,
    pub bpm: f64,
    /// When `true`, tempo interpolates linearly from this point's `bpm` to the next `TempoPoint`'s `bpm`!
    #[serde(default)]
    pub ramp_to_next: bool,
}

impl TempoPoint {
    pub fn new(beat: f64, bpm: f64, ramp_to_next: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            beat: beat.max(0.0),
            bpm: bpm.clamp(20.0, 320.0),
            ramp_to_next,
        }
    }
}

/// Conductor Time-Signature Change node (`4/4` -> `3/4` -> `7/8` -> `6/8` mid-song!).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterChange {
    pub id: Uuid,
    pub beat: u32,
    pub signature: TimeSignature,
}

impl MeterChange {
    pub fn new(beat: u32, numerator: u8, denominator: u8) -> Self {
        Self {
            id: Uuid::new_v4(),
            beat,
            signature: TimeSignature {
                numerator: numerator.clamp(1, 32),
                denominator: denominator.clamp(1, 16),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: Uuid,
    pub name: String,
    pub length_beats: f64,
}

/// Hardware MIDI Controller CC -> Parameter binding (1-click MIDI Learn!).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiLearnMapping {
    pub id: Uuid,
    pub cc: u8,
    pub channel: Option<u8>,
    pub track_id: TrackId,
    pub target: AutomationTarget,
}

/// Large-Format Studio Console VCA Master Fader Group (scales member tracks' post-automation gain & mute/solo!).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcaGroup {
    pub id: Uuid,
    pub name: String,
    pub color: [u8; 3],
    pub gain_db: f32,
    pub mute: bool,
    pub solo: bool,
}

impl VcaGroup {
    pub fn new(name: impl Into<String>, color: [u8; 3]) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            color,
            gain_db: 0.0,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMixState {
    pub track_id: TrackId,
    pub gain_db: f32,
    pub pan: f32,
    pub stereo_width: f32,
    pub mute: bool,
    pub solo: bool,
    pub send_gains: Vec<(Uuid, f32, bool)>,
    pub fx_enabled_wet: Vec<(Uuid, bool, f32)>,
}

/// Console Mix Snapshot (`Mix A` / `Mix B` / `Mix C` / `Mix D`) for instant 1-click A/B mix comparison!
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixSnapshot {
    pub name: String,
    pub master_gain_db: f32,
    pub vca_gains: Vec<(Uuid, f32, bool, bool)>,
    pub tracks: Vec<TrackMixState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterBus {
    pub gain_db: f32,
    pub pan: f32,
    pub mono_check: bool,
    pub dim_enabled: bool,
    pub safety_limiter: bool,
    pub fx_chain: Vec<FxSlot>,
}

impl Default for MasterBus {
    fn default() -> Self {
        let mut eq = FxSlot::new(FxKind::ParametricEq);
        eq.set_param(0, 28.0);
        eq.set_param(10, 1.0);
        let mut tape = FxSlot::new(FxKind::TapeSaturator);
        tape.set_param(0, 2.5);
        tape.set_param(1, 35.0);
        tape.set_param(2, -0.2);
        Self {
            gain_db: 0.0,
            pan: 0.0,
            mono_check: false,
            dim_enabled: false,
            safety_limiter: true,
            fx_chain: vec![eq, tape],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub title: String,
    pub artist: String,
    pub bpm: f64,
    pub time_signature: TimeSignature,
    #[serde(default = "default_root_a")]
    pub root_pitch_class: u8,
    #[serde(default = "default_minor_scale")]
    pub musical_scale: MusicalScale,
    pub snap_grid: SnapGrid,
    pub loop_enabled: bool,
    pub loop_start_beat: f64,
    pub loop_end_beat: f64,
    pub punch_in_enabled: bool,
    pub punch_start_beat: f64,
    pub punch_end_beat: f64,
    pub metronome_enabled: bool,
    pub count_in_bars: u8,
    #[serde(default)]
    pub midi_learn_mappings: Vec<MidiLearnMapping>,
    #[serde(default)]
    pub tempo_map: Vec<TempoPoint>,
    #[serde(default)]
    pub meter_map: Vec<MeterChange>,
    #[serde(default)]
    pub show_conductor_lane: bool,
    #[serde(default)]
    pub chord_track: Vec<ChordEvent>,
    #[serde(default = "default_true_flag")]
    pub show_chord_lane: bool,
    #[serde(default)]
    pub vca_groups: Vec<VcaGroup>,
    #[serde(default)]
    pub mix_snapshots: Vec<Option<MixSnapshot>>,
    pub scenes: Vec<Scene>,
    pub markers: Vec<TimelineMarker>,
    pub tracks: Vec<Track>,
    pub master: MasterBus,
    /// Highest Night Current tutorial step index reached (survives title renames).
    #[serde(default)]
    pub tutorial_step_reached: u8,
    /// Set when the user completes a Bounce during the Night Current tutorial.
    #[serde(default)]
    pub tutorial_bounce_done: bool,
    /// Project Groove DNA — Feel macro (0..=1) scales track groove amounts.
    #[serde(default = "default_dna_feel")]
    pub dna_feel: f32,
    /// Project Velocity DNA depth (0..=1).
    #[serde(default)]
    pub dna_velocity: f32,
    #[serde(default)]
    pub dna_groove_template: crate::track::GrooveTemplate,
    #[serde(default)]
    pub dna_accent_shape: crate::track::VelocityAccentShape,
}

fn default_dna_feel() -> f32 {
    0.0
}

fn default_true_flag() -> bool {
    true
}

fn default_root_a() -> u8 {
    9 // A
}

fn default_minor_scale() -> MusicalScale {
    MusicalScale::NaturalMinor
}

impl Default for Project {
    fn default() -> Self {
        Self::new_empty("Untitled Session")
    }
}

impl Project {
    pub fn new_empty(title: impl Into<String>) -> Self {
        let scenes = vec![
            Scene {
                id: Uuid::new_v4(),
                name: "Intro".into(),
                length_beats: 16.0,
            },
            Scene {
                id: Uuid::new_v4(),
                name: "Groove".into(),
                length_beats: 16.0,
            },
            Scene {
                id: Uuid::new_v4(),
                name: "Hook".into(),
                length_beats: 16.0,
            },
            Scene {
                id: Uuid::new_v4(),
                name: "Stack".into(),
                length_beats: 16.0,
            },
        ];
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            artist: "Home Studio".into(),
            bpm: 118.0,
            time_signature: TimeSignature::default(),
            root_pitch_class: 9,
            musical_scale: MusicalScale::NaturalMinor,
            snap_grid: SnapGrid::Quarter,
            loop_enabled: true,
            loop_start_beat: 0.0,
            loop_end_beat: 16.0,
            punch_in_enabled: false,
            punch_start_beat: 4.0,
            punch_end_beat: 12.0,
            metronome_enabled: false,
            count_in_bars: 0,
            midi_learn_mappings: Vec::new(),
            tempo_map: Vec::new(),
            meter_map: Vec::new(),
            show_conductor_lane: false,
            chord_track: Vec::new(),
            show_chord_lane: true,
            vca_groups: Vec::new(),
            mix_snapshots: vec![None; 4],
            scenes,
            markers: vec![
                TimelineMarker {
                    id: Uuid::new_v4(),
                    name: "Intro".into(),
                    beat: 0.0,
                    color: [232, 114, 62],
                },
                TimelineMarker {
                    id: Uuid::new_v4(),
                    name: "Hook".into(),
                    beat: 16.0,
                    color: [66, 179, 165],
                },
            ],
            tracks: Vec::new(),
            master: MasterBus::default(),
            tutorial_step_reached: 0,
            tutorial_bounce_done: false,
            dna_feel: 0.0,
            dna_velocity: 0.0,
            dna_groove_template: crate::track::GrooveTemplate::Straight,
            dna_accent_shape: crate::track::VelocityAccentShape::Flat,
        }
    }

    /// Push project DNA onto drum-like tracks (name contains drum/kit/perc) or all MIDI instrument tracks.
    pub fn apply_dna_to_tracks(&mut self, drums_only: bool) {
        let feel = self.dna_feel.clamp(0.0, 1.0);
        let tmpl = self.dna_groove_template;
        for t in &mut self.tracks {
            let name = t.name.to_lowercase();
            let is_drum = name.contains("drum")
                || name.contains("kit")
                || name.contains("perc")
                || t.fx_chain.iter().any(|f| f.kind == crate::fx::FxKind::DrumKit);
            if drums_only && !is_drum {
                continue;
            }
            if !drums_only {
                let has_inst = t.fx_chain.iter().any(|f| f.kind.is_instrument());
                if !has_inst && !is_drum {
                    continue;
                }
            }
            t.groove_template = tmpl;
            t.groove_amount = feel;
        }
    }

    /// Evaluates instantaneous BPM at `beat` from the Conductor `tempo_map` (supporting linear ramps!),
    /// falling back cleanly to `self.bpm` when `tempo_map` is empty.
    #[inline]
    pub fn bpm_at_beat(&self, beat: f64) -> f64 {
        if self.tempo_map.is_empty() {
            return self.bpm.clamp(20.0, 320.0);
        }
        if beat <= self.tempo_map[0].beat {
            return self.tempo_map[0].bpm.clamp(20.0, 320.0);
        }
        for pair in self.tempo_map.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            if beat >= a.beat && beat < b.beat {
                if a.ramp_to_next {
                    let span = (b.beat - a.beat).max(1e-6);
                    let u = ((beat - a.beat) / span).clamp(0.0, 1.0);
                    return (a.bpm + (b.bpm - a.bpm) * u).clamp(20.0, 320.0);
                } else {
                    return a.bpm.clamp(20.0, 320.0);
                }
            }
        }
        self.tempo_map.last().unwrap().bpm.clamp(20.0, 320.0)
    }

    /// Evaluates active `TimeSignature` at `beat` from the Conductor `meter_map`!
    #[inline]
    pub fn time_signature_at_beat(&self, beat: f64) -> TimeSignature {
        let mut cur = self.time_signature;
        for mc in &self.meter_map {
            if beat + 1e-6 >= mc.beat as f64 {
                cur = mc.signature;
            } else {
                break;
            }
        }
        cur
    }

    pub fn upsert_tempo_point(&mut self, beat: f64, bpm: f64, ramp_to_next: bool) -> Uuid {
        let bpm = bpm.clamp(20.0, 320.0);
        if self.tempo_map.is_empty() && beat > 0.1 {
            self.tempo_map
                .push(TempoPoint::new(0.0, self.bpm, true));
        }
        if let Some(existing) = self
            .tempo_map
            .iter_mut()
            .find(|p| (p.beat - beat).abs() < 0.12)
        {
            existing.bpm = bpm;
            existing.beat = beat.max(0.0);
            let id = existing.id;
            self.sort_conductor_maps();
            return id;
        }
        let pt = TempoPoint::new(beat, bpm, ramp_to_next);
        let id = pt.id;
        self.tempo_map.push(pt);
        self.sort_conductor_maps();
        id
    }

    pub fn add_meter_change(&mut self, beat: u32, numerator: u8, denominator: u8) -> Uuid {
        if let Some(existing) = self.meter_map.iter_mut().find(|m| m.beat == beat) {
            existing.signature = TimeSignature {
                numerator: numerator.clamp(1, 32),
                denominator: denominator.clamp(1, 16),
            };
            return existing.id;
        }
        let mc = MeterChange::new(beat, numerator, denominator);
        let id = mc.id;
        self.meter_map.push(mc);
        self.sort_conductor_maps();
        id
    }

    pub fn sort_conductor_maps(&mut self) {
        self.tempo_map
            .sort_by(|a, b| a.beat.total_cmp(&b.beat));
        self.meter_map.sort_by_key(|m| m.beat);
        if let Some(first) = self.tempo_map.first() {
            if first.beat <= 0.01 {
                self.bpm = first.bpm;
            }
        }
    }

    /// Exact closed-form analytical integral `\int_0^beats (60 / BPM(b)) db` across Stepped & Linear Tempo Ramps!
    pub fn beats_to_seconds(&self, beats: f64) -> f64 {
        let target = beats.max(0.0);
        if target <= 0.0 {
            return 0.0;
        }
        if self.tempo_map.is_empty() {
            return target * 60.0 / self.bpm.max(20.0);
        }

        let mut secs = 0.0_f64;
        let mut cursor = 0.0_f64;

        // Pre-first-point segment `[0 .. tempo_map[0].beat]`
        let first = &self.tempo_map[0];
        if first.beat > 0.0 {
            let seg_end = target.min(first.beat);
            secs += seg_end * (60.0 / first.bpm.max(20.0));
            cursor = seg_end;
            if cursor >= target {
                return secs;
            }
        }

        for pair in self.tempo_map.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            if target <= a.beat {
                break;
            }
            let s0 = cursor.max(a.beat);
            let s1 = target.min(b.beat);
            if s1 > s0 {
                let db = s1 - s0;
                if a.ramp_to_next && (b.bpm - a.bpm).abs() > 1e-4 && (b.beat - a.beat) > 1e-6 {
                    let bpm0 = self.bpm_at_beat(s0);
                    let bpm1 = self.bpm_at_beat(s1);
                    secs += 60.0 * db * (bpm1 / bpm0).ln() / (bpm1 - bpm0);
                } else {
                    secs += db * (60.0 / a.bpm.max(20.0));
                }
                cursor = s1;
            }
            if cursor >= target {
                return secs;
            }
        }

        let last_bpm = self.tempo_map.last().unwrap().bpm.max(20.0);
        if target > cursor {
            secs += (target - cursor) * (60.0 / last_bpm);
        }
        secs
    }

    pub fn beat_range_duration_secs(&self, start_beat: f64, end_beat: f64) -> f64 {
        (self.beats_to_seconds(end_beat) - self.beats_to_seconds(start_beat)).max(0.0)
    }

    /// Monotonic inverse `seconds -> beats` solver (exact for constant BPM, sub-microsecond bisection for variable ramps!).
    pub fn seconds_to_beats(&self, secs: f64) -> f64 {
        let s = secs.max(0.0);
        if self.tempo_map.is_empty() {
            return s * self.bpm.max(20.0) / 60.0;
        }
        let mut lo = 0.0_f64;
        let mut hi = (s * 320.0 / 60.0).max(4.0);
        while self.beats_to_seconds(hi) < s {
            hi *= 2.0;
        }
        for _ in 0..42 {
            let mid = 0.5 * (lo + hi);
            if self.beats_to_seconds(mid) < s {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// Multi-Meter Bar.Beat.Tick formatter (`001.1.000`) supporting mid-song Time Signature changes!
    pub fn format_bar_beat_tick(&self, beat: f64) -> String {
        let (bar, in_bar_beat, _ts) = self.bar_and_beat_at(beat.max(0.0));
        let beat_num = in_bar_beat.floor() as i64 + 1;
        let ticks = ((in_bar_beat.fract()) * 960.0).round() as i64;
        format!("{:03}.{}.{:03}", bar, beat_num, ticks.clamp(0, 959))
    }

    pub fn bar_and_beat_at(&self, beat: f64) -> (i64, f64, TimeSignature) {
        let b = beat.max(0.0);
        if self.meter_map.is_empty() {
            let bpb = self.time_signature.beats_per_bar();
            let bar = (b / bpb).floor() as i64 + 1;
            let in_bar = b.rem_euclid(bpb);
            return (bar, in_bar, self.time_signature);
        }

        let mut cur_bar = 1_i64;
        let mut cur_beat = 0.0_f64;
        let mut cur_ts = self.time_signature;

        for mc in &self.meter_map {
            let m_beat = mc.beat as f64;
            if m_beat <= cur_beat {
                cur_ts = mc.signature;
                continue;
            }
            if b < m_beat {
                break;
            }
            let bpb = cur_ts.beats_per_bar();
            let bars_elapsed = ((m_beat - cur_beat) / bpb).ceil() as i64;
            cur_bar += bars_elapsed.max(0);
            cur_beat = m_beat;
            cur_ts = mc.signature;
        }

        let bpb = cur_ts.beats_per_bar();
        let rem = (b - cur_beat).max(0.0);
        let add_bars = (rem / bpb).floor() as i64;
        let in_bar = rem.rem_euclid(bpb);
        (cur_bar + add_bars, in_bar, cur_ts)
    }

    pub fn project_end_beat(&self) -> f64 {
        let max_clip = self
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.end_beat())
            .fold(0.0_f64, f64::max);
        max_clip.max(self.loop_end_beat).max(16.0)
    }

    pub fn find_track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn find_track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn track_depth(&self, track_id: TrackId) -> usize {
        let mut depth = 0;
        let mut cur = self.find_track(track_id).and_then(|t| t.parent_folder_id);
        while let Some(pid) = cur {
            depth += 1;
            if depth > 16 {
                break;
            }
            cur = self.find_track(pid).and_then(|t| t.parent_folder_id);
        }
        depth
    }

    pub fn is_folder_track(&self, track_id: TrackId) -> bool {
        self.tracks
            .iter()
            .any(|t| t.parent_folder_id == Some(track_id))
    }

    /// Computes a cycle-free topological evaluation order of track indices so that all
    /// folder children, aux send sources, AND external sidechain key sources execute strictly BEFORE their destinations!
    pub fn topological_track_order(&self) -> Vec<usize> {
        let n = self.tracks.len();
        if n == 0 {
            return Vec::new();
        }
        let id_to_idx: HashMap<TrackId, usize> = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();

        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indeg = vec![0usize; n];

        for (src_idx, track) in self.tracks.iter().enumerate() {
            if let Some(parent_id) = track.parent_folder_id {
                if let Some(&dst_idx) = id_to_idx.get(&parent_id) {
                    if src_idx != dst_idx && !adj[src_idx].contains(&dst_idx) {
                        adj[src_idx].push(dst_idx);
                        indeg[dst_idx] += 1;
                    }
                }
            }
            for send in &track.sends {
                if let Some(&dst_idx) = id_to_idx.get(&send.target_track_id) {
                    if src_idx != dst_idx && !adj[src_idx].contains(&dst_idx) {
                        adj[src_idx].push(dst_idx);
                        indeg[dst_idx] += 1;
                    }
                }
            }
            // External sidechain dependency: `sc_id` -> `src_idx`
            for fx in &track.fx_chain {
                if let Some(sc_id) = fx.sidechain_source {
                    if let Some(&sc_idx) = id_to_idx.get(&sc_id) {
                        if sc_idx != src_idx && !adj[sc_idx].contains(&src_idx) {
                            adj[sc_idx].push(src_idx);
                            indeg[src_idx] += 1;
                        }
                    }
                }
            }
        }

        let mut queue: VecDeque<usize> = indeg
            .iter()
            .enumerate()
            .filter_map(|(i, &d)| (d == 0).then_some(i))
            .collect();
        let mut order = Vec::with_capacity(n);
        while let Some(u) = queue.pop_front() {
            order.push(u);
            for &v in &adj[u] {
                indeg[v] -= 1;
                if indeg[v] == 0 {
                    queue.push_back(v);
                }
            }
        }

        if order.len() < n {
            for i in 0..n {
                if !order.contains(&i) {
                    order.push(i);
                }
            }
        }
        order
    }

    pub fn can_route(&self, source_id: TrackId, target_id: TrackId) -> bool {
        if source_id == target_id {
            return false;
        }
        let mut stack = vec![target_id];
        let mut visited = Vec::new();
        while let Some(cur) = stack.pop() {
            if cur == source_id {
                return false;
            }
            if visited.contains(&cur) {
                continue;
            }
            visited.push(cur);
            if let Some(track) = self.find_track(cur) {
                if let Some(parent) = track.parent_folder_id {
                    stack.push(parent);
                }
                for s in &track.sends {
                    stack.push(s.target_track_id);
                }
            }
            // Any track that sidechains off `cur` also depends on `cur`
            for other in &self.tracks {
                if other
                    .fx_chain
                    .iter()
                    .any(|f| f.sidechain_source == Some(cur))
                {
                    stack.push(other.id);
                }
            }
        }
        true
    }

    pub fn add_send(
        &mut self,
        src_id: TrackId,
        dst_id: TrackId,
        gain_db: f32,
    ) -> Result<(), ModelError> {
        if !self.can_route(src_id, dst_id) {
            return Err(ModelError::RoutingCycle);
        }
        if let Some(t) = self.find_track_mut(src_id) {
            if !t.sends.iter().any(|s| s.target_track_id == dst_id) {
                t.sends.push(TrackSend::new(dst_id, gain_db));
            }
        }
        Ok(())
    }

    /// Applies an incoming hardware MIDI CC value (`0..=127`) to any learned parameter,
    /// and writes automation points when Touch/Latch/Write is active!
    pub fn apply_midi_cc(
        &mut self,
        channel: u8,
        cc: u8,
        val_7bit: u8,
        playhead_beat: f64,
        is_playing: bool,
    ) -> bool {
        let norm = (val_7bit.min(127) as f32) / 127.0;
        let matches: Vec<(TrackId, AutomationTarget)> = self
            .midi_learn_mappings
            .iter()
            .filter(|m| m.cc == cc && m.channel.map(|c| c == channel).unwrap_or(true))
            .map(|m| (m.track_id, m.target.clone()))
            .collect();

        if matches.is_empty() {
            return false;
        }

        for (tid, target) in matches {
            self.set_target_normalized(tid, &target, norm, playhead_beat, is_playing);
        }
        true
    }

    pub fn set_target_normalized(
        &mut self,
        track_id: TrackId,
        target: &AutomationTarget,
        norm: f32,
        playhead_beat: f64,
        is_playing: bool,
    ) {
        let norm = norm.clamp(0.0, 1.0);
        let Some(trk) = self.find_track_mut(track_id) else {
            return;
        };
        match *target {
            AutomationTarget::Volume => trk.gain_db = -60.0 + norm * 72.0,
            AutomationTarget::Pan => trk.pan = norm * 2.0 - 1.0,
            AutomationTarget::StereoWidth => trk.stereo_width = norm * 2.0,
            AutomationTarget::FxWet { fx_id } => {
                if let Some(fx) = trk.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                    fx.wet_mix = norm;
                }
            }
            AutomationTarget::FxParam { fx_id, param_index } => {
                if let Some(fx) = trk.fx_chain.iter_mut().find(|f| f.id == fx_id) {
                    let denorm = fx.denormalize_param(param_index, norm);
                    fx.set_param(param_index, denorm);
                }
            }
        }

        // Live Automation Write / Touch / Latch!
        if is_playing
            && matches!(
                trk.automation_mode,
                AutomationMode::Touch | AutomationMode::Latch | AutomationMode::Write
            )
        {
            let lane_idx = trk.ensure_automation_lane(target.clone());
            trk.automation_lanes[lane_idx].upsert_point(playhead_beat, norm);
        }
    }

    pub fn add_track(&mut self, name: impl Into<String>) -> TrackId {
        let idx = self.tracks.len();
        let track = Track::new(name, idx, self.scenes.len());
        let id = track.id;
        self.tracks.push(track);
        id
    }

    /// Cleanly removes a track and cleans up any folder/send/sidechain/MIDI-learn references pointing to it!
    pub fn delete_track(&mut self, track_id: TrackId) -> bool {
        let Some(pos) = self.tracks.iter().position(|t| t.id == track_id) else {
            return false;
        };
        let parent_of_deleted = self.tracks[pos].parent_folder_id;
        self.tracks.remove(pos);

        for t in &mut self.tracks {
            if t.parent_folder_id == Some(track_id) {
                t.parent_folder_id = parent_of_deleted;
            }
            t.sends.retain(|s| s.target_track_id != track_id);
            for fx in &mut t.fx_chain {
                if fx.sidechain_source == Some(track_id) {
                    fx.sidechain_source = None;
                }
            }
        }
        self.midi_learn_mappings
            .retain(|m| m.track_id != track_id);
        true
    }

    pub fn duplicate_track(&mut self, track_id: TrackId) -> Option<TrackId> {
        let pos = self.tracks.iter().position(|t| t.id == track_id)?;
        let mut dup = self.tracks[pos].clone();
        dup.id = Uuid::new_v4();
        dup.name = format!("{} (Copy)", dup.name);
        for fx in &mut dup.fx_chain {
            fx.id = Uuid::new_v4();
        }
        for m in &mut dup.modulators {
            m.id = Uuid::new_v4();
            m.routings.clear();
        }
        for c in &mut dup.clips {
            c.id = Uuid::new_v4();
            for fx in &mut c.clip_fx {
                fx.id = Uuid::new_v4();
            }
        }
        let new_id = dup.id;
        self.tracks.insert(pos + 1, dup);
        Some(new_id)
    }

    pub fn move_track(&mut self, track_id: TrackId, delta: isize) -> bool {
        let Some(pos) = self.tracks.iter().position(|t| t.id == track_id) else {
            return false;
        };
        let target = (pos as isize + delta).clamp(0, self.tracks.len().saturating_sub(1) as isize)
            as usize;
        if target == pos {
            return false;
        }
        self.tracks.swap(pos, target);
        true
    }

    /// Moves a `TimelineClip` from `from_track` onto `to_track` cleanly!
    pub fn move_clip_to_track(
        &mut self,
        from_track: TrackId,
        to_track: TrackId,
        clip_id: ClipId,
    ) -> bool {
        if from_track == to_track {
            return false;
        }
        let Some(src) = self.find_track_mut(from_track) else {
            return false;
        };
        let Some(pos) = src.clips.iter().position(|c| c.id == clip_id) else {
            return false;
        };
        let clip = src.clips.remove(pos);
        if let Some(dst) = self.find_track_mut(to_track) {
            dst.clips.push(clip);
            true
        } else {
            false
        }
    }

    pub fn add_marker(&mut self, name: impl Into<String>, beat: f64) -> Uuid {
        let idx = self.markers.len();
        let col = crate::track::STUDIO_TRACK_COLORS[idx % crate::track::STUDIO_TRACK_COLORS.len()];
        let m = TimelineMarker {
            id: Uuid::new_v4(),
            name: name.into(),
            beat: beat.max(0.0),
            color: col,
        };
        let id = m.id;
        self.markers.push(m);
        self.markers
            .sort_by(|a, b| a.beat.total_cmp(&b.beat));
        id
    }

    /// REAPER-style Glue / Consolidate Clips (`Ctrl+G`): merges two or more MIDI or Audio clips
    /// on `track_id` into a single phase-accurate consolidated clip!
    pub fn glue_clips(
        &mut self,
        track_id: TrackId,
        clip_ids: &[ClipId],
        sample_rate: u32,
    ) -> Option<ClipId> {
        let spb = 60.0 / self.bpm.max(20.0);
        let track = self.find_track_mut(track_id)?;
        let selected: Vec<TimelineClip> = track
            .clips
            .iter()
            .filter(|c| clip_ids.contains(&c.id))
            .cloned()
            .collect();
        if selected.len() < 2 {
            return None;
        }

        let min_start = selected
            .iter()
            .map(|c| c.start_beat)
            .fold(f64::MAX, f64::min);
        let max_end = selected
            .iter()
            .map(|c| c.end_beat())
            .fold(0.0_f64, f64::max);
        let total_beats = (max_end - min_start).max(0.25);

        let all_midi = selected.iter().all(|c| c.is_midi());
        let glued = if all_midi {
            let mut merged_notes = Vec::new();
            let mut merged_ccs = Vec::new();
            for clip in &selected {
                let offset = clip.start_beat - min_start;
                let loop_len = if clip.loop_enabled {
                    clip.loop_length_beats.max(0.25)
                } else {
                    clip.length_beats.max(0.25)
                };
                let reps = (clip.length_beats / loop_len).ceil() as usize;
                let t_idx = clip.active_take.min(clip.takes.len().saturating_sub(1));
                if let Some(Take {
                    content: TakeContent::Midi { notes, cc_points },
                    ..
                }) = clip.takes.get(t_idx)
                {
                    for rep in 0..reps.max(1) {
                        let base_b = rep as f64 * loop_len;
                        for n in notes {
                            let local_s = base_b + n.start_beat - clip.slip_offset_beats;
                            if local_s >= 0.0 && local_s < clip.length_beats {
                                let len = n.length_beats.min(clip.length_beats - local_s);
                                let mut copy = *n;
                                copy.id = Uuid::new_v4();
                                copy.start_beat = offset + local_s;
                                copy.length_beats = len.max(0.0625);
                                copy.pitch =
                                    (copy.pitch as i32 + clip.pitch_semitones.round() as i32)
                                        .clamp(0, 127) as u8;
                                merged_notes.push(copy);
                            }
                        }
                    }
                    for cc in cc_points {
                        let mut ccopy = *cc;
                        ccopy.beat += offset;
                        merged_ccs.push(ccopy);
                    }
                }
            }
            merged_notes.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
            merged_ccs.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            let mut c = TimelineClip::new_midi(
                format!("{} (Glued)", selected[0].name),
                min_start,
                total_beats,
                merged_notes,
            );
            c.loop_enabled = false;
            if let Some(Take {
                content: TakeContent::Midi { cc_points, .. },
                ..
            }) = c.takes.first_mut()
            {
                *cc_points = merged_ccs;
            }
            c
        } else {
            let sr = sample_rate.max(44_100);
            let total_frames = (total_beats * spb * sr as f64).ceil() as usize;
            let mut out_l = vec![0.0_f32; total_frames];
            let mut out_r = vec![0.0_f32; total_frames];
            let beats_per_frame = 1.0 / (spb * sr as f64);

            for (f_idx, (sl, sr_out)) in out_l.iter_mut().zip(out_r.iter_mut()).enumerate() {
                let tl_beat = min_start + f_idx as f64 * beats_per_frame;
                for clip in &selected {
                    if let Some((local_b, inner_b)) = clip.content_beat(tl_beat) {
                        let take_i = clip.take_at_local_beat(local_b);
                        let env = clip.gain_envelope_at(local_b);
                        if let Some(Take {
                            content: TakeContent::Audio { buffer },
                            ..
                        }) = clip.takes.get(take_i)
                        {
                            let src_sec = clip.map_beat_to_source_seconds(inner_b, spb);
                            let (l, r) =
                                buffer.sample_interpolated(src_sec * buffer.sample_rate as f64);
                            *sl += l * env;
                            *sr_out += r * env;
                        }
                    }
                }
            }
            let buf = AudioBuffer::from_stereo(sr, out_l, out_r, None);
            TimelineClip::new_audio(
                format!("{} (Glued)", selected[0].name),
                min_start,
                total_beats,
                buf,
            )
        };

        let new_id = glued.id;
        track.clips.retain(|c| !clip_ids.contains(&c.id));
        track.clips.push(glued);
        Some(new_id)
    }

    pub fn split_clip(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        split_beat: f64,
    ) -> Option<ClipId> {
        let track = self.find_track_mut(track_id)?;
        let pos = track.clips.iter().position(|c| c.id == clip_id)?;
        let orig = &track.clips[pos];
        if split_beat <= orig.start_beat + 0.03125 || split_beat >= orig.end_beat() - 0.03125 {
            return None;
        }
        let left_len = split_beat - orig.start_beat;
        let right_len = orig.end_beat() - split_beat;

        let mut right_clip = orig.clone();
        right_clip.id = Uuid::new_v4();
        right_clip.start_beat = split_beat;
        right_clip.length_beats = right_len;
        right_clip.slip_offset_beats =
            orig.slip_offset_beats + left_len * (orig.stretch_rate as f64);
        right_clip.fade_in_beats = 0.02;
        if !right_clip.comp_segments.is_empty() {
            right_clip.comp_segments = right_clip
                .comp_segments
                .into_iter()
                .filter_map(|mut seg| {
                    if seg.end_beat <= left_len {
                        None
                    } else {
                        seg.start_beat = (seg.start_beat - left_len).max(0.0);
                        seg.end_beat = (seg.end_beat - left_len).min(right_len);
                        Some(seg)
                    }
                })
                .collect();
        }

        let left_clip = &mut track.clips[pos];
        left_clip.length_beats = left_len;
        left_clip.fade_out_beats = 0.02;
        if !left_clip.comp_segments.is_empty() {
            left_clip.comp_segments.retain_mut(|seg| {
                if seg.start_beat >= left_len {
                    false
                } else {
                    seg.end_beat = seg.end_beat.min(left_len);
                    true
                }
            });
        }

        let new_id = right_clip.id;
        track.clips.insert(pos + 1, right_clip);
        Some(new_id)
    }

    pub fn duplicate_clip(&mut self, track_id: TrackId, clip_id: ClipId) -> Option<ClipId> {
        let track = self.find_track_mut(track_id)?;
        let clip = track.find_clip(clip_id)?.clone();
        let mut dup = clip.clone();
        dup.id = Uuid::new_v4();
        dup.start_beat = clip.end_beat();
        let id = dup.id;
        track.clips.push(dup);
        Some(id)
    }

    /// Deletes a clip, optionally ripple-shifting all later clips on the track left by `clip.length_beats`!
    pub fn delete_clip(&mut self, track_id: TrackId, clip_id: ClipId, ripple: bool) -> bool {
        let Some(track) = self.find_track_mut(track_id) else {
            return false;
        };
        let Some(pos) = track.clips.iter().position(|c| c.id == clip_id) else {
            return false;
        };
        let removed = track.clips.remove(pos);
        if ripple {
            let cut_start = removed.start_beat;
            let shift = removed.length_beats;
            for c in &mut track.clips {
                if c.start_beat >= cut_start + shift - 1e-4 {
                    c.start_beat = (c.start_beat - shift).max(0.0);
                }
            }
        }
        true
    }

    pub fn print_scene_to_timeline(&mut self, scene_idx: usize, at_beat: f64) {
        let Some(scene) = self.scenes.get(scene_idx).cloned() else {
            return;
        };
        for track in &mut self.tracks {
            if let Some(Some(slot)) = track.session_slots.get(scene_idx) {
                let clip = slot.to_timeline_clip(at_beat, scene.length_beats);
                track.clips.push(clip);
            }
        }
    }

    pub fn launch_scene(&mut self, scene_idx: usize) {
        for track in &mut self.tracks {
            if track
                .session_slots
                .get(scene_idx)
                .and_then(|s| s.as_ref())
                .is_some()
            {
                track.active_session_slot = Some(scene_idx);
            } else {
                track.active_session_slot = None;
            }
        }
    }

    pub fn stop_all_session_clips(&mut self) {
        for track in &mut self.tracks {
            track.active_session_slot = None;
        }
    }

    pub fn quantize_midi_clip(&mut self, track_id: TrackId, clip_id: ClipId, step_beats: f64) {
        let step = step_beats.max(0.0625);
        if let Some(clip) = self
            .find_track_mut(track_id)
            .and_then(|t| t.find_clip_mut(clip_id))
        {
            let active = clip.active_take;
            if let Some(Take {
                content: TakeContent::Midi { notes, .. },
                ..
            }) = clip.takes.get_mut(active)
            {
                for note in notes {
                    note.start_beat = (note.start_beat / step).round() * step;
                    note.length_beats = ((note.length_beats / step).round() * step).max(step);
                }
            }
        }
    }

    pub fn humanize_midi_clip(&mut self, track_id: TrackId, clip_id: ClipId) {
        if let Some(clip) = self
            .find_track_mut(track_id)
            .and_then(|t| t.find_clip_mut(clip_id))
        {
            let active = clip.active_take;
            if let Some(Take {
                content: TakeContent::Midi { notes, .. },
                ..
            }) = clip.takes.get_mut(active)
            {
                let mut seed = 0x9E37_79B9_u32;
                for n in notes {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let r1 = ((seed & 0xFF) as f64 / 255.0) - 0.5;
                    let r2 = (((seed >> 8) & 0xFF) as f32 / 255.0) - 0.5;
                    n.start_beat = (n.start_beat + r1 * 0.035).max(0.0);
                    n.velocity = (n.velocity as f32 + r2 * 16.0).clamp(20.0, 127.0) as u8;
                }
            }
        }
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), ModelError> {
        let is_json = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        let text = if is_json {
            serde_json::to_string_pretty(self)?
        } else {
            let pretty = ron::ser::PrettyConfig::new()
                .depth_limit(6)
                .separate_tuple_members(true)
                .enumerate_arrays(false);
            ron::ser::to_string_pretty(self, pretty).map_err(|e| ModelError::Ron(e.to_string()))?
        };
        // Never truncate the user's project in place (audit H1): write a
        // temporary file, flush it, then atomically replace the old file and
        // keep the previous version as `<name>.bak`.
        crate::fsutil::write_atomic(path, text.as_bytes(), true)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self, ModelError> {
        let text = std::fs::read_to_string(path)?;
        let is_json = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        let mut proj: Self = if is_json {
            serde_json::from_str(&text)?
        } else {
            ron::from_str(&text).map_err(|e| ModelError::Ron(e.to_string()))?
        };
        if proj.mix_snapshots.len() < 4 {
            proj.mix_snapshots.resize_with(4, || None);
        }
        Ok(proj)
    }

    // ========================================================================
    // Milestone 9: Chord Track Arranger, VCA Console Groups & Mix A/B/C Recall
    // ========================================================================

    #[inline]
    pub fn chord_at_beat(&self, beat: f64) -> Option<&ChordEvent> {
        self.chord_track
            .iter()
            .find(|c| beat >= c.start_beat && beat < c.end_beat())
    }

    pub fn sort_chord_track(&mut self) {
        self.chord_track
            .sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
    }

    pub fn upsert_chord_event(
        &mut self,
        start_beat: f64,
        length_beats: f64,
        root_pc: u8,
        quality: ChordQuality,
    ) -> Uuid {
        let sb = start_beat.max(0.0);
        let len = length_beats.max(0.25);
        // Trim or remove overlapping chord events in `[sb .. sb + len]`
        self.chord_track
            .retain(|c| !(c.start_beat >= sb && c.end_beat() <= sb + len));
        for c in &mut self.chord_track {
            if c.start_beat < sb && c.end_beat() > sb {
                c.length_beats = (sb - c.start_beat).max(0.25);
            }
        }
        let ev = ChordEvent::new(sb, len, root_pc, quality);
        let id = ev.id;
        self.chord_track.push(ev);
        self.sort_chord_track();
        id
    }

    /// 1-Click Songwriting Arranger: Generates a voiced Pad Track, a locked Sub-Bass Track, and/or
    /// a Shimmer Arp Track directly from `self.chord_track`!
    /// Cumulative beat range for a scene index (Intro / Groove / … storyboard sections).
    pub fn scene_beat_range(&self, scene_idx: usize) -> Option<(f64, f64)> {
        if scene_idx >= self.scenes.len() {
            return None;
        }
        let mut start = 0.0_f64;
        for (i, s) in self.scenes.iter().enumerate() {
            let end = start + s.length_beats.max(0.25);
            if i == scene_idx {
                return Some((start, end));
            }
            start = end;
        }
        None
    }

    /// Generate arrangement from the chord lane. Optional `beat_range` scopes writes to a section.
    /// Returns created track ids and a human patch-card summary for Undo via history.
    pub fn generate_arrangement_from_chords(
        &mut self,
        voicing: VoicingStyle,
        spawn_pad: bool,
        spawn_bass: bool,
        spawn_arp: bool,
    ) -> Vec<TrackId> {
        self.generate_arrangement_from_chords_in_range(voicing, spawn_pad, spawn_bass, spawn_arp, None)
            .0
    }

    pub fn generate_arrangement_from_chords_in_range(
        &mut self,
        voicing: VoicingStyle,
        spawn_pad: bool,
        spawn_bass: bool,
        spawn_arp: bool,
        beat_range: Option<(f64, f64)>,
    ) -> (Vec<TrackId>, String) {
        if self.chord_track.is_empty() {
            return (Vec::new(), "No chords in lane.".into());
        }
        self.sort_chord_track();
        let chords: Vec<_> = self
            .chord_track
            .iter()
            .filter(|ch| {
                let Some((a, b)) = beat_range else {
                    return true;
                };
                ch.start_beat < b && ch.end_beat() > a
            })
            .cloned()
            .collect();
        if chords.is_empty() {
            return (
                Vec::new(),
                "No chords inside selected storyboard section.".into(),
            );
        }
        let min_b = chords
            .iter()
            .map(|c| c.start_beat)
            .fold(f64::INFINITY, f64::min);
        let max_b = chords
            .iter()
            .map(|c| c.end_beat())
            .fold(4.0_f64, f64::max);
        let clip_len = (max_b - min_b).max(1.0);
        let mut created = Vec::new();
        let mut parts = Vec::new();

        if spawn_pad {
            let mut pad_notes = Vec::new();
            for ch in &chords {
                let rel_b = (ch.start_beat - min_b).max(0.0);
                for pitch in ch.voiced_pitches(voicing, 60) {
                    pad_notes.push(MidiNote::new(
                        pitch,
                        88,
                        rel_b,
                        (ch.length_beats - 0.08).max(0.2),
                    ));
                }
            }
            let tid = self.add_track("Harmony Pad (Chord Track)");
            if let Some(trk) = self.find_track_mut(tid) {
                let mut wt = FxSlot::new(FxKind::WavetableSynth);
                wt.set_param(0, 1.0); // Vocal Formant
                wt.set_param(2, 6.0); // 6x Unison
                wt.set_param(4, 42.0); // Shimmer Cloud
                trk.fx_chain.push(wt);
                trk.fx_chain.push(FxSlot::new(FxKind::ConvolutionIr));
                trk.gain_db = -4.5;
                trk.clips
                    .push(TimelineClip::new_midi("Voiced Chords", min_b, clip_len, pad_notes));
            }
            created.push(tid);
            parts.push("Pad");
        }

        if spawn_bass {
            let mut bass_notes = Vec::new();
            for ch in &chords {
                let rel_b = (ch.start_beat - min_b).max(0.0);
                let root_midi = 36 + (ch.root_pc % 12); // C2..B2 sub-bass register
                let fifth_midi = root_midi + 7;
                let steps = (ch.length_beats / 0.5).floor().max(1.0) as usize;
                for s in 0..steps {
                    let sb = rel_b + s as f64 * 0.5;
                    let is_pickup = s + 1 == steps && steps >= 4;
                    let pitch = if is_pickup {
                        fifth_midi
                    } else if s % 4 == 2 {
                        root_midi + 12
                    } else {
                        root_midi
                    };
                    let vel = if s % 2 == 0 { 112 } else { 90 };
                    bass_notes.push(MidiNote::new(pitch, vel, sb, 0.42));
                }
            }
            let tid = self.add_track("Harmony Bassline");
            if let Some(trk) = self.find_track_mut(tid) {
                let mut syn = FxSlot::new(FxKind::PolySynth);
                syn.set_param(0, 1.0); // Saw
                syn.set_param(4, 340.0); // Punchy lowpass
                syn.set_param(5, 0.35);
                trk.fx_chain.push(syn);
                trk.fx_chain.push(FxSlot::new(FxKind::MultibandOtt));
                trk.groove_template = GrooveTemplate::Mpc58Classic;
                trk.gain_db = -3.0;
                trk.clips
                    .push(TimelineClip::new_midi("Chord Bassline", min_b, clip_len, bass_notes));
            }
            created.push(tid);
            parts.push("Bass");
        }

        if spawn_arp {
            let mut arp_notes = Vec::new();
            for ch in &chords {
                let rel_b = (ch.start_beat - min_b).max(0.0);
                for pitch in ch.voiced_pitches(VoicingStyle::ClosePiano, 60) {
                    arp_notes.push(MidiNote::new(pitch, 96, rel_b, ch.length_beats));
                }
            }
            let tid = self.add_track("Harmony Pluck Arp");
            if let Some(trk) = self.find_track_mut(tid) {
                trk.arpeggiator.enabled = true;
                trk.arpeggiator.mode = ArpMode::UpDown;
                trk.arpeggiator.rate_beats = 0.25;
                trk.arpeggiator.octaves = 2;
                let mut wt = FxSlot::new(FxKind::WavetableSynth);
                wt.set_param(0, 2.0); // Digital FM Bell
                wt.set_param(9, 210.0); // Pluck decay
                wt.set_param(10, 18.0);
                trk.fx_chain.push(wt);
                trk.fx_chain.push(FxSlot::new(FxKind::StereoDelay));
                trk.gain_db = -5.5;
                trk.clips
                    .push(TimelineClip::new_midi("Arp Driver Chords", min_b, clip_len, arp_notes));
            }
            created.push(tid);
            parts.push("Arp");
        }

        let section = beat_range
            .map(|(a, b)| format!(" section {:.0}–{:.0}b", a, b))
            .unwrap_or_default();
        let card = if created.is_empty() {
            format!("Generate Arr: nothing created{section}.")
        } else {
            format!(
                "Generate Arr: created {} ({}){section}. Undo restores prior state.",
                created.len(),
                parts.join("+")
            )
        };
        (created, card)
    }

    /// Snapshot the live console mix without writing a Mix A–D slot.
    pub fn capture_mix_state(&self, name: impl Into<String>) -> MixSnapshot {
        let tracks = self
            .tracks
            .iter()
            .map(|t| TrackMixState {
                track_id: t.id,
                gain_db: t.gain_db,
                pan: t.pan,
                stereo_width: t.stereo_width,
                mute: t.mute,
                solo: t.solo,
                send_gains: t.sends.iter().map(|s| (s.id, s.gain_db, s.muted)).collect(),
                fx_enabled_wet: t
                    .fx_chain
                    .iter()
                    .map(|f| (f.id, f.enabled, f.wet_mix))
                    .collect(),
            })
            .collect();
        let vca_gains = self
            .vca_groups
            .iter()
            .map(|v| (v.id, v.gain_db, v.mute, v.solo))
            .collect();
        MixSnapshot {
            name: name.into(),
            master_gain_db: self.master.gain_db,
            vca_gains,
            tracks,
        }
    }

    /// Absolute mix morph: `value = from + (to - from) * t` (t=0 → from, t=1 → to).
    pub fn apply_lerped_mix(&mut self, from: &MixSnapshot, to: &MixSnapshot, t: f32) {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        self.master.gain_db = lerp(from.master_gain_db, to.master_gain_db);
        for (vid, to_gdb, to_m, to_s) in &to.vca_gains {
            let from_gdb = from
                .vca_gains
                .iter()
                .find(|(id, ..)| id == vid)
                .map(|(_, g, ..)| *g)
                .unwrap_or(*to_gdb);
            if let Some(vg) = self.vca_groups.iter_mut().find(|v| v.id == *vid) {
                vg.gain_db = lerp(from_gdb, *to_gdb);
                if t > 0.5 {
                    vg.mute = *to_m;
                    vg.solo = *to_s;
                }
            }
        }
        for ts in &to.tracks {
            let Some(from_ts) = from.tracks.iter().find(|f| f.track_id == ts.track_id) else {
                continue;
            };
            if let Some(trk) = self.tracks.iter_mut().find(|t| t.id == ts.track_id) {
                trk.gain_db = lerp(from_ts.gain_db, ts.gain_db);
                trk.pan = lerp(from_ts.pan, ts.pan);
                trk.stereo_width = lerp(from_ts.stereo_width, ts.stereo_width);
                if t > 0.5 {
                    trk.mute = ts.mute;
                    trk.solo = ts.solo;
                }
            }
        }
    }


    pub fn add_vca_group(&mut self, name: impl Into<String>) -> Uuid {
        let palette = [
            [235, 118, 60],
            [66, 196, 176],
            [235, 184, 72],
            [158, 118, 232],
        ];
        let col = palette[self.vca_groups.len() % palette.len()];
        let g = VcaGroup::new(name, col);
        let id = g.id;
        self.vca_groups.push(g);
        id
    }

    pub fn remove_vca_group(&mut self, vca_id: Uuid) {
        self.vca_groups.retain(|v| v.id != vca_id);
        for t in &mut self.tracks {
            if t.vca_group_id == Some(vca_id) {
                t.vca_group_id = None;
            }
        }
    }

    pub fn capture_mix_snapshot(&mut self, slot_idx: usize, label: impl Into<String>) {
        if self.mix_snapshots.len() <= slot_idx {
            self.mix_snapshots.resize_with(slot_idx + 1, || None);
        }
        let tracks = self
            .tracks
            .iter()
            .map(|t| TrackMixState {
                track_id: t.id,
                gain_db: t.gain_db,
                pan: t.pan,
                stereo_width: t.stereo_width,
                mute: t.mute,
                solo: t.solo,
                send_gains: t.sends.iter().map(|s| (s.id, s.gain_db, s.muted)).collect(),
                fx_enabled_wet: t
                    .fx_chain
                    .iter()
                    .map(|f| (f.id, f.enabled, f.wet_mix))
                    .collect(),
            })
            .collect();
        let vca_gains = self
            .vca_groups
            .iter()
            .map(|v| (v.id, v.gain_db, v.mute, v.solo))
            .collect();
        self.mix_snapshots[slot_idx] = Some(MixSnapshot {
            name: label.into(),
            master_gain_db: self.master.gain_db,
            vca_gains,
            tracks,
        });
    }

    pub fn recall_mix_snapshot(&mut self, slot_idx: usize) -> bool {
        let Some(Some(snap)) = self.mix_snapshots.get(slot_idx).cloned() else {
            return false;
        };
        self.master.gain_db = snap.master_gain_db;
        for (vid, gdb, vm, vs) in &snap.vca_gains {
            if let Some(vg) = self.vca_groups.iter_mut().find(|v| v.id == *vid) {
                vg.gain_db = *gdb;
                vg.mute = *vm;
                vg.solo = *vs;
            }
        }
        for ts in &snap.tracks {
            if let Some(trk) = self.tracks.iter_mut().find(|t| t.id == ts.track_id) {
                trk.gain_db = ts.gain_db;
                trk.pan = ts.pan;
                trk.stereo_width = ts.stereo_width;
                trk.mute = ts.mute;
                trk.solo = ts.solo;
                for &(sid, sg, sm) in &ts.send_gains {
                    if let Some(snd) = trk.sends.iter_mut().find(|s| s.id == sid) {
                        snd.gain_db = sg;
                        snd.muted = sm;
                    }
                }
                for &(fid, fen, fwet) in &ts.fx_enabled_wet {
                    if let Some(fx) = trk.fx_chain.iter_mut().find(|f| f.id == fid) {
                        fx.enabled = fen;
                        fx.wet_mix = fwet;
                    }
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub action_label: String,
    pub snapshot: Project,
}

#[derive(Debug, Default)]
pub struct HistoryManager {
    undo_stack: VecDeque<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
    max_entries: usize,
}

impl HistoryManager {
    pub fn new(max_entries: usize) -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            max_entries: max_entries.max(8),
        }
    }

    pub fn record(&mut self, label: impl Into<String>, before_state: Project) {
        if self.undo_stack.len() >= self.max_entries {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(HistoryEntry {
            action_label: label.into(),
            snapshot: before_state,
        });
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, current: Project) -> Option<(String, Project)> {
        let entry = self.undo_stack.pop_back()?;
        let label = entry.action_label.clone();
        self.redo_stack.push(HistoryEntry {
            action_label: label.clone(),
            snapshot: current,
        });
        Some((label, entry.snapshot))
    }

    pub fn redo(&mut self, current: Project) -> Option<(String, Project)> {
        let entry = self.redo_stack.pop()?;
        let label = entry.action_label.clone();
        self.undo_stack.push_back(HistoryEntry {
            action_label: label.clone(),
            snapshot: current,
        });
        Some((label, entry.snapshot))
    }

    pub fn last_undo_label(&self) -> Option<&str> {
        self.undo_stack.back().map(|e| e.action_label.as_str())
    }

    pub fn last_redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|e| e.action_label.as_str())
    }
}

/// Built-in Studio Loop & One-Shot Generator for the Dockable Media Browser!
pub fn synthesize_browser_sample(kind_idx: usize, sample_rate: u32, bpm: f64) -> (String, AudioBuffer) {
    let sr = sample_rate.max(44_100);
    let spb = 60.0 / bpm.max(60.0);
    match kind_idx {
        0 => {
            // Warm 808 Sub Glide One-Shot / 4-Beat Phrase
            let frames = (4.0 * spb * sr as f64) as usize;
            let mut l = vec![0.0_f32; frames];
            for (i, s) in l.iter_mut().enumerate() {
                let t = i as f32 / sr as f32;
                let freq = 48.0 + 92.0 * (-t * 18.0).exp();
                let env = (-t * 1.9).exp() * (t / 0.004).min(1.0);
                *s = ((std::f32::consts::TAU * freq * t).sin() * 1.35).tanh() * env * 0.75;
            }
            ("808 Oxide Sub Drop.wav".into(), AudioBuffer::from_stereo(sr, l.clone(), l, None))
        }
        1 => {
            // Crisp Transient Breakbeat Loop (4 Beats) — great for testing Slice Sampler & Warp Markers!
            let frames = (4.0 * spb * sr as f64) as usize;
            let mut l = vec![0.0_f32; frames];
            let mut r = vec![0.0_f32; frames];
            let hits = [
                (0.0, 0),
                (0.5, 2),
                (1.0, 1),
                (1.5, 2),
                (1.75, 1),
                (2.25, 0),
                (2.5, 0),
                (3.0, 1),
                (3.5, 2),
            ];
            let mut rng = 0x1234_5678_u32;
            for (beat, htype) in hits {
                let start = (beat * spb * sr as f64) as usize;
                let dur = (0.35 * sr as f64) as usize;
                for k in 0..dur {
                    if start + k >= frames {
                        break;
                    }
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let noise = (rng as i32 as f32) / (i32::MAX as f32);
                    let t = k as f32 / sr as f32;
                    let sig = match htype {
                        0 => (std::f32::consts::TAU * (55.0 + 110.0 * (-t * 36.0).exp()) * t).sin() * (-t * 9.0).exp() * 0.8,
                        1 => ((std::f32::consts::TAU * 195.0 * t).sin() * 0.45 + noise * 0.55) * (-t * 15.0).exp() * 0.7,
                        _ => noise * (-t * 38.0).exp() * 0.35,
                    };
                    l[start + k] += sig;
                    r[start + k] += sig * (if htype == 2 { 1.1 } else { 0.96 });
                }
            }
            ("Crisp Studio Breakbeat (4b).wav".into(), AudioBuffer::from_stereo(sr, l, r, None))
        }
        2 => {
            // Sung Vocal Vowel Formant Phrase (slightly detuned so Vocal Auto-Pitch shines!)
            let frames = (8.0 * spb * sr as f64) as usize;
            let mut l = vec![0.0_f32; frames];
            let notes = [(0.0, 222.5_f32, 2.0), (2.0, 263.8, 2.0), (4.0, 295.0, 2.0), (6.0, 245.2, 2.0)];
            let mut ph = 0.0_f32;
            for (b0, f0, len_b) in notes {
                let s0 = (b0 * spb * sr as f64) as usize;
                let n = (len_b * spb * sr as f64) as usize;
                for k in 0..n {
                    if s0 + k >= frames {
                        break;
                    }
                    let t = k as f32 / sr as f32;
                    let vibrato = 1.0 + 0.008 * (std::f32::consts::TAU * 5.2 * t).sin();
                    ph = (ph + (f0 * vibrato) / sr as f32).fract();
                    // Rich glottal-like vocal formant harmonics ("Ahh")
                    let p = std::f32::consts::TAU * ph;
                    let v = p.sin() * 0.50 + (2.0 * p).sin() * 0.28 + (3.0 * p).sin() * 0.16 + (4.0 * p).sin() * 0.07;
                    let env = (t / 0.03).min(1.0) * ((len_b as f32 * spb as f32 - t) / 0.06).clamp(0.0, 1.0);
                    l[s0 + k] += v * env * 0.55;
                }
            }
            ("Raw Vocal Hook (Try Auto-Pitch!).wav".into(), AudioBuffer::from_stereo(sr, l.clone(), l, None))
        }
        _ => {
            let buf = synthesize_guitar_bed(sr, bpm, 1);
            ("Clean Midwest Arpeggio Riff.wav".into(), buf)
        }
    }
}

pub fn build_demo_project(sample_rate: u32) -> Project {
    let sr = sample_rate.max(44_100);
    let mut proj = Project::new_empty("Afterglow Copper — Late-Night Studio Session");
    proj.bpm = 96.0;
    proj.root_pitch_class = 9; // A
    proj.musical_scale = MusicalScale::Dorian;
    proj.loop_enabled = true;
    proj.loop_start_beat = 32.0; // Verse → Chorus writing loop
    proj.loop_end_beat = 96.0;
    proj.show_chord_lane = true;
    proj.show_conductor_lane = true;

    // Gentle lift into the chorus, then settle for the outro.
    proj.tempo_map = vec![
        TempoPoint::new(0.0, 96.0, false),
        TempoPoint::new(64.0, 96.0, true),
        TempoPoint::new(66.0, 98.5, false),
        TempoPoint::new(96.0, 98.5, true),
        TempoPoint::new(112.0, 94.0, false),
    ];

    let n_scenes = proj.scenes.len();
    let song_beats = 128.0; // 32 bars

    // ── Markers ────────────────────────────────────────────────────────────
    proj.add_marker("Intro", 0.0);
    proj.add_marker("Verse", 32.0);
    proj.add_marker("Chorus", 64.0);
    proj.add_marker("Break", 96.0);
    proj.add_marker("Outro", 112.0);

    // ── Chord track (Am9 – Fmaj9 – Cmaj7 – Em7) × 8 cycles ─────────────────
    let cycle: [(u8, ChordQuality); 4] = [
        (9, ChordQuality::Min9),
        (5, ChordQuality::Maj9),
        (0, ChordQuality::Maj7),
        (4, ChordQuality::Min7),
    ];
    proj.chord_track.clear();
    for bar in 0..32 {
        let (root, qual) = cycle[bar % 4];
        proj.chord_track
            .push(ChordEvent::new((bar * 4) as f64, 4.0, root, qual));
    }

    // Helper: section-aware drum groove
    let build_kit_notes = || {
        let mut notes = Vec::new();
        for bar in 0..32 {
            let b = (bar * 4) as f64;
            let section = match bar {
                0..=7 => 0,    // Intro
                8..=15 => 1,   // Verse
                16..=23 => 2,  // Chorus
                24..=27 => 3,  // Break
                _ => 4,        // Outro
            };

            // Kick patterns
            match section {
                0 if bar >= 4 => {
                    notes.push(MidiNote::new(36, 100, b, 0.35));
                    notes.push(MidiNote::new(36, 88, b + 2.5, 0.3));
                }
                1 => {
                    notes.push(MidiNote::new(36, 118, b, 0.4));
                    notes.push(MidiNote::new(36, 96, b + 1.75, 0.28));
                    notes.push(MidiNote::new(36, 110, b + 2.5, 0.35));
                }
                2 => {
                    notes.push(MidiNote::new(36, 122, b, 0.4));
                    notes.push(MidiNote::new(36, 102, b + 1.5, 0.3));
                    notes.push(MidiNote::new(36, 112, b + 2.5, 0.35));
                    notes.push(MidiNote::new(36, 90, b + 3.25, 0.25));
                }
                3 => {
                    notes.push(MidiNote::new(36, 85, b + 2.0, 0.3));
                }
                4 => {
                    notes.push(MidiNote::new(36, 118, b, 0.4));
                    notes.push(MidiNote::new(36, 108, b + 2.5, 0.35));
                    if bar == 31 {
                        notes.push(MidiNote::new(36, 120, b + 3.0, 0.5));
                        notes.push(MidiNote::new(36, 100, b + 3.5, 0.4));
                    }
                }
                _ => {}
            }

            // Snare / clap backbeat
            if section != 0 && section != 3 {
                notes.push(MidiNote::new(38, if section == 2 { 118 } else { 108 }, b + 1.0, 0.35));
                notes.push(MidiNote::new(38, if section == 2 { 120 } else { 112 }, b + 3.0, 0.35));
                // Ghosts
                notes.push(MidiNote::new(38, 48, b + 0.75, 0.12));
                notes.push(MidiNote::new(38, 42, b + 2.75, 0.12));
                if section == 2 {
                    notes.push(MidiNote::new(39, 96, b + 1.0, 0.2)); // clap layer
                }
            } else if section == 0 && bar >= 2 {
                notes.push(MidiNote::new(38, 70, b + 3.0, 0.25));
            } else if section == 3 {
                notes.push(MidiNote::new(38, 64, b + 3.0, 0.2));
            }

            // Hats
            for step in 0..8 {
                let hb = b + step as f64 * 0.5;
                let even = step % 2 == 0;
                match section {
                    0 => {
                        if even {
                            notes.push(MidiNote::new(42, 62 + step as u8 * 2, hb, 0.18));
                        }
                    }
                    1 => {
                        let vel = if even { 88 } else { 58 };
                        notes.push(MidiNote::new(42, vel, hb, 0.16));
                        if step == 7 && bar % 2 == 1 {
                            notes.push(MidiNote::new(46, 92, hb, 0.45));
                        }
                    }
                    2 => {
                        if step == 2 || step == 6 {
                            notes.push(MidiNote::new(46, 100, hb, 0.5));
                        } else {
                            notes.push(MidiNote::new(42, if even { 96 } else { 70 }, hb, 0.15));
                        }
                    }
                    3 => {
                        if even {
                            notes.push(MidiNote::new(42, 55, hb, 0.2));
                        }
                    }
                    _ => {
                        notes.push(MidiNote::new(42, if even { 90 } else { 64 }, hb, 0.15));
                        if bar == 31 && step >= 4 {
                            notes.push(MidiNote::new(46, 105, hb, 0.4));
                        }
                    }
                }
            }

            // Fills into section boundaries
            if bar == 7 || bar == 15 || bar == 23 || bar == 27 {
                notes.push(MidiNote::new(38, 90, b + 3.5, 0.2));
                notes.push(MidiNote::new(45, 100, b + 3.25, 0.2)); // tom
                notes.push(MidiNote::new(47, 105, b + 3.75, 0.25));
            }
        }
        notes
    };

    // 1. Kit
    let mut drums = Track::new("01 Pocket Kit", 0, n_scenes);
    drums.gain_db = -2.0;
    drums.fx_chain.push(FxSlot::new(FxKind::DrumKit));
    let mut drum_comp = FxSlot::new(FxKind::Compressor);
    drum_comp.set_param(0, -18.0);
    drum_comp.set_param(1, 3.2);
    drum_comp.set_param(2, 4.0);
    drum_comp.set_param(3, 140.0);
    drum_comp.set_param(5, 1.5);
    drums.fx_chain.push(drum_comp);
    drums.groove_template = GrooveTemplate::LaidBackNeoSoul;
    let drums_id = drums.id;
    let kit_notes = build_kit_notes();
    drums.clips.push(TimelineClip::new_midi(
        "Full Arrangement Kit",
        0.0,
        song_beats,
        kit_notes.clone(),
    ));
    // Session clips: verse pocket + chorus drive
    let verse_kit: Vec<_> = kit_notes
        .iter()
        .filter(|n| n.start_beat >= 32.0 && n.start_beat < 64.0)
        .map(|n| {
            MidiNote::new(
                n.pitch,
                n.velocity,
                n.start_beat - 32.0,
                n.length_beats,
            )
        })
        .collect();
    let chorus_kit: Vec<_> = kit_notes
        .iter()
        .filter(|n| n.start_beat >= 64.0 && n.start_beat < 96.0)
        .map(|n| {
            MidiNote::new(
                n.pitch,
                n.velocity,
                n.start_beat - 64.0,
                n.length_beats,
            )
        })
        .collect();
    drums.session_slots[0] = Some(SessionSlot::new_midi("Verse Pocket", 32.0, verse_kit));
    drums.session_slots[1] = Some(SessionSlot::new_midi("Chorus Drive", 32.0, chorus_kit));

    // 2. Sidechained Sub Bass
    let mut bass = Track::new("02 Sidechain Sub", 1, n_scenes);
    bass.gain_db = -3.0;
    let mut bass_synth = FxSlot::new(FxKind::PolySynth);
    bass_synth.set_param(0, 0.0); // saw-ish / default wave
    bass_synth.set_param(1, 62.0); // sub detune
    bass_synth.set_param(2, 420.0); // dark cutoff
    bass_synth.set_param(3, 0.22);
    bass_synth.set_param(4, 35.0);
    bass_synth.set_param(5, 3.0);
    bass_synth.set_param(6, 180.0);
    bass_synth.set_param(7, 55.0);
    bass_synth.set_param(8, 90.0);
    bass_synth.set_param(9, -2.0);
    bass.fx_chain.push(bass_synth);
    let mut sc_comp = FxSlot::new(FxKind::Compressor);
    sc_comp.preset_name = "Kick Duck".into();
    sc_comp.sidechain_source = Some(drums_id);
    sc_comp.set_param(0, -24.0);
    sc_comp.set_param(1, 6.0);
    sc_comp.set_param(2, 1.5);
    sc_comp.set_param(3, 160.0);
    sc_comp.set_param(5, 2.0);
    bass.fx_chain.push(sc_comp);

    // Bass roots for Am9 Fmaj9 Cmaj7 Em7 — with chorus octave jumps
    let bass_roots = [45u8, 41, 36, 40]; // A2 F2 C2 E2
    let mut bass_notes = Vec::new();
    for bar in 0..32 {
        let b = (bar * 4) as f64;
        let root = bass_roots[bar % 4];
        let section = match bar {
            0..=3 => 0,
            4..=7 => 1,
            8..=15 => 2,
            16..=23 => 3,
            24..=27 => 4,
            _ => 5,
        };
        match section {
            0 => {} // silent intro first half
            1 => {
                bass_notes.push(MidiNote::new(root, 90, b, 3.5));
            }
            2 => {
                bass_notes.push(MidiNote::new(root, 112, b, 0.7));
                bass_notes.push(MidiNote::new(root, 88, b + 1.0, 0.4));
                bass_notes.push(MidiNote::new(root + 12, 96, b + 1.5, 0.35));
                bass_notes.push(MidiNote::new(root, 108, b + 2.5, 0.7));
                bass_notes.push(MidiNote::new(root + 7, 86, b + 3.5, 0.35));
            }
            3 => {
                bass_notes.push(MidiNote::new(root, 118, b, 0.65));
                bass_notes.push(MidiNote::new(root + 12, 102, b + 1.0, 0.4));
                bass_notes.push(MidiNote::new(root, 110, b + 1.5, 0.4));
                bass_notes.push(MidiNote::new(root + 12, 108, b + 2.5, 0.45));
                bass_notes.push(MidiNote::new(root + 7, 94, b + 3.25, 0.35));
                bass_notes.push(MidiNote::new(root, 100, b + 3.75, 0.2));
            }
            4 => {
                bass_notes.push(MidiNote::new(root, 80, b + 0.0, 1.8));
                bass_notes.push(MidiNote::new(root + 7, 70, b + 2.5, 1.0));
            }
            _ => {
                bass_notes.push(MidiNote::new(root, 115, b, 0.7));
                bass_notes.push(MidiNote::new(root + 12, 100, b + 1.5, 0.4));
                bass_notes.push(MidiNote::new(root, 110, b + 2.5, 0.8));
            }
        }
    }
    bass.clips.push(TimelineClip::new_midi(
        "Oxide Sub Line",
        0.0,
        song_beats,
        bass_notes.clone(),
    ));
    bass.session_slots[1] = Some(SessionSlot::new_midi("Verse Bass", 32.0, {
        bass_notes
            .iter()
            .filter(|n| n.start_beat >= 32.0 && n.start_beat < 64.0)
            .map(|n| MidiNote::new(n.pitch, n.velocity, n.start_beat - 32.0, n.length_beats))
            .collect()
    }));

    // 3. Harmonic folder bus
    let mut bus_folder = Track::new("03 Harmonic Bus", 7, n_scenes);
    bus_folder.gain_db = -1.0;
    let mut bus_eq = FxSlot::new(FxKind::ParametricEq);
    bus_eq.set_param(0, 80.0);
    bus_eq.set_param(10, 1.5); // slight air
    bus_folder.fx_chain.push(bus_eq);
    let mut bus_ott = FxSlot::new(FxKind::MultibandOtt);
    bus_ott.set_param(0, 28.0);
    bus_folder.fx_chain.push(bus_ott);
    let folder_id = bus_folder.id;

    // 4. Wide cinema pad (FerricWave)
    let mut pad = Track::new("04 Cinema Pad", 4, n_scenes);
    pad.parent_folder_id = Some(folder_id);
    pad.gain_db = -8.5;
    pad.pan = -0.15;
    pad.stereo_width = 1.35;
    let mut wt = FxSlot::new(FxKind::WavetableSynth);
    wt.set_param(0, 1.0); // formant vocal bank
    wt.set_param(1, 38.0);
    wt.set_param(2, 5.0); // unison
    wt.set_param(3, 32.0);
    wt.set_param(4, 42.0); // grain cloud
    wt.set_param(6, 2800.0);
    wt.set_param(8, 420.0); // slow attack
    wt.set_param(9, 800.0);
    wt.set_param(10, 78.0);
    wt.set_param(11, 1200.0);
    pad.fx_chain.push(wt);
    let mut pad_notes = Vec::new();
    for bar in 0..32 {
        let b = (bar * 4) as f64;
        let ch = &proj.chord_track[bar];
        let pitches = ch.voiced_pitches(VoicingStyle::OpenSpreadPad, 60);
        let vel = if bar < 8 {
            62
        } else if (16..24).contains(&bar) {
            84
        } else if (24..28).contains(&bar) {
            70
        } else {
            76
        };
        for &p in &pitches {
            pad_notes.push(MidiNote::new(p, vel, b + 0.02, 3.85));
        }
    }
    pad.clips.push(TimelineClip::new_midi(
        "Wide Afterglow Pads",
        0.0,
        song_beats,
        pad_notes,
    ));

    // 5. Neo-soul chord stabs (PolySynth)
    let mut keys = Track::new("05 Neo-Soul Keys", 2, n_scenes);
    keys.parent_folder_id = Some(folder_id);
    keys.gain_db = -5.5;
    keys.pan = 0.12;
    let mut poly = FxSlot::new(FxKind::PolySynth);
    poly.set_param(0, 1.0); // triangle-ish / warmer
    poly.set_param(1, 22.0);
    poly.set_param(2, 2400.0);
    poly.set_param(3, 0.32);
    poly.set_param(4, 55.0);
    poly.set_param(5, 8.0);
    poly.set_param(6, 420.0);
    poly.set_param(7, 58.0);
    poly.set_param(8, 380.0);
    let poly_id = poly.id;
    keys.fx_chain.push(poly);
    let mut tape = FxSlot::new(FxKind::TapeSaturator);
    tape.set_param(0, 4.5);
    tape.set_param(1, 48.0);
    keys.fx_chain.push(tape);

    let mut lfo = ModulatorSlot::new_lfo("Breath Filter");
    lfo.kind = ModulatorKind::Lfo {
        shape: LfoShape::Sine,
        rate_hz: 0.35,
        tempo_sync: true,
        sync_beats: 8.0,
        bipolar: true,
    };
    lfo.routings.push(ModRouting {
        target: ModTarget::FxParam {
            fx_id: poly_id,
            param_index: 2,
        },
        depth: 0.14,
    });
    keys.modulators.push(lfo);

    let mut flt_lane = AutomationLane::new(AutomationTarget::FxParam {
        fx_id: poly_id,
        param_index: 2,
    });
    flt_lane.points = vec![
        AutomationPoint::new(0.0, 0.18, 0.3),
        AutomationPoint::new(32.0, 0.28, 0.2),
        AutomationPoint::new(64.0, 0.55, -0.35),
        AutomationPoint::new(96.0, 0.22, 0.25),
        AutomationPoint::new(128.0, 0.15, 0.0),
    ];
    keys.automation_lanes.push(flt_lane);
    keys.show_automation = true;

    let mut chord_notes = Vec::new();
    for bar in 4..32 {
        // enter after intro pad swell
        let b = (bar * 4) as f64;
        let ch = &proj.chord_track[bar];
        let pitches = ch.voiced_pitches(VoicingStyle::Drop2Jazz, 60);
        let chorus = (16..24).contains(&bar) || bar >= 28;
        if chorus {
            for (i, &p) in pitches.iter().enumerate() {
                chord_notes.push(MidiNote::new(p, 92, b + i as f64 * 0.02, 1.6));
                chord_notes.push(MidiNote::new(p, 78, b + 2.0 + i as f64 * 0.02, 1.5));
            }
            if let Some(&top) = pitches.last() {
                chord_notes.push(MidiNote::new(top + 5, 70, b + 3.0, 0.7));
            }
        } else {
            for &p in &pitches {
                chord_notes.push(MidiNote::new(p, 78, b + 0.05, 3.6));
            }
        }
    }
    keys.clips.push(TimelineClip::new_midi(
        "Drop-2 Keys",
        0.0,
        song_beats,
        chord_notes.clone(),
    ));
    keys.session_slots[0] = Some(SessionSlot::new_midi("Keys Loop", 16.0, {
        chord_notes
            .iter()
            .filter(|n| n.start_beat >= 32.0 && n.start_beat < 48.0)
            .map(|n| MidiNote::new(n.pitch, n.velocity, n.start_beat - 32.0, n.length_beats))
            .collect()
    }));

    // 6. Hook lead (chorus + outro) — follows chord track for live rewriting demos
    let mut lead = Track::new("06 Hook Lead", 3, n_scenes);
    lead.parent_folder_id = Some(folder_id);
    lead.gain_db = -6.5;
    lead.pan = 0.28;
    lead.follow_chord_track = true;
    let mut lead_wt = FxSlot::new(FxKind::WavetableSynth);
    lead_wt.set_param(0, 2.0); // Digital FM
    lead_wt.set_param(1, 55.0);
    lead_wt.set_param(2, 3.0);
    lead_wt.set_param(3, 18.0);
    lead_wt.set_param(4, 18.0);
    lead_wt.set_param(6, 5200.0);
    lead_wt.set_param(8, 12.0);
    lead_wt.set_param(9, 220.0);
    lead_wt.set_param(10, 45.0);
    lead_wt.set_param(11, 280.0);
    lead.fx_chain.push(lead_wt);
    let mut lead_delay = FxSlot::new(FxKind::StereoDelay);
    lead_delay.wet_mix = 0.28;
    lead_delay.set_param(1, 3.0); // dotted sync-ish
    lead_delay.set_param(2, 32.0);
    lead_delay.set_param(3, 70.0);
    lead.fx_chain.push(lead_delay);

    // Melodic motif in A Dorian (will snap if chords change via follow)
    let motif: [(f64, u8, f64, u8); 8] = [
        (0.0, 72, 0.7, 100),   // A4
        (1.0, 74, 0.45, 88),   // B
        (1.5, 76, 0.9, 96),    // C
        (2.5, 79, 0.7, 102),   // E
        (3.5, 76, 0.4, 86),    // C
        (4.0, 74, 0.7, 90),    // B
        (5.0, 72, 0.9, 94),    // A
        (6.0, 69, 1.5, 88),    // F
    ];
    let mut lead_notes = Vec::new();
    for start_bar in [16usize, 20, 28] {
        let base = (start_bar * 4) as f64;
        for &(off, pitch, dur, vel) in &motif {
            lead_notes.push(MidiNote::new(pitch, vel, base + off, dur));
        }
    }
    // Intro whisper echo of motif (high + quiet)
    for &(off, pitch, dur, _) in &motif[..4] {
        lead_notes.push(MidiNote::new(pitch + 12, 48, 24.0 + off, dur));
    }
    lead.clips.push(TimelineClip::new_midi(
        "Copper Hook",
        0.0,
        song_beats,
        lead_notes,
    ));

    // 7. Shimmer arp (verse/chorus)
    let mut arp = Track::new("07 Shimmer Arp", 5, n_scenes);
    arp.parent_folder_id = Some(folder_id);
    arp.gain_db = -10.0;
    arp.pan = -0.32;
    arp.follow_chord_track = true;
    let mut arp_syn = FxSlot::new(FxKind::PolySynth);
    arp_syn.set_param(0, 2.0);
    arp_syn.set_param(1, 15.0);
    arp_syn.set_param(2, 4800.0);
    arp_syn.set_param(3, 0.4);
    arp_syn.set_param(5, 2.0);
    arp_syn.set_param(6, 120.0);
    arp_syn.set_param(7, 18.0);
    arp_syn.set_param(8, 140.0);
    arp.fx_chain.push(arp_syn);
    arp.arpeggiator.enabled = true;
    arp.arpeggiator.mode = ArpMode::UpDown;
    arp.arpeggiator.rate_beats = 0.25;
    arp.arpeggiator.gate = 0.55;
    let mut arp_notes = Vec::new();
    for bar in 8..28 {
        let b = (bar * 4) as f64;
        let ch = &proj.chord_track[bar];
        let pitches = ch.voiced_pitches(VoicingStyle::ClosePiano, 72);
        for &p in pitches.iter().take(3) {
            arp_notes.push(MidiNote::new(p, 70, b, 3.8));
        }
    }
    arp.clips.push(TimelineClip::new_midi(
        "Arp Drivers",
        0.0,
        song_beats,
        arp_notes,
    ));

    // 8. Multi-take DI guitar bed (improved synthesis) under chorus
    let mut guitar = Track::new("08 DI Comp Guitar", 6, n_scenes);
    guitar.parent_folder_id = Some(folder_id);
    guitar.gain_db = -7.0;
    guitar.pan = -0.2;
    guitar.input_source = InputSource::AudioMono { channel: 0 };
    guitar.fx_chain.push(FxSlot::new(FxKind::ChromaticTuner));
    let mut amp = FxSlot::new(FxKind::AmpCabSim);
    amp.set_param(0, 8.5);
    amp.set_param(1, 1.0);
    amp.set_param(6, -5.0);
    guitar.fx_chain.push(amp);
    let take1 = synthesize_guitar_bed(sr, proj.bpm, 0);
    let take2 = synthesize_guitar_bed(sr, proj.bpm, 1);
    let take3 = synthesize_guitar_bed(sr, proj.bpm, 2);
    let mut gtr_clip = TimelineClip::new_audio("Chorus Comp Bed", 64.0, 32.0, take1.clone());
    gtr_clip.takes[0].name = "Take 1 — Warm Strum".into();
    gtr_clip.takes[0].rating = 4;
    let mut t2 = Take::new_audio("Take 2 — Octave Arp", take2);
    t2.rating = 5;
    let mut t3 = Take::new_audio("Take 3 — High Double", take3);
    t3.rating = 4;
    gtr_clip.takes.push(t2);
    gtr_clip.takes.push(t3);
    gtr_clip.show_take_lanes = true;
    gtr_clip.set_comp_region(0.0, 8.0, 0);
    gtr_clip.set_comp_region(8.0, 16.0, 1);
    gtr_clip.set_comp_region(16.0, 24.0, 0);
    gtr_clip.set_comp_region(24.0, 32.0, 2);
    guitar.clips.push(gtr_clip);
    guitar.session_slots[0] = Some(SessionSlot::new_audio("Clean Riff", 16.0, take1));

    // 9. Plate verb aux
    let mut verb_bus = Track::new("09 Plate Aux", 5, n_scenes);
    verb_bus.gain_db = -1.5;
    let mut reverb = FxSlot::new(FxKind::PlateHallReverb);
    reverb.wet_mix = 1.0;
    reverb.set_param(0, 28.0);
    reverb.set_param(1, 78.0);
    reverb.set_param(2, 42.0);
    reverb.set_param(3, 160.0);
    verb_bus.fx_chain.push(reverb);
    let mut conv = FxSlot::new(FxKind::ConvolutionIr);
    conv.wet_mix = 0.35;
    conv.set_param(0, 1.0); // EMT plate
    verb_bus.fx_chain.push(conv);
    let verb_id = verb_bus.id;

    // 10. Delay aux
    let mut delay_bus = Track::new("10 Tape Delay Aux", 8, n_scenes);
    delay_bus.gain_db = -2.0;
    let mut delay = FxSlot::new(FxKind::StereoDelay);
    delay.wet_mix = 1.0;
    delay.set_param(1, 2.0);
    delay.set_param(2, 42.0);
    delay.set_param(3, 80.0);
    delay.set_param(5, 22.0);
    delay_bus.fx_chain.push(delay);
    let delay_id = delay_bus.id;

    drums.sends.push(TrackSend::new(verb_id, -16.0));
    pad.sends.push(TrackSend::new(verb_id, -6.0));
    keys.sends.push(TrackSend::new(verb_id, -8.5));
    lead.sends.push(TrackSend::new(verb_id, -9.0));
    lead.sends.push(TrackSend::new(delay_id, -7.5));
    arp.sends.push(TrackSend::new(delay_id, -11.0));
    guitar.sends.push(TrackSend::new(verb_id, -10.0));

    // VCAs
    let rhythm_vca = VcaGroup::new("Rhythm VCA", [235, 118, 60]);
    let harmony_vca = VcaGroup::new("Harmony VCA", [66, 196, 176]);
    drums.vca_group_id = Some(rhythm_vca.id);
    bass.vca_group_id = Some(rhythm_vca.id);
    pad.vca_group_id = Some(harmony_vca.id);
    keys.vca_group_id = Some(harmony_vca.id);
    lead.vca_group_id = Some(harmony_vca.id);
    arp.vca_group_id = Some(harmony_vca.id);
    guitar.vca_group_id = Some(harmony_vca.id);
    proj.vca_groups.push(rhythm_vca);
    proj.vca_groups.push(harmony_vca);

    proj.tracks = vec![
        drums, bass, bus_folder, pad, keys, lead, arp, guitar, verb_bus, delay_bus,
    ];

    // Mix snapshots: Balanced / Lead-forward / Stripped
    proj.capture_mix_snapshot(0, "Mix A — Balanced");
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("06")) {
        t.gain_db = -3.5;
    }
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("05")) {
        t.gain_db = -7.5;
    }
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("04")) {
        t.gain_db = -10.0;
    }
    proj.capture_mix_snapshot(1, "Mix B — Lead Forward");
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("06")) {
        t.gain_db = -6.5;
    }
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("05")) {
        t.gain_db = -5.5;
    }
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("04")) {
        t.gain_db = -8.5;
    }
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("07")) {
        t.mute = true;
    }
    if let Some(t) = proj.tracks.iter_mut().find(|t| t.name.starts_with("08")) {
        t.mute = true;
    }
    proj.capture_mix_snapshot(2, "Mix C — Rhythm + Pad");
    proj.recall_mix_snapshot(0);

    proj.master.gain_db = -1.0;
    proj.master.safety_limiter = true;
    proj
}

/// Beginner tutorial song **Night Current** — mirrors `docs/BEGINNER_GUIDE.md`.
/// Dark techno @ 128 BPM, Cm–Ab–Eb–Bb, 16 bars. Offline bounce of this project is the
/// finished-product demo target shipped as `demo/NightCurrent-Demo.wav`.
pub fn build_tutorial_project(sample_rate: u32) -> Project {
    build_tutorial_project_through(sample_rate, crate::tutorial::NightCurrentStep::Mix)
}

/// Progressive Night Current build for the in-app coach (`through` = last step to apply).
pub fn build_tutorial_project_through(
    sample_rate: u32,
    through: crate::tutorial::NightCurrentStep,
) -> Project {
    use crate::tutorial::NightCurrentStep;
    let _sr = sample_rate.max(44_100);
    let n_scenes = 8usize;
    let mut proj = Project::new_empty("Night Current");
    proj.bpm = 128.0;
    proj.root_pitch_class = 0; // C minor
    proj.musical_scale = MusicalScale::NaturalMinor;
    proj.show_chord_lane = true;
    proj.loop_enabled = true;
    proj.loop_start_beat = 0.0;
    proj.loop_end_beat = 64.0;

    proj.add_marker("Intro", 0.0);
    proj.add_marker("Groove", 8.0);
    proj.add_marker("Hook A", 16.0);
    proj.add_marker("Stack", 32.0);
    proj.add_marker("Hook B", 48.0);

    if through < NightCurrentStep::Chords {
        return proj;
    }

    // B — Chord Lane: Cm → Ab → Eb → Bb × 4
    let cycle: [(u8, ChordQuality); 4] = [
        (0, ChordQuality::Minor),  // Cm
        (8, ChordQuality::Major),  // Ab
        (3, ChordQuality::Major),  // Eb
        (10, ChordQuality::Major), // Bb
    ];
    proj.chord_track.clear();
    for bar in 0..16 {
        let (root, qual) = cycle[bar % 4];
        proj.chord_track
            .push(ChordEvent::new((bar * 4) as f64, 4.0, root, qual));
    }

    if through < NightCurrentStep::Harmony {
        return proj;
    }

    // C — Generate Arr (Pads / Bass / Arp), then rename + section the arp
    let created =
        proj.generate_arrangement_from_chords(VoicingStyle::OpenSpreadPad, true, true, true);
    let mut pads_id = None;
    let mut bass_id = None;
    let mut arp_id = None;
    for tid in created {
        if let Some(t) = proj.find_track_mut(tid) {
            if t.name.contains("Pad") {
                t.name = "03 Pads".into();
                t.gain_db = -11.0;
                t.color = [70, 90, 120];
                pads_id = Some(tid);
            } else if t.name.contains("Bass") {
                t.name = "02 Bass".into();
                t.gain_db = -1.0;
                t.color = [40, 60, 90];
                bass_id = Some(tid);
            } else if t.name.contains("Arp") {
                t.name = "04 Arp".into();
                t.gain_db = -11.5;
                t.color = [90, 70, 130];
                arp_id = Some(tid);
            }
        }
    }

    // Dark bed — keep low/murky timbre, but restore pump + section lift (was too flat)
    let vol_n = |db: f32| ((db + 60.0) / 72.0).clamp(0.0, 1.0);
    let wt_cut_n = |hz: f32| ((hz - 60.0) / 17_940.0).clamp(0.0, 1.0);
    let poly_cut_n = |hz: f32| ((hz - 60.0) / 15_940.0).clamp(0.0, 1.0);

    if let Some(tid) = pads_id {
        if let Some(t) = proj.find_track_mut(tid) {
            t.fx_chain
                .retain(|f| f.kind != FxKind::ConvolutionIr); // hall IR = too glossy
            let mut wt_id = None;
            if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.kind == FxKind::WavetableSynth) {
                fx.set_param(0, 0.0);    // Analog morph
                fx.set_param(1, 28.0);   // slightly more body
                fx.set_param(2, 4.0);    // mild width (not sheen)
                fx.set_param(3, 18.0);
                fx.set_param(4, 10.0);   // tiny grain for motion
                fx.set_param(6, 1400.0); // dark but not mud
                fx.set_param(7, 0.22);
                fx.set_param(8, 45.0);
                wt_id = Some(fx.id);
            }
            let mut eq = FxSlot::new(FxKind::ParametricEq);
            eq.set_param(0, 55.0);
            eq.set_param(2, 2.5);  // low bump
            eq.set_param(10, -4.0); // high shelf down (less than before)
            eq.set_param(11, -1.0);
            t.fx_chain.push(eq);
            let mut tape = FxSlot::new(FxKind::TapeSaturator);
            tape.set_param(0, 5.5);
            tape.set_param(1, 52.0);
            t.fx_chain.push(tape);

            // Breathing LFO on pad cutoff
            if let Some(fx_id) = wt_id {
                let mut lfo = ModulatorSlot::new_lfo("Pad Pump Breath");
                lfo.kind = ModulatorKind::Lfo {
                    shape: LfoShape::Sine,
                    rate_hz: 0.53, // ~1 bar @ 128
                    tempo_sync: true,
                    sync_beats: 4.0,
                    bipolar: true,
                };
                lfo.routings.push(ModRouting {
                    target: ModTarget::FxParam {
                        fx_id,
                        param_index: 6,
                    },
                    depth: 0.10,
                });
                t.modulators.push(lfo);

                // Section filter lift: intro closed → groove opens → stack peaks → hook B
                let mut flt = AutomationLane::new(AutomationTarget::FxParam {
                    fx_id,
                    param_index: 6,
                });
                flt.points = vec![
                    AutomationPoint::new(0.0, wt_cut_n(900.0), 0.2),
                    AutomationPoint::new(8.0, wt_cut_n(1300.0), 0.15),
                    AutomationPoint::new(16.0, wt_cut_n(1600.0), 0.1),
                    AutomationPoint::new(32.0, wt_cut_n(2100.0), -0.2),
                    AutomationPoint::new(48.0, wt_cut_n(1800.0), 0.15),
                    AutomationPoint::new(64.0, wt_cut_n(1200.0), 0.0),
                ];
                t.automation_lanes.push(flt);
            }

            // Volume rides for arrangement contrast
            let mut vol = AutomationLane::new(AutomationTarget::Volume);
            vol.points = vec![
                AutomationPoint::new(0.0, vol_n(-16.0), 0.25),
                AutomationPoint::new(8.0, vol_n(-11.0), 0.1),
                AutomationPoint::new(16.0, vol_n(-10.0), 0.05),
                AutomationPoint::new(32.0, vol_n(-8.5), -0.15),
                AutomationPoint::new(48.0, vol_n(-9.5), 0.1),
                AutomationPoint::new(64.0, vol_n(-12.0), 0.0),
            ];
            t.automation_lanes.push(vol);
            t.show_automation = true;
        }
    }
    if let Some(tid) = bass_id {
        if let Some(t) = proj.find_track_mut(tid) {
            if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.kind == FxKind::PolySynth) {
                fx.set_param(0, 1.0);    // saw
                fx.set_param(1, 35.0);   // sub weight
                fx.set_param(2, 320.0);  // dark lowpass
                fx.set_param(3, 0.28);
                fx.set_param(4, 55.0);   // filter-env bite = movement
                fx.set_param(5, 3.0);    // fast attack
                fx.set_param(6, 160.0);
                fx.set_param(7, 42.0);
                fx.set_param(8, 90.0);
            }
            if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.kind == FxKind::MultibandOtt) {
                fx.set_param(0, 42.0);
                fx.set_param(6, -2.5);
                fx.set_param(4, 3.5); // push low
            }
        }
    }
    if let Some(tid) = arp_id {
        if let Some(t) = proj.find_track_mut(tid) {
            t.fx_chain.retain(|f| f.kind != FxKind::StereoDelay);
            if let Some(fx) = t.fx_chain.iter_mut().find(|f| f.kind == FxKind::WavetableSynth) {
                fx.set_param(0, 0.0);
                fx.set_param(4, 0.0);
                fx.set_param(6, 1500.0);
                fx.set_param(7, 0.30);
                fx.set_param(9, 90.0);
                fx.set_param(10, 28.0);
            }
            let mut eq = FxSlot::new(FxKind::ParametricEq);
            eq.set_param(0, 140.0);
            eq.set_param(10, -5.5);
            t.fx_chain.push(eq);
        }
    }

    // Arp only in Stack + Hook B (bars 9–16) — matches tutorial mute table
    if let Some(tid) = arp_id {
        if let Some(t) = proj.find_track_mut(tid) {
            t.clips.clear();
            let mut notes = Vec::new();
            for bar in 8..16 {
                let local_bar = (bar - 8) as f64;
                let (root, qual) = cycle[bar % 4];
                let ch = ChordEvent::new(0.0, 4.0, root, qual);
                let pitches = ch.voiced_pitches(VoicingStyle::ClosePiano, 48);
                if pitches.is_empty() {
                    continue;
                }
                for s in 0..16 {
                    if s % 4 == 3 {
                        continue;
                    }
                    let pitch = pitches[s % pitches.len()];
                    // Accent downs, ghost offs — more groove than flat 16ths
                    let vel = match s % 4 {
                        0 => 98,
                        1 => 52,
                        2 => 78,
                        _ => 48,
                    };
                    notes.push(MidiNote::new(
                        pitch,
                        vel,
                        local_bar * 4.0 + s as f64 * 0.25,
                        0.14,
                    ));
                }
            }
            t.clips
                .push(TimelineClip::new_midi("Arp Stack", 32.0, 32.0, notes));
        }
    }

    if through < NightCurrentStep::Drums {
        return proj;
    }

    // D — Drums (sparse intro → full techno grid) — punchy, not bright
    let drums_id = {
        let mut drums = Track::new("01 Drums", 0, n_scenes);
        drums.gain_db = 0.5;
        drums.color = [200, 80, 70];
        let mut kit = FxSlot::new(FxKind::DrumKit);
        kit.set_param(0, 92.0); // kick punch
        kit.set_param(1, 44.0); // low kick tune
        kit.set_param(2, 48.0); // controlled snap (not rice)
        kit.set_param(3, 70.0); // short hats
        kit.set_param(4, 12.0); // tiny room glue
        kit.set_param(5, 1.5);
        drums.fx_chain.push(kit);
        let mut notes = Vec::new();
        for bar in 0..16 {
            let b = (bar * 4) as f64;
            let kicks: &[f64] = if bar < 2 {
                &[0.0, 2.0]
            } else {
                &[0.0, 1.0, 2.0, 3.0]
            };
            for &k in kicks {
                let accent = k == 0.0 || k == 2.0;
                let vel = if bar < 2 {
                    108
                } else if accent {
                    127
                } else {
                    112
                };
                notes.push(MidiNote::new(36, vel, b + k, 0.35));
            }
            if bar >= 2 {
                // Clap on 2/4 with velocity lift into Hook/Stack
                let clap_vel = if bar >= 8 { 112 } else { 100 };
                notes.push(MidiNote::new(39, clap_vel, b + 1.0, 0.22));
                notes.push(MidiNote::new(39, clap_vel + 4, b + 3.0, 0.22));
                // Soft snare under for body (not crackle)
                notes.push(MidiNote::new(38, 58, b + 1.0, 0.18));
                notes.push(MidiNote::new(38, 62, b + 3.0, 0.18));
            }
            // Hats: 8ths with offbeat ghosts; open hat accents in Stack
            let hat_steps = if bar < 4 { 4 } else { 8 };
            for s in 0..hat_steps {
                let step = if hat_steps == 4 { 1.0 } else { 0.5 };
                let onbeat = s % 2 == 0;
                let vel = if bar < 4 {
                    if onbeat { 62 } else { 40 }
                } else if onbeat {
                    72
                } else {
                    44
                };
                notes.push(MidiNote::new(42, vel, b + s as f64 * step, 0.09));
            }
            if bar >= 8 && bar % 2 == 1 {
                notes.push(MidiNote::new(46, 70, b + 1.5, 0.18)); // open hat lift
            }
        }
        drums
            .clips
            .push(TimelineClip::new_midi("Techno Grid", 0.0, 64.0, notes));

        // Drum volume swell into Hook B
        let mut vol = AutomationLane::new(AutomationTarget::Volume);
        vol.points = vec![
            AutomationPoint::new(0.0, vol_n(-2.0), 0.1),
            AutomationPoint::new(8.0, vol_n(0.5), 0.05),
            AutomationPoint::new(32.0, vol_n(1.0), 0.0),
            AutomationPoint::new(48.0, vol_n(1.5), 0.0),
            AutomationPoint::new(64.0, vol_n(0.0), 0.0),
        ];
        drums.automation_lanes.push(vol);

        let id = drums.id;
        proj.tracks.insert(0, drums);
        id
    };

    if through < NightCurrentStep::Hook {
        let _ = drums_id;
        return proj;
    }

    // E — Hook lead — dark, but with filter-env bite + section lifts (not flat)
    {
        let mut lead = Track::new("05 Hook", 2, n_scenes);
        lead.gain_db = -4.5;
        lead.color = [220, 140, 60];
        let mut syn = FxSlot::new(FxKind::PolySynth);
        syn.set_param(0, 0.0);    // square
        syn.set_param(1, 22.0);
        syn.set_param(2, 1100.0); // dark but readable
        syn.set_param(3, 0.42);   // resonance = edge without air
        syn.set_param(4, 62.0);   // filter env = note attack movement
        syn.set_param(5, 4.0);
        syn.set_param(6, 140.0);
        syn.set_param(7, 48.0);
        syn.set_param(8, 180.0);
        let syn_id = syn.id;
        lead.fx_chain.push(syn);
        let mut eq = FxSlot::new(FxKind::ParametricEq);
        eq.set_param(0, 85.0);
        eq.set_param(4, 2.0);  // slight mid presence
        eq.set_param(3, 780.0);
        eq.set_param(10, -3.5);
        lead.fx_chain.push(eq);
        let mut delay = FxSlot::new(FxKind::StereoDelay);
        delay.wet_mix = 0.16;
        lead.fx_chain.push(delay);
        let mut rev = FxSlot::new(FxKind::PlateHallReverb);
        rev.wet_mix = 0.10;
        lead.fx_chain.push(rev);

        // Slow cutoff swell into Hook B
        let mut flt = AutomationLane::new(AutomationTarget::FxParam {
            fx_id: syn_id,
            param_index: 2,
        });
        flt.points = vec![
            AutomationPoint::new(16.0, poly_cut_n(950.0), 0.1),
            AutomationPoint::new(24.0, poly_cut_n(1200.0), 0.05),
            AutomationPoint::new(48.0, poly_cut_n(1450.0), -0.1),
            AutomationPoint::new(56.0, poly_cut_n(1100.0), 0.0),
        ];
        lead.automation_lanes.push(flt);

        let mut vol = AutomationLane::new(AutomationTarget::Volume);
        vol.points = vec![
            AutomationPoint::new(16.0, vol_n(-5.0), 0.05),
            AutomationPoint::new(24.0, vol_n(-4.0), 0.0),
            AutomationPoint::new(48.0, vol_n(-3.0), 0.0),
            AutomationPoint::new(56.0, vol_n(-4.5), 0.0),
        ];
        lead.automation_lanes.push(vol);
        lead.show_automation = true;

        // Same contour as the sketch, −12 st, more velocity contrast
        let phrase: [(f64, u8, f64, u8); 11] = [
            (0.0, 48, 0.45, 108),
            (0.5, 43, 0.4, 86),
            (1.0, 46, 0.45, 96),
            (1.5, 48, 0.7, 118),
            (2.0, 51, 0.55, 104),
            (2.5, 48, 0.4, 82),
            (3.0, 43, 0.65, 92),
            (4.0, 44, 0.45, 90),
            (4.5, 46, 0.45, 94),
            (5.0, 48, 0.8, 122),
            (6.0, 43, 1.2, 100),
        ];
        let notes_a: Vec<_> = phrase
            .iter()
            .map(|&(off, pitch, dur, vel)| MidiNote::new(pitch, vel, off, dur))
            .collect();
        lead.clips
            .push(TimelineClip::new_midi("Hook A", 16.0, 8.0, notes_a));
        lead.clips.push(TimelineClip::new_midi(
            "Hook B",
            48.0,
            8.0,
            phrase
                .iter()
                .map(|&(off, pitch, dur, vel)| {
                    MidiNote::new(pitch, (vel as u16 + 8).min(127) as u8, off, dur)
                })
                .collect(),
        ));
        proj.tracks.push(lead);
    }

    if through < NightCurrentStep::Counter {
        let _ = drums_id;
        return proj;
    }

    // F — Counter on Stack (+ soft under Hook B) — darker, with env bite
    {
        let mut counter = Track::new("06 Counter", 3, n_scenes);
        counter.gain_db = -11.0;
        counter.color = [160, 100, 180];
        let mut syn = FxSlot::new(FxKind::PolySynth);
        syn.set_param(0, 0.0);
        syn.set_param(2, 1200.0);
        syn.set_param(3, 0.34);
        syn.set_param(4, 48.0);
        syn.set_param(5, 6.0);
        syn.set_param(6, 200.0);
        syn.set_param(7, 40.0);
        counter.fx_chain.push(syn);
        let mut eq = FxSlot::new(FxKind::ParametricEq);
        eq.set_param(10, -4.5);
        counter.fx_chain.push(eq);
        let phrase: [(f64, u8, f64, u8); 7] = [
            (0.0, 55, 0.4, 92),
            (1.0, 51, 0.4, 74),
            (2.0, 53, 0.5, 88),
            (3.0, 55, 0.9, 100),
            (4.0, 58, 0.55, 90),
            (5.0, 55, 0.4, 72),
            (6.0, 51, 1.0, 86),
        ];
        let notes: Vec<_> = phrase
            .iter()
            .map(|&(o, p, d, v)| MidiNote::new(p, v, o, d))
            .collect();
        counter
            .clips
            .push(TimelineClip::new_midi("Counter Stack", 32.0, 8.0, notes.clone()));
        let soft: Vec<_> = notes
            .iter()
            .map(|n| {
                MidiNote::new(
                    n.pitch,
                    (n.velocity as u16 * 7 / 10) as u8,
                    n.start_beat,
                    n.length_beats,
                )
            })
            .collect();
        counter
            .clips
            .push(TimelineClip::new_midi("Counter Soft", 48.0, 8.0, soft));
        proj.tracks.push(counter);
    }

    if through < NightCurrentStep::Sidechain {
        let _ = drums_id;
        return proj;
    }

    // G/H — hard techno sidechain pads ← drums (pump, not polite)
    if let Some(pid) = pads_id {
        if let Some(t) = proj.find_track_mut(pid) {
            let mut comp = FxSlot::new(FxKind::Compressor);
            comp.preset_name = "Night Current Sidechain".into();
            comp.set_param(0, -28.0); // deeper duck
            comp.set_param(1, 6.0);
            comp.set_param(2, 2.0);   // fast attack
            comp.set_param(3, 180.0); // release ≈ 1 beat @ 128
            comp.set_param(4, 2.0);
            comp.set_param(5, 4.5);   // makeup so ducking reads as pump
            comp.sidechain_source = Some(drums_id);
            t.fx_chain.insert(0, comp);
        }
    }
    // Bass also ducks off kick — classic four-on-floor pump
    if let Some(bid) = bass_id {
        if let Some(t) = proj.find_track_mut(bid) {
            let mut comp = FxSlot::new(FxKind::Compressor);
            comp.preset_name = "Bass Kick Duck".into();
            comp.set_param(0, -26.0);
            comp.set_param(1, 5.0);
            comp.set_param(2, 1.5);
            comp.set_param(3, 140.0);
            comp.set_param(5, 3.5);
            comp.sidechain_source = Some(drums_id);
            t.fx_chain.insert(0, comp);
        }
    }

    if through < NightCurrentStep::Mix {
        return proj;
    }

    // I — Mix A snapshot
    proj.capture_mix_snapshot(0, "Mix A — Night Current tutorial");

    // Master: a bit more tape weight for glue (keep dark, add density)
    if let Some(tape) = proj
        .master
        .fx_chain
        .iter_mut()
        .find(|f| f.kind == FxKind::TapeSaturator)
    {
        tape.set_param(0, 3.8);
        tape.set_param(1, 48.0);
        tape.set_param(2, -0.4);
    }
    if let Some(eq) = proj
        .master
        .fx_chain
        .iter_mut()
        .find(|f| f.kind == FxKind::ParametricEq)
    {
        eq.set_param(2, 1.5);   // low weight
        eq.set_param(10, -1.5); // slightly darker air
    }
    proj.master.gain_db = -0.8;
    proj.master.safety_limiter = true;
    let _ = n_scenes;
    proj
}

/// Improved DI guitar bed: plucked multi-partials with string-ish decay (chorus-length 8 bars).
fn synthesize_guitar_bed(sample_rate: u32, bpm: f64, variation: usize) -> AudioBuffer {
    let spb = 60.0 / bpm.max(20.0);
    let bars = 8.0;
    let total_secs = bars * 4.0 * spb;
    let frames = (total_secs * sample_rate as f64) as usize;
    let mut left = vec![0.0_f32; frames];
    let mut right = vec![0.0_f32; frames];

    // Am9 / Fmaj9 / Cmaj7 / Em7 shapes (same as song cycle)
    let shapes: [[u8; 4]; 4] = [
        [57, 60, 64, 71], // Am9
        [53, 57, 60, 69], // Fmaj9-ish
        [60, 64, 67, 71], // Cmaj7
        [52, 55, 59, 67], // Em7
    ];

    let mut events: Vec<(f64, u8, f64, f32)> = Vec::new();
    for bar in 0..8usize {
        let b = (bar * 4) as f64;
        let chord = shapes[bar % 4];
        match variation {
            0 => {
                for (i, &p) in chord.iter().enumerate() {
                    let stagger = i as f64 * 0.03;
                    events.push((b + stagger, p, 1.7, 0.26));
                    events.push((b + 2.0 + stagger, p, 1.6, 0.24));
                }
            }
            1 => {
                let pattern = [
                    chord[0],
                    chord[1],
                    chord[2],
                    chord[3],
                    chord[2],
                    chord[1],
                    chord[3] + 5,
                    chord[2],
                ];
                for (step, &p) in pattern.iter().enumerate() {
                    events.push((b + step as f64 * 0.5, p, 0.55, 0.30));
                }
            }
            _ => {
                events.push((b, chord[3] + 12, 1.2, 0.28));
                events.push((b + 1.5, chord[2] + 12, 0.8, 0.25));
                events.push((b + 2.5, chord[3] + 12, 1.1, 0.28));
                events.push((b + 3.5, chord[1] + 12, 0.4, 0.22));
            }
        }
    }

    for (start_b, midi, dur_b, amp) in events {
        let start_frame = (start_b * spb * sample_rate as f64) as usize;
        let dur_frames = (dur_b * spb * sample_rate as f64) as usize;
        let freq = 440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0);
        let end_frame = (start_frame + dur_frames).min(frames);
        for (k, idx) in (start_frame..end_frame).enumerate() {
            let t = k as f32 / sample_rate as f32;
            let attack = (t / 0.003).min(1.0);
            let body = (-t * 2.1).exp();
            let bright = (-t * 9.0).exp();
            let ph = std::f32::consts::TAU * freq * t;
            // Soft plucked string: fundamental + decaying odd partials + tiny noise tick
            let tick = if k < (sample_rate as usize / 400).max(1) {
                ((k as f32 * 19.7).sin()) * 0.15 * bright
            } else {
                0.0
            };
            let sig = (ph.sin() * 0.62
                + (2.0 * ph).sin() * 0.22 * bright
                + (3.0 * ph).sin() * 0.12 * bright
                + (4.0 * ph).sin() * 0.05 * bright
                + tick)
                * attack
                * body
                * amp;
            let spread = 0.15 * (ph * 0.5).sin();
            left[idx] += sig * (1.0 - spread);
            right[idx] += sig * (1.0 + spread);
        }
    }

    // Soft peak normalize so amp sim has headroom
    let peak = left
        .iter()
        .chain(right.iter())
        .map(|v| v.abs())
        .fold(0.0_f32, f32::max)
        .max(1e-6);
    let norm = 0.55 / peak;
    for s in left.iter_mut().chain(right.iter_mut()) {
        *s *= norm;
    }

    AudioBuffer::from_stereo(sample_rate, left, right, None)
}
