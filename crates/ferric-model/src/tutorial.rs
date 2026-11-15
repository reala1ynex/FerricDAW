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
                "We’ll build a short dark-techno track that should match demo/NightCurrent-Demo.wav. Close anytime (✕ / Esc) — nothing starts until you hit Begin Session."
            }
            Self::Session => {
                "Fresh project titled Night Current, 128 BPM, loop bars 1–16. Skip Afterglow Copper for this."
            }
            Self::Chords => {
                "Turn on Chord Lane and lay Cm → Ab → Eb → Bb (one bar each) across 16 bars."
            }
            Self::Harmony => {
                "Generate Pad + Bass + Arp, rename the tracks, keep arp only on Stack/Hook B, keep the bed dark."
            }
            Self::Drums => {
                "Add a Drum Kit: sparse kick on bars 1–2, four-on-the-floor from bar 3, claps on 2/4, soft hats."
            }
            Self::Hook => {
                "PolySynth hook an octave down — Hook A at bar 5, Hook B at bar 13 — filtered, light delay/reverb."
            }
            Self::Counter => {
                "Add a higher answering Counter on bars 9–12 (softer under 13–16). Check the layout table."
            }
            Self::Sidechain => {
                "Compressor on Pads (and Bass) keyed to Drums — pads should duck a little with the kick."
            }
            Self::Mix => {
                "Kick loudest → bass → hook clear → pads/arp tucked. Store Mix A."
            }
            Self::Bounce => {
                "Ctrl+B · beats 0→64 · export NightCurrent_Master.wav · A/B against the demo WAV."
            }
            Self::Done => {
                "Hit Space, bounce if you haven’t yet, and compare to demo/NightCurrent-Demo.wav. You can reopen this anytime."
            }
        }
    }

    pub fn how_to(self) -> &'static str {
        match self {
            Self::Welcome => {
                "Opens only when you ask (🎓 Tutorial). Begin Session makes a clean Night Current project. Do this step runs one step for you — or close this and work on your own."
            }
            Self::Session => {
                "Ferric menu → New Empty Session, set BPM to 128, save as NightCurrent.ferric — or hit Do this step."
            }
            Self::Chords => {
                "Toggle Chord Lane above the timeline. Drop the Cm–Ab–Eb–Bb loop for 16 bars — or Do this step."
            }
            Self::Harmony => {
                "Chord Lane toolbar → Generate Arr (Pad + Bass + Arp). Rename to 03 Pads / 02 Bass / 04 Arp. Trim arp to bars 9–16."
            }
            Self::Drums => {
                "Browser → Drum Kit, name it 01 Drums — or Do this step for the tutorial grid."
            }
            Self::Hook => {
                "Add PolySynth as 05 Hook. Draw the phrase at bar 5 (8 bars) and copy to bar 13. Keep cutoff low."
            }
            Self::Counter => {
                "Add 06 Counter with a higher answering line on the Stack section — or Do this step."
            }
            Self::Sidechain => {
                "Select 03 Pads → FX → Compressor → Sidechain source = 01 Drums. Same idea on Bass."
            }
            Self::Mix => {
                "Open the mixer in the bottom dock. Balance levels, then capture Mix A."
            }
            Self::Bounce => {
                "Ctrl+B (or Ferric menu → Bounce). Range 0–64 beats, WAV, −14 LUFS."
            }
            Self::Done => "Close this panel or hit Restart to build it again.",
        }
    }

    pub fn ui_hint(self) -> &'static str {
        match self {
            Self::Welcome => "This panel stays on the right while you work.",
            Self::Session => "Look: top transport BPM · Ferric menu",
            Self::Chords => "Look: Chord Lane toggle above the arrangement",
            Self::Harmony => "Look: Chord Lane → Gen / Generate Arr",
            Self::Drums => "Look: left Browser · track headers",
            Self::Hook => "Look: Browser PolySynth · piano roll / MIDI clips",
            Self::Counter => "Look: layout table in this panel",
            Self::Sidechain => "Look: bottom dock → FX · Compressor sidechain",
            Self::Mix => "Look: bottom dock → Mixer · Mix A/B/C/D",
            Self::Bounce => "Look: Ctrl+B bounce dialog",
            Self::Done => "Look: Space to play · demo WAV in the zip",
        }
    }
}

/// Checkpoint — progress flags first, then structural heuristics.
pub fn night_current_step_complete(proj: &Project, step: NightCurrentStep) -> bool {
    if proj.tutorial_step_reached >= step.index() as u8 {
        return match step {
            NightCurrentStep::Bounce | NightCurrentStep::Done => proj.tutorial_bounce_done,
            _ => true,
        };
    }
    match step {
        NightCurrentStep::Welcome => true,
        NightCurrentStep::Session => {
            (proj.bpm - 128.0).abs() < 0.01
                && proj.loop_enabled
                && (proj.loop_end_beat - 64.0).abs() < 0.01
                && (proj.title.to_lowercase().contains("night")
                    || proj.tutorial_step_reached >= NightCurrentStep::Session.index() as u8)
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
        NightCurrentStep::Bounce | NightCurrentStep::Done => proj.tutorial_bounce_done,
    }
}

fn track_named<'a>(proj: &'a Project, name: &str) -> Option<&'a crate::track::Track> {
    proj.tracks.iter().find(|t| t.name == name)
}
