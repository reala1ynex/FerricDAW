use crate::clip::{AudioBuffer, ClipId, MidiNote, SessionSlot, Take, TakeContent, TimelineClip};
use crate::fx::{FxKind, FxSlot, LfoShape, ModRouting, ModTarget, ModulatorKind, ModulatorSlot};
use crate::track::{InputSource, Track, TrackId, TrackSend};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: Uuid,
    pub name: String,
    pub length_beats: f64,
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
        eq.set_param(0, 28.0); // Gentle 28 Hz subsonic cleanup
        eq.set_param(10, 1.0); // +1 dB air shelf
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
    pub snap_grid: SnapGrid,
    pub loop_enabled: bool,
    pub loop_start_beat: f64,
    pub loop_end_beat: f64,
    pub punch_in_enabled: bool,
    pub punch_start_beat: f64,
    pub punch_end_beat: f64,
    pub metronome_enabled: bool,
    pub count_in_bars: u8,
    pub scenes: Vec<Scene>,
    pub markers: Vec<TimelineMarker>,
    pub tracks: Vec<Track>,
    pub master: MasterBus,
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
                name: "Verse".into(),
                length_beats: 16.0,
            },
            Scene {
                id: Uuid::new_v4(),
                name: "Chorus".into(),
                length_beats: 16.0,
            },
            Scene {
                id: Uuid::new_v4(),
                name: "Jam / Bridge".into(),
                length_beats: 16.0,
            },
        ];
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            artist: "Home Studio".into(),
            bpm: 118.0,
            time_signature: TimeSignature::default(),
            snap_grid: SnapGrid::Quarter,
            loop_enabled: true,
            loop_start_beat: 0.0,
            loop_end_beat: 16.0,
            punch_in_enabled: false,
            punch_start_beat: 4.0,
            punch_end_beat: 12.0,
            metronome_enabled: false,
            count_in_bars: 0,
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
        }
    }

    pub fn beats_to_seconds(&self, beats: f64) -> f64 {
        beats * 60.0 / self.bpm.max(20.0)
    }

    pub fn seconds_to_beats(&self, secs: f64) -> f64 {
        secs * self.bpm.max(20.0) / 60.0
    }

    pub fn format_bar_beat_tick(&self, beat: f64) -> String {
        let bpb = self.time_signature.beats_per_bar();
        let bar = (beat / bpb).floor() as i64 + 1;
        let in_bar = beat.rem_euclid(bpb);
        let beat_num = in_bar.floor() as i64 + 1;
        let ticks = ((in_bar.fract()) * 960.0).round() as i64;
        format!("{:03}.{}.{:03}", bar, beat_num, ticks.clamp(0, 959))
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
    /// folder children and send sources execute strictly BEFORE their destination buses!
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
                    if src_idx != dst_idx {
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

    /// Verifies that adding an edge `source_id -> target_id` will not create a cycle.
    pub fn can_route(&self, source_id: TrackId, target_id: TrackId) -> bool {
        if source_id == target_id {
            return false;
        }
        // Check if `source_id` is reachable starting from `target_id`
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
        }
        true
    }

    pub fn add_send(&mut self, src_id: TrackId, dst_id: TrackId, gain_db: f32) -> Result<(), ModelError> {
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

    pub fn add_track(&mut self, name: impl Into<String>) -> TrackId {
        let idx = self.tracks.len();
        let track = Track::new(name, idx, self.scenes.len());
        let id = track.id;
        self.tracks.push(track);
        id
    }

    /// Non-destructive REAPER-style Razor/Split clip at `split_beat`.
    pub fn split_clip(&mut self, track_id: TrackId, clip_id: ClipId, split_beat: f64) -> Option<ClipId> {
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
        right_clip.slip_offset_beats = orig.slip_offset_beats + left_len;
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

    /// Drops an entire Session Launcher scene directly onto the linear arrangement at `at_beat`.
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
        if let Some(clip) = self.find_track_mut(track_id).and_then(|t| t.find_clip_mut(clip_id)) {
            let active = clip.active_take;
            if let Some(Take {
                content: TakeContent::Midi { notes },
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

    /// Saves human-readable Git-diffable RON (`.ferric`) or JSON (`.json`).
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
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn load_from_file(path: &Path) -> Result<Self, ModelError> {
        let text = std::fs::read_to_string(path)?;
        let is_json = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if is_json {
            Ok(serde_json::from_str(&text)?)
        } else {
            ron::from_str(&text).map_err(|e| ModelError::Ron(e.to_string()))
        }
    }
}

/// Snapshot-based Undo/Redo manager (`Arc`-backed audio buffers make snapshots < 20 KB & instant!).
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

/// Builds a rich, playable out-of-the-box demo project showcasing:
/// 1. Folder Bus submix ("Band & Guitars Bus")
/// 2. 8-Pad Studio Drum Kit groove + Session Clip variations
/// 3. Sub/Analog Bassline
/// 4. Multi-Take Recorded Electric Guitar with REAPER Swipe-Comping across 3 takes + Tube Amp Sim!
/// 5. PolySynth Chord/Arp Lead with Bitwig-style LFO modulating Filter Cutoff
/// 6. Lush Plate Reverb Aux Return Bus fed via Post-Fader Sends
pub fn build_demo_project(sample_rate: u32) -> Project {
    let sr = sample_rate.max(44_100);
    let mut proj = Project::new_empty("Neon Oxide — Hybrid Demo Session");
    proj.bpm = 116.0;
    proj.loop_enabled = true;
    proj.loop_start_beat = 0.0;
    proj.loop_end_beat = 16.0;

    let n_scenes = proj.scenes.len();

    // 1. Drums Track
    let mut drums = Track::new("01 Studio Kit", 0, n_scenes);
    drums.gain_db = -1.5;
    drums.fx_chain.push(FxSlot::new(FxKind::DrumKit));
    let mut drum_comp = FxSlot::new(FxKind::Compressor);
    drum_comp.set_param(0, -16.0);
    drum_comp.set_param(1, 3.5);
    drum_comp.set_param(5, 2.0);
    drums.fx_chain.push(drum_comp);

    let mut beat_notes = Vec::new();
    for bar in 0..4 {
        let b = (bar * 4) as f64;
        // Kick (36)
        beat_notes.push(MidiNote::new(36, 118, b + 0.0, 0.4));
        beat_notes.push(MidiNote::new(36, 102, b + 1.5, 0.4));
        beat_notes.push(MidiNote::new(36, 112, b + 2.5, 0.4));
        // Snare (38)
        beat_notes.push(MidiNote::new(38, 115, b + 1.0, 0.4));
        beat_notes.push(MidiNote::new(38, 118, b + 3.0, 0.4));
        if bar == 3 {
            beat_notes.push(MidiNote::new(38, 92, b + 3.5, 0.25));
            beat_notes.push(MidiNote::new(39, 105, b + 3.75, 0.25));
        }
        // Closed & Open Hi-Hats (42, 46)
        for eighth in 0..8 {
            let hb = b + eighth as f64 * 0.5;
            if eighth == 7 && bar % 2 == 1 {
                beat_notes.push(MidiNote::new(46, 94, hb, 0.45));
            } else {
                let vel = if eighth % 2 == 0 { 92 } else { 68 };
                beat_notes.push(MidiNote::new(42, vel, hb, 0.2));
            }
        }
    }
    drums.clips.push(TimelineClip::new_midi(
        "Main Pocket Groove",
        0.0,
        16.0,
        beat_notes.clone(),
    ));
    drums.session_slots[0] = Some(SessionSlot::new_midi("Intro Groove", 16.0, beat_notes.clone()));
    drums.session_slots[1] = Some(SessionSlot::new_midi("Verse Pocket", 16.0, beat_notes));

    // 2. Analog Bass Track
    let mut bass = Track::new("02 Ferric Bass", 1, n_scenes);
    bass.gain_db = -3.0;
    let mut bass_synth = FxSlot::new(FxKind::PolySynth);
    bass_synth.set_param(0, 0.0); // Saw
    bass_synth.set_param(1, 55.0); // Rich sub/detune
    bass_synth.set_param(2, 720.0); // Warm lowpass
    bass_synth.set_param(3, 0.34);
    bass_synth.set_param(4, 58.0);
    bass_synth.set_param(5, 4.0);
    bass_synth.set_param(6, 220.0);
    bass_synth.set_param(7, 45.0);
    bass_synth.set_param(8, 120.0);
    bass.fx_chain.push(bass_synth);

    // Progression: Am -> F -> C -> G (A1=33, F1=29, C2=36, G1=31)
    let roots = [45u8, 41, 48, 43];
    let mut bass_notes = Vec::new();
    for (bar, &root) in roots.iter().enumerate() {
        let b = (bar * 4) as f64;
        bass_notes.push(MidiNote::new(root, 112, b + 0.0, 0.85));
        bass_notes.push(MidiNote::new(root, 96, b + 1.0, 0.45));
        bass_notes.push(MidiNote::new(root + 12, 100, b + 1.5, 0.45));
        bass_notes.push(MidiNote::new(root, 108, b + 2.5, 0.85));
        bass_notes.push(MidiNote::new(root + 7, 94, b + 3.5, 0.45));
    }
    bass.clips.push(TimelineClip::new_midi(
        "Oxide Sub Line",
        0.0,
        16.0,
        bass_notes.clone(),
    ));
    bass.session_slots[1] = Some(SessionSlot::new_midi("Verse Bass", 16.0, bass_notes));

    // 3. Folder Bus: Guitars & Keys Submix
    let mut bus_folder = Track::new("03 Harmonic Bus (Folder)", 7, n_scenes);
    bus_folder.gain_db = -1.0;
    let mut bus_eq = FxSlot::new(FxKind::ParametricEq);
    bus_eq.set_param(0, 95.0); // Clean low-end mud for bass
    bus_folder.fx_chain.push(bus_eq);
    let folder_id = bus_folder.id;

    // 4. Multi-Take Electric Guitar Audio Track (inside Folder Bus!)
    let mut guitar = Track::new("04 DI Riff Guitar (3 Takes)", 2, n_scenes);
    guitar.parent_folder_id = Some(folder_id);
    guitar.gain_db = -3.5;
    guitar.pan = -0.25;
    guitar.input_source = InputSource::AudioMono { channel: 0 };
    guitar.fx_chain.push(FxSlot::new(FxKind::ChromaticTuner));
    let mut amp = FxSlot::new(FxKind::AmpCabSim);
    amp.set_param(0, 11.5); // Edge-of-breakup tube warmth
    amp.set_param(1, 1.0); // 2x12 Brit cab
    amp.set_param(6, -4.0);
    guitar.fx_chain.push(amp);

    let take1_buf = synthesize_guitar_take(sr, proj.bpm, 0);
    let take2_buf = synthesize_guitar_take(sr, proj.bpm, 1);
    let take3_buf = synthesize_guitar_take(sr, proj.bpm, 2);

    let mut gtr_clip = TimelineClip::new_audio("Chorus Riff Comp", 0.0, 16.0, take1_buf.clone());
    gtr_clip.takes[0].name = "Take 1 — Warm Chord Strum".into();
    gtr_clip.takes[0].rating = 4;
    let mut t2 = Take::new_audio("Take 2 — Octave Arpeggio", take2_buf);
    t2.rating = 5;
    let mut t3 = Take::new_audio("Take 3 — High Lead Double", take3_buf);
    t3.rating = 4;
    gtr_clip.takes.push(t2);
    gtr_clip.takes.push(t3);
    gtr_clip.show_take_lanes = true;
    // Pre-build a genuine REAPER Swipe Comp across all 3 takes!
    gtr_clip.set_comp_region(0.0, 4.0, 0);
    gtr_clip.set_comp_region(4.0, 8.0, 1);
    gtr_clip.set_comp_region(8.0, 12.0, 0);
    gtr_clip.set_comp_region(12.0, 16.0, 2);
    guitar.clips.push(gtr_clip);
    guitar.session_slots[0] = Some(SessionSlot::new_audio("Clean Riff Loop", 16.0, take1_buf));

    // 5. PolySynth Pad/Arp with Bitwig LFO Modulator (inside Folder Bus!)
    let mut keys = Track::new("05 PolySynth + LFO Filter", 4, n_scenes);
    keys.parent_folder_id = Some(folder_id);
    keys.gain_db = -4.5;
    keys.pan = 0.25;
    let mut poly = FxSlot::new(FxKind::PolySynth);
    poly.set_param(0, 0.0); // Saw
    poly.set_param(1, 38.0); // Lush unison detune
    poly.set_param(2, 1900.0); // Base cutoff modulated by LFO!
    poly.set_param(3, 0.36);
    poly.set_param(5, 35.0);
    poly.set_param(7, 78.0);
    poly.set_param(8, 520.0);
    let poly_id = poly.id;
    keys.fx_chain.push(poly);
    keys.fx_chain.push(FxSlot::new(FxKind::StereoDelay));

    let mut lfo = ModulatorSlot::new_lfo("Filter Breath LFO");
    lfo.kind = ModulatorKind::Lfo {
        shape: LfoShape::Sine,
        rate_hz: 0.5,
        tempo_sync: true,
        sync_beats: 4.0,
        bipolar: true,
    };
    lfo.routings.push(ModRouting {
        target: ModTarget::FxParam {
            fx_id: poly_id,
            param_index: 2, // Cutoff!
        },
        depth: 0.28,
    });
    keys.modulators.push(lfo);

    let chords: [[u8; 4]; 4] = [
        [57, 60, 64, 71], // Am9
        [53, 57, 60, 69], // Fmaj7/6
        [60, 64, 67, 71], // Cmaj7
        [55, 59, 62, 69], // G6
    ];
    let mut chord_notes = Vec::new();
    for (bar, chord) in chords.iter().enumerate() {
        let b = (bar * 4) as f64;
        for &n in chord {
            chord_notes.push(MidiNote::new(n, 84, b, 3.75));
        }
        // Top sparkle note on beat 3.5
        chord_notes.push(MidiNote::new(chord[3] + 5, 76, b + 3.0, 0.85));
    }
    keys.clips.push(TimelineClip::new_midi(
        "Lush Modulated Chords",
        0.0,
        16.0,
        chord_notes.clone(),
    ));
    keys.session_slots[0] = Some(SessionSlot::new_midi("Intro Pad", 16.0, chord_notes));

    // 6. Aux Reverb Return Bus
    let mut verb_bus = Track::new("06 Lush Plate Aux", 5, n_scenes);
    verb_bus.gain_db = -2.0;
    let mut reverb = FxSlot::new(FxKind::PlateHallReverb);
    reverb.wet_mix = 1.0; // 100% wet aux return
    reverb.set_param(0, 24.0);
    reverb.set_param(1, 82.0);
    reverb.set_param(3, 180.0);
    verb_bus.fx_chain.push(reverb);
    let verb_id = verb_bus.id;

    // Route Sends from Drums, Guitar, and Keys into Lush Plate Aux!
    drums.sends.push(TrackSend::new(verb_id, -14.0));
    guitar.sends.push(TrackSend::new(verb_id, -9.5));
    keys.sends.push(TrackSend::new(verb_id, -7.5));

    proj.tracks = vec![drums, bass, bus_folder, guitar, keys, verb_bus];
    proj
}

/// Generates realistic Plucked Karplus-Strong / Harmonic DI Electric Guitar audio buffers
/// for 3 distinct musical variations so Multi-Take Swipe Comping sounds awesome immediately!
fn synthesize_guitar_take(sample_rate: u32, bpm: f64, variation: usize) -> AudioBuffer {
    let spb = 60.0 / bpm;
    let total_secs = 16.0 * spb;
    let frames = (total_secs * sample_rate as f64) as usize;
    let mut left = vec![0.0_f32; frames];
    let mut right = vec![0.0_f32; frames];

    // Pluck events: (beat, midi_pitch, duration_beats, gain)
    let base_chords: [[u8; 3]; 4] = [
        [57, 64, 69], // Am
        [53, 60, 69], // F
        [60, 64, 72], // C
        [55, 62, 71], // G
    ];

    let mut events: Vec<(f64, u8, f64, f32)> = Vec::new();
    for (bar, triad) in base_chords.iter().enumerate() {
        let b = (bar * 4) as f64;
        match variation {
            0 => {
                // Warm rhythmic strummed chords
                for (i, &p) in triad.iter().enumerate() {
                    events.push((b + i as f64 * 0.04, p, 1.8, 0.30));
                    events.push((b + 2.0 + i as f64 * 0.04, p, 1.8, 0.28));
                }
            }
            1 => {
                // Sparkling 8th-note arpeggio
                let pattern = [triad[0], triad[1], triad[2], triad[1], triad[2] + 5, triad[2], triad[1], triad[0]];
                for (step, &p) in pattern.iter().enumerate() {
                    events.push((b + step as f64 * 0.5, p, 0.75, 0.34));
                }
            }
            _ => {
                // Melodic high octave lead hook
                events.push((b + 0.0, triad[2] + 12, 1.4, 0.34));
                events.push((b + 1.5, triad[1] + 12, 0.9, 0.30));
                events.push((b + 2.5, triad[2] + 12, 1.4, 0.34));
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
            let attack = (t / 0.004).min(1.0);
            let decay = (-t * 2.6).exp();
            let brightness = (-t * 7.5).exp();
            let ph = std::f32::consts::TAU * freq * t;
            // Rich plucked guitar harmonics (fundamental + 2nd + 3rd + pick transient)
            let sig = (ph.sin() * 0.65
                + (2.0 * ph).sin() * 0.24 * (0.4 + 0.6 * brightness)
                + (3.0 * ph).sin() * 0.12 * brightness
                + (4.0 * ph).sin() * 0.06 * brightness)
                * attack
                * decay
                * amp;
            left[idx] += sig;
            right[idx] += sig * 0.98;
        }
    }

    AudioBuffer::from_stereo(sample_rate, left, right, None)
}
