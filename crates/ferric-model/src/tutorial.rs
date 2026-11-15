//! In-app **Night Current** guided tutorial — step cards + checkpoints.
//! Progressive project builds live in [`crate::project::build_tutorial_project_through`].

use crate::fx::FxKind;
use crate::project::Project;

/// Ordered coach steps for building Night Current end-to-end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NightCurrentStep {
    Welcome,
    Session,
    Chords,
    Harmony,
    Drums,
    Hook,
    Counter,
    Sidechain,
    Mix,
    Bounce,
    Done,
}

impl NightCurrentStep {
    pub const ALL: &'static [NightCurrentStep] = &[
        Self::Welcome,
        Self::Session,
        Self::Chords,
        Self::Harmony,
        Self::Drums,
        Self::Hook,
        Self::Counter,
        Self::Sidechain,
        Self::Mix,
        Self::Bounce,
        Self::Done,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&s| s == self).unwrap_or(0)
    }

    pub fn from_index(i: usize) -> Self {
        Self::ALL.get(i).copied().unwrap_or(Self::Done)
    }

    pub fn next(self) -> Self {
        Self::from_index(self.index().saturating_add(1).min(Self::ALL.len() - 1))
    }

    pub fn prev(self) -> Self {
        Self::from_index(self.index().saturating_sub(1))
    }

    /// Last step that mutates the project (Bounce/Done are coach-only).
    pub fn is_build_step(self) -> bool {
        matches!(
            self,
            Self::Session
                | Self::Chords
                | Self::Harmony
                | Self::Drums
                | Self::Hook
                | Self::Counter
                | Self::Sidechain
                | Self::Mix
        )
    }

    pub fn letter(self) -> &'static str {
        match self {
            Self::Welcome => "★",
            Self::Session => "A",
            Self::Chords => "B",
            Self::Harmony => "C",
            Self::Drums => "D",
            Self::Hook => "E",
            Self::Counter => "F",
            Self::Sidechain => "G/H",
            Self::Mix => "I",
            Self::Bounce => "J",
            Self::Done => "✓",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome — Night Current",
            Self::Session => "New session @ 128 BPM",
            Self::Chords => "Chord Lane loop",
            Self::Harmony => "Generate Pads / Bass / Arp",
            Self::Drums => "Techno drum grid",
            Self::Hook => "Dark synth hook",
            Self::Counter => "Counter + section layout",
            Self::Sidechain => "Sidechain pump",
            Self::Mix => "Balance + Mix A",
            Self::Bounce => "Bounce the master",
            Self::Done => "You built Night Current",
        }
    }

    pub fn goal(self) -> &'static str {
        match self {
            Self::Welcome => {
                "Optional coach for the dark-techno song (same recipe as demo/NightCurrent-Demo.wav). Close anytime with ✕ / Esc / Close tutorial — nothing starts until you click Begin Session."
            }
            Self::Session => {
                "Clean session titled Night Current, 128 BPM, loop bars 1–16. Ignore Afterglow Copper."
            }
            Self::Chords => {
                "Enable Chord Lane and lay Cm → Ab → Eb → Bb (1 bar each) across 16 bars."
            }
            Self::Harmony => {
                "Generate Arr (Pad + Bass + Arp), rename tracks, keep arp only on Stack/Hook B, darken the bed."
            }
            Self::Drums => {
                "Add Drum Kit: sparse kick bars 1–2, full four-on-floor from bar 3, claps on 2/4, soft hats."
            }
            Self::Hook => {
                "PolySynth hook an octave down — Hook A at bar 5, Hook B at bar 13 — filtered, light delay/reverb."
            }
            Self::Counter => {
                "Add answering Counter on bars 9–12 (softer under 13–16). Check the mute/layout table."
            }
            Self::Sidechain => {
                "Compressor on Pads (and Bass) sidechained to Drums — pads should breathe with the kick."
            }
            Self::Mix => {
                "Kick loudest → bass → hook clear → pads/arp tucked. Store Mix A."
            }
            Self::Bounce => {
                "Ctrl+B · beats 0→64 · export NightCurrent_Master.wav · A/B against the demo WAV."
            }
            Self::Done => {
                "Play Space, bounce if you haven't, and compare to demo/NightCurrent-Demo.wav. You can re-open this coach anytime."
            }
        }
    }

    pub fn how_to(self) -> &'static str {
        match self {
            Self::Welcome => {
                "Only opens when you ask (🎓 Tutorial). Begin Session starts a clean Night Current project; Do this step applies one recipe step. Or ignore the coach and close it."
            }
            Self::Session => {
                "Ferric menu → New Empty Session, set BPM to 128, save as NightCurrent.ferric — or hit Do this step."
            }
            Self::Chords => {
                "Toggle Chord Lane above the timeline. Drop the Cm–Ab–Eb–Bb loop for 16 bars — or Do this step."
            }
            Self::Harmony => {
                "On the Chord Lane toolbar: Generate Arr (Pad + Bass + Arp). Rename 03 Pads / 02 Bass / 04 Arp. Trim arp to bars 9–16."
            }
            Self::Drums => {
                "Browser → Drum Kit track named 01 Drums, or Do this step for the tutorial grid."
            }
            Self::Hook => {
                "Add PolySynth track 05 Hook. Draw the phrase at bar 5 (8 bars) and copy to bar 13. Keep cutoff low."
            }
            Self::Counter => {
                "Add 06 Counter with a higher answering line on the Stack section — or Do this step."
            }
            Self::Sidechain => {
                "Select 03 Pads → FX → Compressor → set Sidechain source to 01 Drums. Same idea on Bass."
            }
            Self::Mix => {
                "Open Mixing Console (bottom dock). Balance levels, then capture Mix A."
            }
            Self::Bounce => {
                "Ctrl+B (or Ferric menu → Bounce). Range 0–64 beats, WAV, −14 LUFS."
            }
            Self::Done => "Close the coach or hit Restart to build it again.",
        }
    }

    pub fn ui_hint(self) -> &'static str {
        match self {
            Self::Welcome => "Coach stays on the right — keep it open while you work.",
            Self::Session => "Look: top transport BPM · Ferric menu",
            Self::Chords => "Look: Chord Lane toggle above arrangement",
            Self::Harmony => "Look: Chord Lane → Gen / Generate Arr",
            Self::Drums => "Look: left Browser instruments · timeline track headers",
            Self::Hook => "Look: Browser PolySynth · piano roll / MIDI clips",
            Self::Counter => "Look: arrangement mute table in the coach",
            Self::Sidechain => "Look: bottom dock → FX · Compressor sidechain",
            Self::Mix => "Look: bottom dock → Mixing Console · Mix A/B/C/D",
            Self::Bounce => "Look: Ctrl+B bounce modal",
            Self::Done => "Look: Space to play · demo WAV in the zip",
        }
    }
}

