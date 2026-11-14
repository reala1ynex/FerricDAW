use crate::clip::{MidiCcPoint, MidiCcTarget, MidiNote};
use midly::{
    num::{u15, u24, u28, u4, u7},
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const NOTE_PITCH_CLASSES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MusicalScale {
    Chromatic,
    Major,
    NaturalMinor,
    HarmonicMinor,
    Dorian,
    Mixolydian,
    MinorPentatonic,
    Blues,
}

impl MusicalScale {
    pub const ALL: &'static [MusicalScale] = &[
        MusicalScale::Chromatic,
        MusicalScale::Major,
        MusicalScale::NaturalMinor,
        MusicalScale::HarmonicMinor,
        MusicalScale::Dorian,
        MusicalScale::Mixolydian,
        MusicalScale::MinorPentatonic,
        MusicalScale::Blues,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MusicalScale::Chromatic => "Chromatic (12-Tone)",
            MusicalScale::Major => "Major (Ionian)",
            MusicalScale::NaturalMinor => "Natural Minor (Aeolian)",
            MusicalScale::HarmonicMinor => "Harmonic Minor",
            MusicalScale::Dorian => "Dorian",
            MusicalScale::Mixolydian => "Mixolydian",
            MusicalScale::MinorPentatonic => "Minor Pentatonic",
            MusicalScale::Blues => "Blues Hexatonic",
        }
    }

    /// 12-bit pitch-class mask relative to root (bit `i` = semitone `i` is in scale).
    pub fn degree_mask(self) -> u16 {
        match self {
            MusicalScale::Chromatic => 0b1111_1111_1111,
            MusicalScale::Major => 0b1010_1011_0101,           // 0,2,4,5,7,9,11
            MusicalScale::NaturalMinor => 0b0101_1010_1101,    // 0,2,3,5,7,8,10
            MusicalScale::HarmonicMinor => 0b1001_1010_1101,   // 0,2,3,5,7,8,11
            MusicalScale::Dorian => 0b0110_1010_1101,          // 0,2,3,5,7,9,10
            MusicalScale::Mixolydian => 0b0110_1011_0101,      // 0,2,4,5,7,9,10
            MusicalScale::MinorPentatonic => 0b0100_1010_1001, // 0,3,5,7,10
            MusicalScale::Blues => 0b0100_1110_1001,           // 0,3,5,6,7,10
        }
    }

    #[inline]
    pub fn contains_pitch(self, root_pc: u8, midi_pitch: u8) -> bool {
        let rel = (midi_pitch as i32 - (root_pc % 12) as i32).rem_euclid(12) as u16;
        (self.degree_mask() & (1 << rel)) != 0
    }

    /// Snaps `midi_pitch` to the nearest allowed note in `(root_pc, self)`.
    #[inline]
    pub fn snap_pitch(self, root_pc: u8, midi_pitch: u8) -> u8 {
        if self.contains_pitch(root_pc, midi_pitch) {
            return midi_pitch;
        }
        for dist in 1..=6i32 {
            let down = (midi_pitch as i32 - dist).clamp(0, 127) as u8;
            if self.contains_pitch(root_pc, down) {
                return down;
            }
            let up = (midi_pitch as i32 + dist).clamp(0, 127) as u8;
            if self.contains_pitch(root_pc, up) {
                return up;
            }
        }
        midi_pitch
    }

    /// Snaps a continuous MIDI note float (e.g. `69.34`) to the nearest scale degree!
    #[inline]
    pub fn snap_continuous_midi(self, root_pc: u8, exact_midi: f32) -> f32 {
        if self == MusicalScale::Chromatic {
            return exact_midi.round();
        }
        let center = exact_midi.round() as i32;
        let mut best_pitch = center;
        let mut best_dist = f32::MAX;
        for cand in (center - 6)..=(center + 6) {
            if !(0..=127).contains(&cand) {
                continue;
            }
            if self.contains_pitch(root_pc, cand as u8) {
                let d = (exact_midi - cand as f32).abs();
                if d < best_dist {
                    best_dist = d;
                    best_pitch = cand;
                }
            }
        }
        best_pitch as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChordStamp {
    SingleNote,
    Major,
    Minor,
    Maj7,
    Min7,
    Min9,
    Dom7,
    Sus4,
    Power5,
}

impl ChordStamp {
    pub const ALL: &'static [ChordStamp] = &[
        ChordStamp::SingleNote,
        ChordStamp::Major,
        ChordStamp::Minor,
        ChordStamp::Maj7,
        ChordStamp::Min7,
        ChordStamp::Min9,
        ChordStamp::Dom7,
        ChordStamp::Sus4,
        ChordStamp::Power5,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChordStamp::SingleNote => "Single Note",
            ChordStamp::Major => "Major Triad",
            ChordStamp::Minor => "Minor Triad",
            ChordStamp::Maj7 => "Maj7 Lush",
            ChordStamp::Min7 => "Min7 Neo-Soul",
            ChordStamp::Min9 => "Min9 Studio",
            ChordStamp::Dom7 => "Dom7 Blues",
            ChordStamp::Sus4 => "Sus4 Open",
            ChordStamp::Power5 => "Power 5th (Guitar)",
        }
    }

    pub fn intervals(self) -> &'static [u8] {
        match self {
            ChordStamp::SingleNote => &[0],
            ChordStamp::Major => &[0, 4, 7],
            ChordStamp::Minor => &[0, 3, 7],
            ChordStamp::Maj7 => &[0, 4, 7, 11],
            ChordStamp::Min7 => &[0, 3, 7, 10],
            ChordStamp::Min9 => &[0, 3, 7, 10, 14],
            ChordStamp::Dom7 => &[0, 4, 7, 10],
            ChordStamp::Sus4 => &[0, 5, 7],
            ChordStamp::Power5 => &[0, 7, 12],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportedMidiTrack {
    pub name: String,
    pub notes: Vec<MidiNote>,
    pub cc_points: Vec<MidiCcPoint>,
    pub detected_bpm: Option<f64>,
}

/// Imports Standard MIDI Files (`.mid` / `.midi`, Format 0 or Format 1) into FerricDAW tracks!
pub fn import_smf_file(path: &Path) -> Result<Vec<ImportedMidiTrack>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Read error: {}", e))?;
    let smf = Smf::parse(&bytes).map_err(|e| format!("SMF parse error: {}", e))?;

    let ppq = match smf.header.timing {
        Timing::Metrical(ticks) => ticks.as_int().max(1) as f64,
        Timing::Timecode(fps, sub) => (fps.as_f32() * sub as f32).max(24.0) as f64,
    };

    let mut out = Vec::new();
    let mut global_bpm: Option<f64> = None;

    for (t_idx, events) in smf.tracks.iter().enumerate() {
        let mut tick_cursor: u64 = 0;
        let mut track_name = format!("MIDI Track {}", t_idx + 1);
        let mut active: [Option<(f64, u8, u8)>; 128] = [None; 128];
        let mut notes = Vec::new();
        let mut cc_points = Vec::new();

        for ev in events {
            tick_cursor += ev.delta.as_int() as u64;
            let beat = tick_cursor as f64 / ppq;

            match ev.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(raw)) => {
                    if let Ok(s) = std::str::from_utf8(raw) {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            track_name = trimmed.to_string();
                        }
                    }
                }
                TrackEventKind::Meta(MetaMessage::Tempo(us_per_quarter)) => {
                    let us = us_per_quarter.as_int().max(1) as f64;
                    global_bpm = Some((60_000_000.0 / us).clamp(20.0, 300.0));
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } => {
                            let p = key.as_int() as usize;
                            let v = vel.as_int();
                            if v > 0 {
                                if let Some((prev_b, prev_v, prev_ch)) = active[p].take() {
                                    let mut n = MidiNote::new(
                                        p as u8,
                                        prev_v,
                                        prev_b,
                                        (beat - prev_b).max(0.0625),
                                    );
                                    n.channel = prev_ch;
                                    notes.push(n);
                                }
                                active[p] = Some((beat, v, ch));
                            } else if let Some((on_b, on_v, on_ch)) = active[p].take() {
                                let mut n = MidiNote::new(
                                    p as u8,
                                    on_v,
                                    on_b,
                                    (beat - on_b).max(0.0625),
                                );
                                n.channel = on_ch;
                                notes.push(n);
                            }
                        }
                        MidiMessage::NoteOff { key, .. } => {
                            let p = key.as_int() as usize;
                            if let Some((on_b, on_v, on_ch)) = active[p].take() {
                                let mut n = MidiNote::new(
                                    p as u8,
                                    on_v,
                                    on_b,
                                    (beat - on_b).max(0.0625),
                                );
                                n.channel = on_ch;
                                notes.push(n);
                            }
                        }
                        MidiMessage::Controller { controller, value } => {
                            let cc_num = controller.as_int();
                            let val_norm = value.as_int() as f32 / 127.0;
                            let target = match cc_num {
                                1 => Some(MidiCcTarget::ModWheel),
                                11 => Some(MidiCcTarget::Expression),
                                64 => Some(MidiCcTarget::SustainPedal),
                                74 => Some(MidiCcTarget::FilterBrightness),
                                _ => None,
                            };
                            if let Some(tgt) = target {
                                cc_points.push(MidiCcPoint {
                                    beat,
                                    target: tgt,
                                    value: val_norm,
                                });
                            }
                        }
                        MidiMessage::PitchBend { bend } => {
                            let norm = ((bend.as_f32() + 1.0) * 0.5).clamp(0.0, 1.0);
                            cc_points.push(MidiCcPoint {
                                beat,
                                target: MidiCcTarget::PitchBend,
                                value: norm,
                            });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Flush any unclosed notes
        for (p, slot) in active.iter().enumerate() {
            if let Some(&(on_b, on_v, on_ch)) = slot.as_ref() {
                let mut n = MidiNote::new(p as u8, on_v, on_b, 0.5);
                n.channel = on_ch;
                notes.push(n);
            }
        }

        if !notes.is_empty() {
            out.push(ImportedMidiTrack {
                name: track_name,
                notes,
                cc_points,
                detected_bpm: global_bpm,
            });
        }
    }

    Ok(out)
}

