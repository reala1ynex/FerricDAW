use crate::clip::{ClipId, SessionSlot, TimelineClip};
use crate::fx::{FxId, FxSlot, ModulatorSlot};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type TrackId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputSource {
    /// Hardware stereo or mono audio input + all MIDI inputs (REAPER effortless default).
    AudioAndMidi { stereo_pair: u8 },
    AudioMono { channel: u8 },
    MidiAll,
    /// Record the post-FX stereo output of this track / submix directly!
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

/// Arbitrary track-to-track routing (aux sends, sidechains, parallel compression buses).
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub beat: f64,
    pub normalized_value: f32, // 0.0 ..= 1.0
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutomationTarget {
    Volume,
    Pan,
    FxParam { fx_id: FxId, param_index: usize },
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
                let t = ((beat - a.beat) / span) as f32;
                return Some(a.normalized_value + (b.normalized_value - a.normalized_value) * t);
            }
        }
        None
    }
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

/// REAPER's signature Universal Track: holds Audio clips, MIDI clips, Folder submix children,
/// Instrument & Audio FX inserts, Modulators, Session Clip slots, and arbitrary Sends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub color: [u8; 3],
    /// When `Some(folder_id)`, this track's master output feeds its parent folder track!
    pub parent_folder_id: Option<TrackId>,
    pub folder_collapsed: bool,
    pub send_to_master: bool,
    pub gain_db: f32,
    pub pan: f32, // -1.0 (L) ..= +1.0 (R)
    pub stereo_width: f32, // 0.0 (Mono) ..= 2.0 (Wide)
    pub mute: bool,
    pub solo: bool,
    pub record_arm: bool,
    pub input_monitoring: bool,
    pub phase_invert: bool,
    pub input_source: InputSource,
    pub fx_chain: Vec<FxSlot>,
    pub modulators: Vec<ModulatorSlot>,
    pub sends: Vec<TrackSend>,
    pub clips: Vec<TimelineClip>,
    pub session_slots: Vec<Option<SessionSlot>>,
    pub active_session_slot: Option<usize>,
    pub automation_lanes: Vec<AutomationLane>,
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
            fx_chain: Vec::new(),
            modulators: Vec::new(),
            sends: Vec::new(),
            clips: Vec::new(),
            session_slots: vec![None; num_scenes.max(4)],
            active_session_slot: None,
            automation_lanes: Vec::new(),
            height_px: 84.0,
        }
    }

    pub fn find_clip(&self, clip_id: ClipId) -> Option<&TimelineClip> {
        self.clips.iter().find(|c| c.id == clip_id)
    }

    pub fn find_clip_mut(&mut self, clip_id: ClipId) -> Option<&mut TimelineClip> {
        self.clips.iter_mut().find(|c| c.id == clip_id)
    }

    pub fn ensure_scene_count(&mut self, scenes: usize) {
        if self.session_slots.len() < scenes {
            self.session_slots.resize_with(scenes, || None);
        }
    }
}
