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
            Self::Welcome => "Night Current — let’s build it",
            Self::Session => "Start a clean 128 BPM session",
            Self::Chords => "Put the chord loop on the lane",
            Self::Harmony => "Generate pads, bass, and arp",
            Self::Drums => "Program the drums",
            Self::Hook => "Write the dark synth hook",
            Self::Counter => "Add the counter line",
            Self::Sidechain => "Pump the pads with the kick",
            Self::Mix => "Balance the mix",
            Self::Bounce => "Export the master",
            Self::Done => "You’re done — compare to the demo",
        }
    }

    pub fn goal(self) -> &'static str {
        match self {
            Self::Welcome => {
                "Make a ~30s dark-techno track that matches demo/NightCurrent-Demo.wav.\n\
                 Close this panel anytime (✕ or Esc). Nothing changes until you click Begin Session."
            }
            Self::Session => {
                "You want a saved project named Night Current at 128 BPM with the loop set to bars 1–16."
            }
            Self::Chords => {
                "Chord Lane should show Cm → Ab → Eb → Bb, one bar each, repeated for 16 bars."
            }
            Self::Harmony => {
                "Three harmony tracks exist (pads, bass, arp). Arp only plays in the later half; pads stay quiet."
            }
            Self::Drums => {
                "A drum track with a thin intro (bars 1–2), then a solid four-on-the-floor groove from bar 3."
            }
            Self::Hook => {
                "A filtered PolySynth lead: Hook A from bar 5, louder Hook B from bar 13 — an octave down, not bright."
            }
            Self::Counter => {
                "A higher answering melody on bars 9–12 (softer under 13–16), and the section mutes match the layout below."
            }
            Self::Sidechain => {
                "Pads (and bass) duck a little every kick — compressor sidechained to the drums."
            }
            Self::Mix => {
                "Kick sits on top, then bass, then a clear hook; pads/arp are quieter. Mix A is saved."
            }
            Self::Bounce => {
                "Export bars 1–16 as NightCurrent_Master.wav and A/B it against the demo."
            }
            Self::Done => {
                "Play your bounce next to demo/NightCurrent-Demo.wav. Reopen this tutorial anytime from 🎓 Tutorial."
            }
        }
    }

    pub fn how_to(self) -> &'static str {
        match self {
            Self::Welcome => {
                "1. Optional: click Play demo WAV later to hear the target.\n\
                 2. Click Begin Session — that loads a clean Night Current project.\n\
                 3. Or close this and follow docs/BEGINNER_GUIDE.md by hand.\n\
                 Stuck? Use “Do this step for me” on later steps — it rebuilds the song through that step."
            }
            Self::Session => {
                "1. Ferric menu → New Empty Session (skip Afterglow if it’s open).\n\
                 2. In the top bar, set BPM to 128.\n\
                 3. Ctrl+S → save as NightCurrent.ferric.\n\
                 4. Turn Loop on and set the loop to bars 1–16 (beats 0–64).\n\
                 Shortcut: click “Do this step for me”."
            }
            Self::Chords => {
                "1. Above the timeline, turn on Chord Lane.\n\
                 2. From bar 1, place: Cm, Ab, Eb, Bb — one bar each.\n\
                 3. Repeat that 4-chord loop until you fill 16 bars.\n\
                 Shortcut: “Do this step for me”."
            }
            Self::Harmony => {
                "1. On the Chord Lane toolbar, click Gen / Generate Arr (Pad + Bass + Arp).\n\
                 2. Rename the new tracks to: 03 Pads, 02 Bass, 04 Arp.\n\
                 3. Mute or trim 04 Arp so it only plays bars 9–16.\n\
                 4. Pull pads down in the mixer a bit — bed, not a blanket.\n\
                 Shortcut: “Do this step for me”."
            }
            Self::Drums => {
                "1. Left Browser → add Studio Drum Kit. Name the track 01 Drums.\n\
                 2. Bars 1–2: kick only on beats 1 and 3.\n\
                 3. Bars 3–16: kick every beat; clap/snare on 2 and 4.\n\
                 4. Add soft hats — keep them quiet.\n\
                 Shortcut: “Do this step for me” fills the tutorial grid."
            }
            Self::Hook => {
                "1. Browser → PolySynth (or Wavetable). Name it 05 Hook.\n\
                 2. At bar 5, draw ~8 bars of MIDI an octave down, e.g.\n\
                    C3 G2 Bb2 C3 | Eb3 C3 G2 | Ab2 Bb2 C3 | G2 (hold).\n\
                 3. Copy that clip to bar 13 (Hook B) and nudge it a bit louder.\n\
                 4. Lower the synth cutoff. Add a short Delay + light Reverb on the track FX.\n\
                 Shortcut: “Do this step for me”."
            }
            Self::Counter => {
                "1. Add another synth/MIDI track named 06 Counter.\n\
                 2. Write a higher answering phrase on bars 9–12.\n\
                 3. Keep it softer (or shorter) under bars 13–16.\n\
                 4. Mute/unmute tracks per the layout lines below so sections breathe.\n\
                 Shortcut: “Do this step for me”."
            }
            Self::Sidechain => {
                "1. Select track 03 Pads.\n\
                 2. Bottom dock → FX → add Compressor.\n\
                 3. Set the compressor’s Sidechain source to 01 Drums.\n\
                 4. Keep the amount gentle — pads should breathe, not choke.\n\
                 5. Optional: same idea on 02 Bass.\n\
                 Shortcut: “Do this step for me”."
            }
            Self::Mix => {
                "1. Open Mixer in the bottom dock.\n\
                 2. Raise 01 Drums (kick) highest, then 02 Bass, then 05 Hook.\n\
                 3. Tuck 03 Pads and 04 Arp underneath.\n\
                 4. Click Mix A to store the snapshot.\n\
                 Shortcut: “Do this step for me”."
            }
            Self::Bounce => {
                "1. Click Open Bounce… (or press Ctrl+B).\n\
                 2. Range: beats 0 → 64 (bars 1–16).\n\
                 3. Format: WAV, −14 LUFS on.\n\
                 4. Save as NightCurrent_Master.wav.\n\
                 5. Use Play demo WAV here, or Golden Bounce after export, to A/B."
            }
            Self::Done => {
                "1. Press Space and listen through once.\n\
                 2. Play demo WAV and flip between it and your bounce.\n\
                 3. Close this panel, or Restart tutorial to build again."
            }
        }
    }

    pub fn ui_hint(self) -> &'static str {
        match self {
            Self::Welcome => "Tip: this panel sits bottom-right so the timeline stays visible.",
            Self::Session => "Where: top transport (BPM) · Ferric menu · Ctrl+S",
            Self::Chords => "Where: Chord Lane toggle just above the arrangement",
            Self::Harmony => "Where: Chord Lane toolbar → Gen / Generate Arr",
            Self::Drums => "Where: left Browser → instruments · track name on the left of the lane",
            Self::Hook => "Where: Browser → PolySynth · double-click clip for piano roll",
            Self::Counter => "Where: + Track / Browser · mute buttons on track headers",
            Self::Sidechain => "Where: select Pads → bottom dock FX → Compressor → Sidechain",
            Self::Mix => "Where: bottom dock → Mixer · Mix A/B/C/D buttons",
            Self::Bounce => "Where: Ctrl+B or Ferric menu → Bounce",
            Self::Done => "Where: Space = play · Play demo WAV button above",
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
