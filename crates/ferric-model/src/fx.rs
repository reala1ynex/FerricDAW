use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type FxId = Uuid;
pub type ModulatorId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FxKind {
    // Instruments (MIDI -> Audio, can live anywhere on a universal track!)
    PolySynth,
    DrumKit,
    // Studio Utility & Dynamics
    ChromaticTuner,
    NoiseGate,
    ParametricEq,
    Compressor,
    // Home Recording & Character
    AmpCabSim,
    TapeSaturator,
    // Time & Space
    StereoDelay,
    PlateHallReverb,
}

impl FxKind {
    pub const ALL: &'static [FxKind] = &[
        FxKind::PolySynth,
        FxKind::DrumKit,
        FxKind::ChromaticTuner,
        FxKind::NoiseGate,
        FxKind::ParametricEq,
        FxKind::Compressor,
        FxKind::AmpCabSim,
        FxKind::TapeSaturator,
        FxKind::StereoDelay,
        FxKind::PlateHallReverb,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            FxKind::PolySynth => "Ferric PolySynth (16-Voice VA/FM)",
            FxKind::DrumKit => "Ferric 8-Pad Studio Drum Kit",
            FxKind::ChromaticTuner => "Chromatic Strobe Tuner",
            FxKind::NoiseGate => "Hysteresis Studio Gate",
            FxKind::ParametricEq => "4-Band Parametric EQ + Analyzer",
            FxKind::Compressor => "Opto/VCA Studio Compressor",
            FxKind::AmpCabSim => "Ferric Tube Amp & Cab Modeler",
            FxKind::TapeSaturator => "Oxide Tape Saturator & Limiter",
            FxKind::StereoDelay => "Tempo Ping-Pong Tape Delay",
            FxKind::PlateHallReverb => "Lush Plate / Hall Reverb",
        }
    }

    pub fn short_badge(self) -> &'static str {
        match self {
            FxKind::PolySynth => "SYNTH",
            FxKind::DrumKit => "DRUMS",
            FxKind::ChromaticTuner => "TUNER",
            FxKind::NoiseGate => "GATE",
            FxKind::ParametricEq => "EQ-4",
            FxKind::Compressor => "COMP",
            FxKind::AmpCabSim => "AMP",
            FxKind::TapeSaturator => "TAPE",
            FxKind::StereoDelay => "DELAY",
            FxKind::PlateHallReverb => "REVERB",
        }
    }

    pub fn is_instrument(self) -> bool {
        matches!(self, FxKind::PolySynth | FxKind::DrumKit)
    }

    pub fn param_specs(self) -> &'static [ParamSpec] {
        const POLYSYNTH_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Waveform", "", 0.0, 3.0, 0.0), // 0=Saw, 1=Pulse, 2=Tri, 3=FM
            ParamSpec::new("Sub / Detune", "%", 0.0, 100.0, 28.0),
            ParamSpec::new("Cutoff", "Hz", 60.0, 16000.0, 2600.0),
            ParamSpec::new("Resonance", "", 0.05, 0.92, 0.28),
            ParamSpec::new("Filter Env", "%", -100.0, 100.0, 45.0),
            ParamSpec::new("Attack", "ms", 1.0, 1500.0, 12.0),
            ParamSpec::new("Decay", "ms", 10.0, 2500.0, 320.0),
            ParamSpec::new("Sustain", "%", 0.0, 100.0, 65.0),
            ParamSpec::new("Release", "ms", 10.0, 3500.0, 280.0),
            ParamSpec::new("Output", "dB", -24.0, 12.0, -3.0),
        ];
        const DRUMKIT_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Kick Punch", "%", 0.0, 100.0, 68.0),
            ParamSpec::new("Kick Tune", "Hz", 36.0, 85.0, 52.0),
            ParamSpec::new("Snare Snap", "%", 0.0, 100.0, 64.0),
            ParamSpec::new("Hat Decay", "ms", 20.0, 650.0, 110.0),
            ParamSpec::new("Room Glue", "%", 0.0, 100.0, 22.0),
            ParamSpec::new("Kit Level", "dB", -24.0, 12.0, 0.0),
        ];
        const TUNER_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Ref A4", "Hz", 432.0, 446.0, 440.0),
            ParamSpec::new("Mute Thru", "", 0.0, 1.0, 0.0),
        ];
        const GATE_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Open Thresh", "dB", -80.0, -6.0, -42.0),
            ParamSpec::new("Hysteresis", "dB", 1.0, 18.0, 6.0),
            ParamSpec::new("Attack", "ms", 0.2, 50.0, 1.5),
            ParamSpec::new("Hold", "ms", 5.0, 500.0, 45.0),
            ParamSpec::new("Release", "ms", 10.0, 1000.0, 120.0),
            ParamSpec::new("Floor", "dB", -90.0, 0.0, -72.0),
        ];
        const EQ_SPECS: &[ParamSpec] = &[
            ParamSpec::new("HPF Cutoff", "Hz", 20.0, 600.0, 35.0),
            ParamSpec::new("Low Freq", "Hz", 40.0, 500.0, 110.0),
            ParamSpec::new("Low Gain", "dB", -18.0, 18.0, 0.0),
            ParamSpec::new("Mid 1 Freq", "Hz", 180.0, 4000.0, 650.0),
            ParamSpec::new("Mid 1 Gain", "dB", -18.0, 18.0, 0.0),
            ParamSpec::new("Mid 1 Q", "", 0.3, 10.0, 1.2),
            ParamSpec::new("Mid 2 Freq", "Hz", 800.0, 10000.0, 2800.0),
            ParamSpec::new("Mid 2 Gain", "dB", -18.0, 18.0, 0.0),
            ParamSpec::new("Mid 2 Q", "", 0.3, 10.0, 1.4),
            ParamSpec::new("High Freq", "Hz", 2500.0, 16000.0, 8500.0),
            ParamSpec::new("High Gain", "dB", -18.0, 18.0, 0.0),
            ParamSpec::new("Output", "dB", -18.0, 18.0, 0.0),
        ];
        const COMP_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Threshold", "dB", -48.0, 0.0, -18.0),
            ParamSpec::new("Ratio", ":1", 1.0, 20.0, 4.0),
            ParamSpec::new("Attack", "ms", 0.2, 120.0, 8.0),
            ParamSpec::new("Release", "ms", 15.0, 1200.0, 110.0),
            ParamSpec::new("Knee", "dB", 0.0, 18.0, 4.0),
            ParamSpec::new("Makeup", "dB", -6.0, 24.0, 3.0),
        ];
        const AMP_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Drive", "dB", 0.0, 36.0, 14.0),
            ParamSpec::new("Cab Model", "", 0.0, 3.0, 1.0),
            ParamSpec::new("Bass", "dB", -12.0, 12.0, 1.5),
            ParamSpec::new("Mid", "dB", -12.0, 12.0, 1.0),
            ParamSpec::new("Treble", "dB", -12.0, 12.0, 2.0),
            ParamSpec::new("Presence", "%", 0.0, 100.0, 45.0),
            ParamSpec::new("Output", "dB", -24.0, 12.0, -3.0),
        ];
        const TAPE_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Tape Drive", "dB", 0.0, 24.0, 5.0),
            ParamSpec::new("Oxide Warmth", "%", 0.0, 100.0, 42.0),
            ParamSpec::new("Ceiling", "dB", -12.0, 0.0, -0.3),
        ];
        const DELAY_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Time", "ms", 20.0, 1500.0, 375.0),
            ParamSpec::new("Sync Beats", "", 0.0, 4.0, 2.0),
            ParamSpec::new("Feedback", "%", 0.0, 92.0, 38.0),
            ParamSpec::new("Ping-Pong", "%", 0.0, 100.0, 65.0),
            ParamSpec::new("Tape Tone", "Hz", 800.0, 12000.0, 4200.0),
            ParamSpec::new("Flutter", "%", 0.0, 100.0, 18.0),
        ];
        const REVERB_SPECS: &[ParamSpec] = &[
            ParamSpec::new("Pre-Delay", "ms", 0.0, 120.0, 18.0),
            ParamSpec::new("Decay", "%", 10.0, 98.0, 74.0),
            ParamSpec::new("Damping", "%", 0.0, 95.0, 38.0),
            ParamSpec::new("Low Cut", "Hz", 20.0, 600.0, 120.0),
            ParamSpec::new("Width", "%", 0.0, 100.0, 100.0),
        ];

        match self {
            FxKind::PolySynth => POLYSYNTH_SPECS,
            FxKind::DrumKit => DRUMKIT_SPECS,
            FxKind::ChromaticTuner => TUNER_SPECS,
            FxKind::NoiseGate => GATE_SPECS,
            FxKind::ParametricEq => EQ_SPECS,
            FxKind::Compressor => COMP_SPECS,
            FxKind::AmpCabSim => AMP_SPECS,
            FxKind::TapeSaturator => TAPE_SPECS,
            FxKind::StereoDelay => DELAY_SPECS,
            FxKind::PlateHallReverb => REVERB_SPECS,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub name: &'static str,
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

impl ParamSpec {
    pub const fn new(
        name: &'static str,
        unit: &'static str,
        min: f32,
        max: f32,
        default: f32,
    ) -> Self {
        Self {
            name,
            unit,
            min,
            max,
            default,
        }
    }

    pub fn normalize(&self, value: f32) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            0.0
        } else {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    pub fn denormalize(&self, norm: f32) -> f32 {
        self.min + norm.clamp(0.0, 1.0) * (self.max - self.min)
    }
}

/// Every FX slot has REAPER's signature universal Wet/Dry blend knob + bypass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxSlot {
    pub id: FxId,
    pub kind: FxKind,
    pub enabled: bool,
    /// REAPER-style universal wet/dry knob on every insert (0.0 ..= 1.0).
    pub wet_mix: f32,
    pub params: Vec<f32>,
    pub preset_name: String,
}