/// Soft checkpoint — true when the project already reflects this step.
pub fn night_current_step_complete(proj: &Project, step: NightCurrentStep) -> bool {
    match step {
        NightCurrentStep::Welcome => true,
        NightCurrentStep::Session => {
            (proj.bpm - 128.0).abs() < 0.01
                && proj.title.to_lowercase().contains("night")
                && proj.loop_enabled
                && (proj.loop_end_beat - 64.0).abs() < 0.01
        }
        NightCurrentStep::Chords => {
            proj.show_chord_lane && proj.chord_track.len() >= 16
        }
        NightCurrentStep::Harmony => {
            track_named(proj, "03 Pads").is_some()
                && track_named(proj, "02 Bass").is_some()
                && track_named(proj, "04 Arp").is_some()
        }
        NightCurrentStep::Drums => track_named(proj, "01 Drums").is_some(),
        NightCurrentStep::Hook => track_named(proj, "05 Hook").is_some(),
        NightCurrentStep::Counter => track_named(proj, "06 Counter").is_some(),
        NightCurrentStep::Sidechain => {
            track_named(proj, "03 Pads").is_some_and(|t| {
                t.fx_chain
                    .iter()
                    .any(|f| f.kind == FxKind::Compressor && f.sidechain_source.is_some())
            })
        }
        NightCurrentStep::Mix => proj.mix_snapshots.iter().any(|s| s.is_some()),
        NightCurrentStep::Bounce | NightCurrentStep::Done => {
            night_current_step_complete(proj, NightCurrentStep::Mix)
        }
    }
}

fn track_named<'a>(proj: &'a Project, name: &str) -> Option<&'a crate::track::Track> {
    proj.tracks.iter().find(|t| t.name == name)
}
