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
    build_demo_project, synthesize_browser_sample, HistoryManager, MasterBus, MidiLearnMapping,
    ModelError, Project, Scene, SnapGrid, TimeSignature, TimelineMarker,
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
}