impl FxSlot {
    pub fn new(kind: FxKind) -> Self {
        let specs = kind.param_specs();
        let params = specs.iter().map(|s| s.default).collect();
        let wet_mix = match kind {
            FxKind::StereoDelay => 0.30,
            FxKind::PlateHallReverb => 0.25,
            _ => 1.0,
        };
        Self {
            id: Uuid::new_v4(),
            kind,
            enabled: true,
            wet_mix,
            params,
            preset_name: "Init".to_string(),
        }
    }

    pub fn param(&self, idx: usize) -> f32 {
        self.params
            .get(idx)
            .copied()
            .unwrap_or_else(|| self.kind.param_specs().get(idx).map(|s| s.default).unwrap_or(0.0))
    }

    pub fn set_param(&mut self, idx: usize, val: f32) {
        let specs = self.kind.param_specs();
        if self.params.len() < specs.len() {
            self.params.resize_with(specs.len(), || 0.0);
        }
        if let (Some(slot), Some(spec)) = (self.params.get_mut(idx), specs.get(idx)) {
            *slot = val.clamp(spec.min, spec.max);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LfoShape {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleAndHold,
}

impl LfoShape {
    pub const ALL: &'static [LfoShape] = &[
        LfoShape::Sine,
        LfoShape::Triangle,
        LfoShape::Saw,
        LfoShape::Square,
        LfoShape::SampleAndHold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LfoShape::Sine => "Sine",
            LfoShape::Triangle => "Triangle",
            LfoShape::Saw => "Ramp Saw",
            LfoShape::Square => "Pulse",
            LfoShape::SampleAndHold => "S&H Random",
        }
    }
}

/// Bitwig-style track-level modulator sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModulatorKind {
    Lfo {
        shape: LfoShape,
        rate_hz: f32,
        tempo_sync: bool,
        sync_beats: f32,
        bipolar: bool,
    },
    EnvelopeFollower {
        attack_ms: f32,
        release_ms: f32,
        sensitivity_db: f32,
    },
    StepSequencer {
        steps: [f32; 8],
        step_beats: f32,
        smoothing: f32,
    },
    Macro {
        value: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModTarget {
    TrackGain,
    TrackPan,
    FxParam { fx_id: FxId, param_index: usize },
    FxWet { fx_id: FxId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModRouting {
    pub target: ModTarget,
    /// Bipolar normalized modulation amount (-1.0 ..= 1.0).
    pub depth: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulatorSlot {
    pub id: ModulatorId,
    pub name: String,
    pub enabled: bool,
    pub kind: ModulatorKind,
    pub routings: Vec<ModRouting>,
}

impl ModulatorSlot {
    pub fn new_lfo(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: true,
            kind: ModulatorKind::Lfo {
                shape: LfoShape::Sine,
                rate_hz: 0.8,
                tempo_sync: true,
                sync_beats: 2.0,
                bipolar: true,
            },
            routings: Vec::new(),
        }
    }

    pub fn new_env_follower(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: true,
            kind: ModulatorKind::EnvelopeFollower {
                attack_ms: 8.0,
                release_ms: 140.0,
                sensitivity_db: 12.0,
            },
            routings: Vec::new(),
        }
    }

    pub fn new_macro(name: impl Into<String>, value: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: true,
            kind: ModulatorKind::Macro {
                value: value.clamp(0.0, 1.0),
            },
            routings: Vec::new(),
        }
    }
}
