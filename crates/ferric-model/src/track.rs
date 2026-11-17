use crate::clip::{AudioBuffer, ClipId, SessionSlot, TimelineClip};
use crate::fx::{FxId, FxSlot, ModulatorSlot};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type TrackId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputSource {
    /// Stereo hardware pair `stereo_pair` → channels `(pair*2, pair*2+1)` plus all MIDI.
    AudioAndMidi { stereo_pair: u8 },
    /// Mono hardware channel index (0-based).
    AudioMono { channel: u8 },
    MidiAll,
    ResampleTrackOutput,
}

impl InputSource {
    pub fn label(self) -> String {
        match self {
            InputSource::AudioAndMidi { stereo_pair } => {
                let a = stereo_pair as u16 * 2 + 1;
                let b = a + 1;
                format!("Mic/Line {a}/{b} + MIDI")
            }
            InputSource::AudioMono { channel } => {
                format!("Mic/Line {} (Mono)", channel as u16 + 1)
            }
            InputSource::MidiAll => "MIDI only (no mic)".into(),
            InputSource::ResampleTrackOutput => "Resample Track Out".into(),
        }
    }

    /// Compact label for cramped track headers.
    pub fn short_label(self) -> String {
        match self {
            InputSource::AudioAndMidi { stereo_pair } => {
                let a = stereo_pair as u16 * 2 + 1;
                format!("In{a}/{}+MIDI", a + 1)
            }
            InputSource::AudioMono { channel } => {
                format!("Mic {}", channel as u16 + 1)
            }
            InputSource::MidiAll => "MIDI".into(),
            InputSource::ResampleTrackOutput => "Resample".into(),
        }
    }

    /// Hardware channel indices used by this source (empty for MIDI-only / resample).
    pub fn hardware_channels(self) -> Option<(u8, Option<u8>)> {
        match self {
            InputSource::AudioAndMidi { stereo_pair } => {
                let l = stereo_pair.saturating_mul(2);
                Some((l, Some(l.saturating_add(1))))
            }
            InputSource::AudioMono { channel } => Some((channel, None)),
            InputSource::MidiAll | InputSource::ResampleTrackOutput => None,
        }
    }

    pub fn all_hardware_choices() -> Vec<InputSource> {
        let mut v = Vec::with_capacity(20);
        for pair in 0..8u8 {
            v.push(InputSource::AudioAndMidi { stereo_pair: pair });
        }
        for ch in 0..16u8 {
            v.push(InputSource::AudioMono { channel: ch });
        }
        v.push(InputSource::MidiAll);
        v.push(InputSource::ResampleTrackOutput);
        v
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
            .sort_by(|a, b| a.beat.total_cmp(&b.beat));
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

/// Built-in tempo-synced MIDI arpeggiator / note-repeat per track.
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

/// Non-destructive MPC Groove Pool Template per Track (applied live in `AudioGraphCore` without
/// mutating stored MIDI clip grid coordinates!).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GrooveTemplate {
    #[default]
    Straight,
    Mpc54Tight,
    Mpc58Classic,
    Mpc62BoomBap,
    Mpc66TripletSwing,
    Mpc71HeavyPocket,
    LaidBackNeoSoul,
    PushedUrgent16ths,
}

impl GrooveTemplate {
    pub const ALL: &'static [GrooveTemplate] = &[
        GrooveTemplate::Straight,
        GrooveTemplate::Mpc54Tight,
        GrooveTemplate::Mpc58Classic,
        GrooveTemplate::Mpc62BoomBap,
        GrooveTemplate::Mpc66TripletSwing,
        GrooveTemplate::Mpc71HeavyPocket,
        GrooveTemplate::LaidBackNeoSoul,
        GrooveTemplate::PushedUrgent16ths,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GrooveTemplate::Straight => "Straight 50% (Grid)",
            GrooveTemplate::Mpc54Tight => "MPC-60 54% Tight",
            GrooveTemplate::Mpc58Classic => "MPC-3000 58% Pocket",
            GrooveTemplate::Mpc62BoomBap => "MPC 62% Boom-Bap",
            GrooveTemplate::Mpc66TripletSwing => "MPC 66.7% Triplet",
            GrooveTemplate::Mpc71HeavyPocket => "SP-1200 71% Heavy",
            GrooveTemplate::LaidBackNeoSoul => "Dilla / Neo-Soul Drag",
            GrooveTemplate::PushedUrgent16ths => "Pushed Rush (+Pocket)",
        }
    }

    pub fn short_badge(self) -> &'static str {
        match self {
            GrooveTemplate::Straight => "50%",
            GrooveTemplate::Mpc54Tight => "MPC 54%",
            GrooveTemplate::Mpc58Classic => "MPC 58%",
            GrooveTemplate::Mpc62BoomBap => "MPC 62%",
            GrooveTemplate::Mpc66TripletSwing => "MPC 67%",
            GrooveTemplate::Mpc71HeavyPocket => "SP 71%",
            GrooveTemplate::LaidBackNeoSoul => "DILLA",
            GrooveTemplate::PushedUrgent16ths => "PUSH",
        }
    }

    /// Computes `(beat_offset, velocity_multiplier)` for a note starting at `beat` scaled by `amount` (`0.0..=1.0`).
    #[inline]
    pub fn transform_note(self, beat: f64, amount: f32) -> (f64, f32) {
        let amt = amount.clamp(0.0, 1.0) as f64;
        if self == GrooveTemplate::Straight || amt <= 1e-4 {
            return (0.0, 1.0);
        }
        // Position within a 1/8-note pair (0.5 beats = two 1/16th steps: `0.00` = downbeat 16th, `0.25` = upbeat 16th)
        let pos_in_eighth = beat.rem_euclid(0.5);
        let is_odd_16th = (pos_in_eighth - 0.25).abs() < 0.09;

        match self {
            GrooveTemplate::Straight => (0.0, 1.0),
            GrooveTemplate::Mpc54Tight => {
                if is_odd_16th {
                    (0.5 * (0.54 - 0.50) * amt, 1.0 + 0.05 * amt as f32)
                } else {
                    (0.0, 1.0)
                }
            }
            GrooveTemplate::Mpc58Classic => {
                if is_odd_16th {
                    (0.5 * (0.58 - 0.50) * amt, 1.0 + 0.08 * amt as f32)
                } else {
                    (0.0, 1.0)
                }
            }
            GrooveTemplate::Mpc62BoomBap => {
                if is_odd_16th {
                    (0.5 * (0.62 - 0.50) * amt, 1.0 + 0.10 * amt as f32)
                } else {
                    (0.0, 0.96)
                }
            }
            GrooveTemplate::Mpc66TripletSwing => {
                if is_odd_16th {
                    (0.5 * (0.6667 - 0.50) * amt, 1.0 + 0.10 * amt as f32)
                } else {
                    (0.0, 1.0)
                }
            }
            GrooveTemplate::Mpc71HeavyPocket => {
                if is_odd_16th {
                    (0.5 * (0.71 - 0.50) * amt, 1.0 + 0.12 * amt as f32)
                } else {
                    (0.0, 0.95)
                }
            }
            GrooveTemplate::LaidBackNeoSoul => {
                // Backbeat snares & odd 16ths drag slightly behind the grid for warm human pocket
                let beat_in_bar = beat.rem_euclid(4.0);
                let is_backbeat = (beat_in_bar - 1.0).abs() < 0.08 || (beat_in_bar - 3.0).abs() < 0.08;
                if is_odd_16th {
                    (0.048 * amt, 0.92)
                } else if is_backbeat {
                    (0.026 * amt, 1.06)
                } else {
                    (0.0, 1.0)
                }
            }
            GrooveTemplate::PushedUrgent16ths => {
                if is_odd_16th {
                    (-0.018 * amt, 1.05)
                } else {
                    (0.0, 1.0)
                }
            }
        }
    }
}

