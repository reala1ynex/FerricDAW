pub mod clip;
pub mod fx;
pub mod project;
pub mod theory;
pub mod track;

pub use clip::{
    eval_midi_cc_curve, AudioBuffer, ClipId, CompSegment, FadeShape, MidiCcPoint, MidiCcTarget,
    MidiNote, SessionSlot, StretchMarker, Take, TakeContent, TakeId, TimeStretchMode, TimelineClip,
};
pub use fx::{
    ExternalPluginFormat, FactoryPreset, FxId, FxKind, FxSlot, LfoShape, ModRouting, ModTarget,
    ModulatorId, ModulatorKind, ModulatorSlot, ParamSpec, PluginParamDescriptor, FACTORY_PRESETS,
};
pub use project::{
    build_demo_project, synthesize_browser_sample, HistoryManager, MasterBus, MeterChange,
    MidiLearnMapping, ModelError, Project, Scene, SnapGrid, TempoPoint, TimeSignature,
    TimelineMarker,
};
pub use theory::{
    export_smf_clip, import_smf_file, ChordStamp, ImportedMidiTrack, MusicalScale,
    NOTE_PITCH_CLASSES,
};
pub use track::{
    apply_curve_tension, ArpMode, AutomationLane, AutomationMode, AutomationPoint,
    AutomationTarget, InputSource, SendTap, Track, TrackArpeggiator, TrackId, TrackSend,
    STUDIO_TRACK_COLORS,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swipe_comping_splits_and_merges_cleanly() {
        let mut clip = TimelineClip::new_midi("Vocal", 0.0, 16.0, vec![]);
        clip.takes.push(Take::new_midi("Take 2", vec![]));
        clip.takes.push(Take::new_midi("Take 3", vec![]));

        clip.set_comp_region(4.0, 12.0, 1);
        assert_eq!(clip.take_at_local_beat(2.0), 0);
        assert_eq!(clip.take_at_local_beat(6.0), 1);
        assert_eq!(clip.take_at_local_beat(14.0), 0);

        clip.set_comp_region(6.0, 8.0, 2);
        assert_eq!(clip.take_at_local_beat(5.0), 1);
        assert_eq!(clip.take_at_local_beat(7.0), 2);
        assert_eq!(clip.take_at_local_beat(10.0), 1);

        clip.set_comp_region(4.0, 12.0, 0);
        assert_eq!(clip.comp_segments.len(), 1);
        assert_eq!(clip.comp_segments[0].take_index, 0);
    }

    #[test]
    fn routing_dag_prevents_cycles_and_orders_buses_and_sidechains() {
        let mut proj = Project::new_empty("Routing Test");
        let gtr = proj.add_track("Guitar");
        let bus = proj.add_track("Guitar Bus");
        let rev = proj.add_track("Reverb Aux");

        proj.find_track_mut(gtr).unwrap().parent_folder_id = Some(bus);
        proj.add_send(bus, rev, -6.0).unwrap();

        assert!(proj.add_send(rev, gtr, 0.0).is_err());

        let order = proj.topological_track_order();
        let pos_gtr = order.iter().position(|&i| proj.tracks[i].id == gtr).unwrap();
        let pos_bus = order.iter().position(|&i| proj.tracks[i].id == bus).unwrap();
        let pos_rev = order.iter().position(|&i| proj.tracks[i].id == rev).unwrap();
        assert!(pos_gtr < pos_bus);
        assert!(pos_bus < pos_rev);
    }

    #[test]
    fn smf_midi_roundtrip_preserves_notes_and_cc() {
        let notes = vec![
            MidiNote::new(60, 100, 0.0, 1.0),
            MidiNote::new(64, 110, 1.0, 0.5),
            MidiNote::new(67, 95, 2.0, 1.5),
        ];
        let ccs = vec![MidiCcPoint {
            beat: 0.5,
            target: MidiCcTarget::ModWheel,
            value: 0.75,
        }];
        let path = std::env::temp_dir().join("ferric_smf_roundtrip.mid");
        export_smf_clip(&path, 120.0, &notes, &ccs).expect("export smf");
        let imported = import_smf_file(&path).expect("import smf");
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].notes.len(), 3);
        assert_eq!(imported[0].notes[0].pitch, 60);
        assert!(!imported[0].cc_points.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn scale_snap_and_bezier_automation_work() {
        // C Major should snap C#4 (61) -> C4 (60) or D4 (62)
        let snapped = MusicalScale::Major.snap_pitch(0, 61);
        assert!(snapped == 60 || snapped == 62);
        assert!(MusicalScale::Major.contains_pitch(0, snapped));

        let mut lane = AutomationLane::new(AutomationTarget::Volume);
        lane.points.push(AutomationPoint::new(0.0, 0.0, 0.6));
        lane.points.push(AutomationPoint::new(4.0, 1.0, 0.0));
        let mid = lane.eval_at(2.0).unwrap();
        // Positive tension = slow exponential start, so at midpoint value < 0.5
        assert!(mid > 0.0 && mid < 0.45);
    }

    #[test]
    fn conductor_linear_tempo_ramp_integral_meter_map_and_auto_crossfade_work() {
        let mut proj = Project::new_empty("Conductor Test");
        proj.upsert_tempo_point(0.0, 60.0, true);
        proj.upsert_tempo_point(4.0, 120.0, false);

        // Analytical integral of 60 / (60 + 15*b) over [0..4] = 4 * ln(2) = 2.7725887222s!
        let secs = proj.beats_to_seconds(4.0);
        let exact = 4.0 * 2.0_f64.ln();
        assert!((secs - exact).abs() < 1e-6, "got {} vs {}", secs, exact);

        // Inverse `seconds_to_beats` recovers beat 4.0 within 1e-6!
        let inv_beat = proj.seconds_to_beats(secs);
        assert!((inv_beat - 4.0).abs() < 1e-5);

        // Mid-song Time Signature change: 2 bars of 4/4 (beats 0..8) -> switch to 3/4 at beat 8!
        proj.add_meter_change(8, 3, 4);
        assert_eq!(proj.format_bar_beat_tick(8.0), "003.1.000");
        assert_eq!(proj.format_bar_beat_tick(11.0), "004.1.000"); // 8 + 3 = Bar 4 Beat 1!

        // Equal-Power Auto-Crossfade across two overlapping audio clips (`[0..6]` and `[4..10]`)
        let tid = proj.add_track("Audio Crossfade");
        let buf = AudioBuffer::from_stereo(48_000, vec![0.5; 4800], vec![0.5; 4800], None);
        {
            let trk = proj.find_track_mut(tid).unwrap();
            trk.clips.push(TimelineClip::new_audio("Clip A", 0.0, 6.0, buf.clone()));
            trk.clips.push(TimelineClip::new_audio("Clip B", 4.0, 6.0, buf));
            // At midpoint of overlap (beat 5.0), g_a^2 + g_b^2 == 1.0 (equal power!)
            let ga = trk.auto_crossfade_gain(0, 5.0);
            let gb = trk.auto_crossfade_gain(1, 5.0);
            assert!(((ga * ga + gb * gb) - 1.0).abs() < 1e-5);
            assert!((ga - gb).abs() < 1e-5);
        }

        // Verify Glue / Consolidate (`Ctrl+G`) merges overlapping audio clips into one consolidated 10-beat clip!
        let ids: Vec<ClipId> = proj
            .find_track(tid)
            .unwrap()
            .clips
            .iter()
            .map(|c| c.id)
            .collect();
        let glued_id = proj.glue_clips(tid, &ids, 48_000).expect("glue clips");
        let trk = proj.find_track(tid).unwrap();
        assert_eq!(trk.clips.len(), 1);
        assert_eq!(trk.clips[0].id, glued_id);
        assert!((trk.clips[0].length_beats - 10.0).abs() < 1e-5);
    }
}