/// Exports a MIDI clip (notes + CC/Pitch-Bend automation) to a Standard `.mid` file!
pub fn export_smf_clip(
    path: &Path,
    bpm: f64,
    notes: &[MidiNote],
    cc_points: &[MidiCcPoint],
) -> Result<(), String> {
    let ppq: u16 = 480;
    let header = Header::new(Format::SingleTrack, Timing::Metrical(u15::new(ppq)));

    #[derive(Clone)]
    struct RawEv {
        tick: u64,
        kind: TrackEventKind<'static>,
    }

    let mut raw = Vec::with_capacity(notes.len() * 2 + cc_points.len() + 2);
    let us_per_beat = (60_000_000.0 / bpm.clamp(20.0, 300.0)).round() as u32;
    raw.push(RawEv {
        tick: 0,
        kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us_per_beat.min(0xFF_FFFF)))),
    });

    for n in notes {
        let on_tick = (n.start_beat.max(0.0) * ppq as f64).round() as u64;
        let off_tick = (n.end_beat().max(n.start_beat + 0.03) * ppq as f64).round() as u64;
        let ch = u4::new(n.channel.min(15));
        let key = u7::new(n.pitch.min(127));
        let vel = u7::new(n.velocity.clamp(1, 127));
        raw.push(RawEv {
            tick: on_tick,
            kind: TrackEventKind::Midi {
                channel: ch,
                message: MidiMessage::NoteOn { key, vel },
            },
        });
        raw.push(RawEv {
            tick: off_tick,
            kind: TrackEventKind::Midi {
                channel: ch,
                message: MidiMessage::NoteOff {
                    key,
                    vel: u7::new(0),
                },
            },
        });
    }

    for pt in cc_points {
        let tick = (pt.beat.max(0.0) * ppq as f64).round() as u64;
        let kind = match pt.target {
            MidiCcTarget::PitchBend => {
                let raw14 = ((pt.value.clamp(0.0, 1.0) * 16383.0).round() as u16).min(16383);
                TrackEventKind::Midi {
                    channel: u4::new(0),
                    message: MidiMessage::PitchBend {
                        bend: midly::PitchBend(midly::num::u14::new(raw14)),
                    },
                }
            }
            other => {
                let cc_num = match other {
                    MidiCcTarget::ModWheel => 1,
                    MidiCcTarget::Expression => 11,
                    MidiCcTarget::SustainPedal => 64,
                    MidiCcTarget::FilterBrightness => 74,
                    MidiCcTarget::PitchBend => 1,
                };
                TrackEventKind::Midi {
                    channel: u4::new(0),
                    message: MidiMessage::Controller {
                        controller: u7::new(cc_num),
                        value: u7::new((pt.value.clamp(0.0, 1.0) * 127.0).round() as u8),
                    },
                }
            }
        };
        raw.push(RawEv { tick, kind });
    }

    raw.sort_by_key(|e| e.tick);

    let mut track_events = Vec::with_capacity(raw.len() + 1);
    let mut prev_tick = 0u64;
    for ev in raw {
        let delta = (ev.tick.saturating_sub(prev_tick)).min(0x0FFF_FFFF) as u32;
        prev_tick = ev.tick;
        track_events.push(TrackEvent {
            delta: u28::new(delta),
            kind: ev.kind,
        });
    }
    track_events.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    let smf = Smf {
        header,
        tracks: vec![track_events],
    };
    smf.save(path).map_err(|e| format!("SMF write error: {}", e))
}