fn default_groove_amount() -> f32 {
    1.0
}

/// Accent shapes for Velocity DNA (deterministic pocket accents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VelocityAccentShape {
    #[default]
    Flat,
    OffbeatLift,
    DownbeatPunch,
    HatChatter,
}

impl VelocityAccentShape {
    pub const ALL: &'static [VelocityAccentShape] = &[
        Self::Flat,
        Self::OffbeatLift,
        Self::DownbeatPunch,
        Self::HatChatter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::OffbeatLift => "Offbeat Lift",
            Self::DownbeatPunch => "Downbeat Punch",
            Self::HatChatter => "Hat Chatter",
        }
    }

    /// Returns a velocity scale factor for a note at `beat` given DNA depth `0..=1`.
    /// Blends toward accents; depth 0 leaves velocity unchanged (factor 1.0).
    #[inline]
    pub fn velocity_scale(self, beat: f64, depth: f32, seed: u32) -> f32 {
        let d = depth.clamp(0.0, 1.0);
        if d < 1e-4 || self == Self::Flat {
            return 1.0;
        }
        let pos16 = ((beat.rem_euclid(4.0) / 0.25).floor() as i32).rem_euclid(16);
        let accent = match self {
            Self::Flat => 1.0,
            Self::OffbeatLift => {
                if pos16 % 2 == 1 {
                    1.12
                } else {
                    0.92
                }
            }
            Self::DownbeatPunch => {
                if pos16 == 0 || pos16 == 8 {
                    1.18
                } else if pos16 % 4 == 0 {
                    1.06
                } else {
                    0.90
                }
            }
            Self::HatChatter => {
                let h = seed
                    .wrapping_mul(0x9E37_79B9)
                    .wrapping_add(pos16 as u32 * 0x85EB_CA6B);
                let micro = ((h >> 8) & 0xFF) as f32 / 255.0;
                0.88 + micro * 0.28
            }
        };
        1.0 + (accent - 1.0) * d
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
    /// Non-destructive MPC / Pocket Groove template applied at playback & bounce time.
    #[serde(default)]
    pub groove_template: GrooveTemplate,
    #[serde(default = "default_groove_amount")]
    pub groove_amount: f32,
    /// When `true`, MIDI notes on this track automatically snap to the active global `ChordTrack` harmony!
    #[serde(default)]
    pub follow_chord_track: bool,
    /// Optional VCA Console Fader Group controlling this track's post-automation gain & mute/solo!
    #[serde(default)]
    pub vca_group_id: Option<Uuid>,
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
            groove_template: GrooveTemplate::Straight,
            groove_amount: 1.0,
            follow_chord_track: false,
            vca_group_id: None,
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
            height_px: 108.0,
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
