use crate::clip::{AudioBuffer, ClipId, SessionSlot, TimelineClip};
use crate::fx::{FxId, FxSlot, ModulatorSlot};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type TrackId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputSource {
    AudioAndMidi { stereo_pair: u8 },
    AudioMono { channel: u8 },
    MidiAll,
    ResampleTrackOutput,
}

impl InputSource {
    pub fn label(self) -> &'static str {
        match self {
            InputSource::AudioAndMidi { .. } => "In 1/2 + All MIDI",
            InputSource::AudioMono { channel: 0 } => "Mic / Guitar In 1 (Mono)",
            InputSource::AudioMono { .. } => "Inst In 2 (Mono)",
            InputSource::MidiAll => "All MIDI Inputs",
            InputSource::ResampleTrackOutput => "Resample Track Out",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendTap {
    PreFx,
    PreFaderPostFx,
    PostFader,
}

impl SendTap {
    pub fn label(self) -> &'static str {
        match self {
            SendTap::PreFx => "Pre-FX",
            SendTap::PreFaderPostFx => "Pre-Fader",
            SendTap::PostFader => "Post-Fader",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSend {
    pub id: Uuid,
    pub target_track_id: TrackId,
    pub gain_db: f32,
    pub pan: f32,
    pub tap: SendTap,
    pub muted: bool,
}

impl TrackSend {
    pub fn new(target_track_id: TrackId, gain_db: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            target_track_id,
            gain_db,
            pan: 0.0,
            tap: SendTap::PostFader,
            muted: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AutomationMode {
    Off,
    #[default]
    Read,
    Touch,
    Latch,
    Write,
}

impl AutomationMode {
    pub const ALL: &'static [AutomationMode] = &[
        AutomationMode::Off,
        AutomationMode::Read,
        AutomationMode::Touch,
        AutomationMode::Latch,
        AutomationMode::Write,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AutomationMode::Off => "Trim/Off",
            AutomationMode::Read => "Read",
            AutomationMode::Touch => "Touch",
            AutomationMode::Latch => "Latch",
            AutomationMode::Write => "Write",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub id: Uuid,
    pub beat: f64,
    pub normalized_value: f32, // 0.0 ..= 1.0
    /// Bezier curve tension (`-1.0` = fast/log, `0.0` = linear, `+1.0` = slow/exp).
    #[serde(default)]
    pub tension: f32,
}

impl AutomationPoint {
    pub fn new(beat: f64, normalized_value: f32, tension: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            beat: beat.max(0.0),
            normalized_value: normalized_value.clamp(0.0, 1.0),
            tension: tension.clamp(-0.95, 0.95),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutomationTarget {
    Volume,
    Pan,
    StereoWidth,
    FxParam { fx_id: FxId, param_index: usize },
    FxWet { fx_id: FxId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLane {
    pub id: Uuid,
    pub target: AutomationTarget,
    pub visible: bool,
    pub enabled: bool,
    pub points: Vec<AutomationPoint>,
}

impl AutomationLane {
    pub fn new(target: AutomationTarget) -> Self {
        Self {
            id: Uuid::new_v4(),
            target,
            visible: true,
            enabled: true,
            points: Vec::new(),
        }
    }

    /// Evaluates curved Bezier/power-law automation envelope at `beat`.
    #[inline]
    pub fn eval_at(&self, beat: f64) -> Option<f32> {
        if !self.enabled || self.points.is_empty() {
            return None;
        }
        if beat <= self.points[0].beat {
            return Some(self.points[0].normalized_value);
        }
        let last = self.points.last().unwrap();
        if beat >= last.beat {
            return Some(last.normalized_value);
        }
        for pair in self.points.windows(2) {
            let a = &pair[0];
            let b = &pair[1];
            if beat >= a.beat && beat <= b.beat {
                let span = (b.beat - a.beat).max(1e-6);
                let lin_t = ((beat - a.beat) / span).clamp(0.0, 1.0) as f32;
                let shaped_t = apply_curve_tension(lin_t, a.tension);
                return Some(
                    a.normalized_value + (b.normalized_value - a.normalized_value) * shaped_t,
                );
            }
        }
        None
    }

    pub fn upsert_point(&mut self, beat: f64, norm_val: f32) -> Uuid {
        let norm_val = norm_val.clamp(0.0, 1.0);
        if let Some(existing) = self
            .points
            .iter_mut()
            .find(|p| (p.beat - beat).abs() < 0.08)
        {
            existing.normalized_value = norm_val;
            existing.beat = beat.max(0.0);
            let id = existing.id;
            self.sort_points();
            return id;
        }
        let pt = AutomationPoint::new(beat, norm_val, 0.0);
        let id = pt.id;
        self.points.push(pt);
        self.sort_points();
        id
    }

    pub fn sort_points(&mut self) {
        self.points
            .sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
    }
}

#[inline]
pub fn apply_curve_tension(t: f32, tension: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if tension.abs() < 0.01 {
        return t;
    }
    let exp = 2.0_f32.powf(tension * 2.4);
    t.powf(exp)
}

pub const STUDIO_TRACK_COLORS: &[[u8; 3]] = &[
    [232, 114, 62],  // Ferric Copper
    [66, 179, 165],  // Teal Oxide
    [235, 178, 56],  // Warm Amber
    [116, 146, 242], // Cobalt Blue
    [186, 108, 226], // Violet Synth
    [94, 198, 117],  // Studio Sage
    [232, 88, 116],  // Crimson Vocal
    [92, 176, 232],  // Sky Bus
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArpMode {
    #[default]
    Up,
    Down,
    UpDown,
    ChordGate,
}

impl ArpMode {
    pub const ALL: &'static [ArpMode] = &[
        ArpMode::Up,
        ArpMode::Down,
        ArpMode::UpDown,
        ArpMode::ChordGate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ArpMode::Up => "Up",
            ArpMode::Down => "Down",
            ArpMode::UpDown => "Up/Down",
            ArpMode::ChordGate => "Chord Gate",
        }
    }
}

/// Built-in Tempo-Synced MIDI Arpeggiator / Note-Repeat per Track (pairs great with Akai LPK25!).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrackArpeggiator {
    pub enabled: bool,
    pub mode: ArpMode,
    /// Step division in beats (`0.25` = 1/16, `0.333` = 1/8T, `0.5` = 1/8, `1.0` = 1/4).
    pub rate_beats: f32,
    pub octaves: u8,
    pub gate: f32,
}

impl Default for TrackArpeggiator {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ArpMode::Up,
            rate_beats: 0.25,
            octaves: 2,
            gate: 0.72,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub color: [u8; 3],
    pub parent_folder_id: Option<TrackId>,
    pub folder_collapsed: bool,
    pub send_to_master: bool,
    pub gain_db: f32,
    pub pan: f32,
    pub stereo_width: f32,
    pub mute: bool,
    pub solo: bool,
    pub record_arm: bool,
    pub input_monitoring: bool,
    pub phase_invert: bool,
    pub input_source: InputSource,
    #[serde(default)]
    pub arpeggiator: TrackArpeggiator,
    pub fx_chain: Vec<FxSlot>,
    pub modulators: Vec<ModulatorSlot>,
    pub sends: Vec<TrackSend>,
    pub clips: Vec<TimelineClip>,
    pub session_slots: Vec<Option<SessionSlot>>,
    pub active_session_slot: Option<usize>,
    #[serde(default)]
    pub automation_mode: AutomationMode,
    pub automation_lanes: Vec<AutomationLane>,
    #[serde(default)]
    pub show_automation: bool,
    /// 1-Click Track Freeze: streams pre-rendered 32-bit stem and bypasses heavy DSP!
    #[serde(default)]
    pub frozen: bool,
    #[serde(skip)]
    pub frozen_buffer: Option<AudioBuffer>,
    pub height_px: f32,
}

impl Track {
    pub fn new(name: impl Into<String>, color_idx: usize, num_scenes: usize) -> Self {
        let color = STUDIO_TRACK_COLORS[color_idx % STUDIO_TRACK_COLORS.len()];
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            color,
            parent_folder_id: None,
            folder_collapsed: false,
            send_to_master: true,
            gain_db: 0.0,
            pan: 0.0,
            stereo_width: 1.0,
            mute: false,
            solo: false,
            record_arm: false,
            input_monitoring: true,
            phase_invert: false,
            input_source: InputSource::AudioAndMidi { stereo_pair: 0 },
            arpeggiator: TrackArpeggiator::default(),
            fx_chain: Vec::new(),
            modulators: Vec::new(),
            sends: Vec::new(),
            clips: Vec::new(),
            session_slots: vec![None; num_scenes.max(4)],
            active_session_slot: None,
            automation_mode: AutomationMode::Read,
            automation_lanes: Vec::new(),
            show_automation: false,
            frozen: false,
            frozen_buffer: None,
            height_px: 84.0,
        }
    }

    pub fn find_clip(&self, clip_id: ClipId) -> Option<&TimelineClip> {
        self.clips.iter().find(|c| c.id == clip_id)
    }

    pub fn find_clip_mut(&mut self, clip_id: ClipId) -> Option<&mut TimelineClip> {
        self.clips.iter_mut().find(|c| c.id == clip_id)
    }

    /// Automatic Equal-Power Overlap Crossfade Gain (`cos(t*pi/2)` fade-out for earlier clip,
    /// `sin(t*pi/2)` fade-in for later clip so `g_a^2 + g_b^2 == 1.0`!).
    #[inline]
    pub fn auto_crossfade_gain(&self, clip_idx: usize, beat: f64) -> f32 {
        let Some(me) = self.clips.get(clip_idx) else {
            return 1.0;
        };
        if me.muted || me.is_midi() {
            return 1.0;
        }
        let mut gain = 1.0_f32;
        for (other_idx, other) in self.clips.iter().enumerate() {
            if other_idx == clip_idx || other.muted || other.is_midi() {
                continue;
            }
            let ov_start = me.start_beat.max(other.start_beat);
            let ov_end = me.end_beat().min(other.end_beat());
            let span = ov_end - ov_start;
            if span > 1e-4 && beat >= ov_start && beat <= ov_end {
                let t = ((beat - ov_start) / span).clamp(0.0, 1.0) as f32;
                let me_is_later = me.start_beat > other.start_beat
                    || ((me.start_beat - other.start_beat).abs() < 1e-5 && clip_idx > other_idx);
                if me_is_later {
                    gain *= (t * std::f32::consts::FRAC_PI_2).sin();
                } else {
                    gain *= (t * std::f32::consts::FRAC_PI_2).cos();
                }
            }
        }
        gain
    }

    pub fn ensure_scene_count(&mut self, scenes: usize) {
        if self.session_slots.len() < scenes {
            self.session_slots.resize_with(scenes, || None);
        }
    }

    pub fn ensure_automation_lane(&mut self, target: AutomationTarget) -> usize {
        if let Some(idx) = self
            .automation_lanes
            .iter()
            .position(|l| l.target == target)
        {
            self.automation_lanes[idx].visible = true;
            self.show_automation = true;
            return idx;
        }
        let mut lane = AutomationLane::new(target.clone());
        let init_norm = match target {
            AutomationTarget::Volume => ((self.gain_db + 60.0) / 72.0).clamp(0.0, 1.0),
            AutomationTarget::Pan => ((self.pan + 1.0) * 0.5).clamp(0.0, 1.0),
            AutomationTarget::StereoWidth => (self.stereo_width * 0.5).clamp(0.0, 1.0),
            AutomationTarget::FxWet { fx_id } => self
                .fx_chain
                .iter()
                .find(|f| f.id == fx_id)
                .map(|f| f.wet_mix)
                .unwrap_or(1.0),
            AutomationTarget::FxParam { fx_id, param_index } => self
                .fx_chain
                .iter()
                .find(|f| f.id == fx_id)
                .map(|f| f.normalize_param(param_index, f.param(param_index)))
                .unwrap_or(0.5),
        };
        lane.points
            .push(crate::track::AutomationPoint::new(0.0, init_norm, 0.0));
        self.automation_lanes.push(lane);
        self.show_automation = true;
        self.automation_lanes.len() - 1
    }
}
