pub mod clip;
pub mod fx;
pub mod project;
pub mod track;

pub use clip::{
    AudioBuffer, ClipId, CompSegment, FadeShape, MidiNote, SessionSlot, Take, TakeContent, TakeId,
    TimelineClip,
};
pub use fx::{
    FxId, FxKind, FxSlot, LfoShape, ModRouting, ModTarget, ModulatorId, ModulatorKind,
    ModulatorSlot, ParamSpec,
};
pub use project::{
    build_demo_project, HistoryManager, MasterBus, ModelError, Project, Scene, SnapGrid,
    TimeSignature, TimelineMarker,
};
pub use track::{
    AutomationLane, AutomationPoint, AutomationTarget, InputSource, SendTap, Track, TrackId,
    TrackSend, STUDIO_TRACK_COLORS,
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

        // Overwrite middle of Take 2 with Take 3
        clip.set_comp_region(6.0, 8.0, 2);
        assert_eq!(clip.take_at_local_beat(5.0), 1);
        assert_eq!(clip.take_at_local_beat(7.0), 2);
        assert_eq!(clip.take_at_local_beat(10.0), 1);

        // Merge back to Take 1 across [4, 12]
        clip.set_comp_region(4.0, 12.0, 0);
        assert_eq!(clip.comp_segments.len(), 1);
        assert_eq!(clip.comp_segments[0].take_index, 0);
    }

    #[test]
    fn routing_dag_prevents_cycles_and_orders_buses_last() {
        let mut proj = Project::new_empty("Routing Test");
        let gtr = proj.add_track("Guitar");
        let bus = proj.add_track("Guitar Bus");
        let rev = proj.add_track("Reverb Aux");

        proj.find_track_mut(gtr).unwrap().parent_folder_id = Some(bus);
        proj.add_send(bus, rev, -6.0).unwrap();

        // Cycle: Reverb -> Guitar should be rejected!
        assert!(proj.add_send(rev, gtr, 0.0).is_err());

        let order = proj.topological_track_order();
        let pos_gtr = order.iter().position(|&i| proj.tracks[i].id == gtr).unwrap();
        let pos_bus = order.iter().position(|&i| proj.tracks[i].id == bus).unwrap();
        let pos_rev = order.iter().position(|&i| proj.tracks[i].id == rev).unwrap();
        assert!(pos_gtr < pos_bus);
        assert!(pos_bus < pos_rev);
    }

    #[test]
    fn split_clip_preserves_slip_offset_and_comp_lanes() {
        let mut proj = build_demo_project(48_000);
        let gtr_id = proj.tracks[3].id;
        let clip_id = proj.tracks[3].clips[0].id;
        let right_id = proj.split_clip(gtr_id, clip_id, 8.0).expect("split ok");
        let gtr = proj.find_track(gtr_id).unwrap();
        assert_eq!(gtr.clips.len(), 2);
        let right = gtr.find_clip(right_id).unwrap();
        assert_eq!(right.start_beat, 8.0);
        assert_eq!(right.length_beats, 8.0);
        assert_eq!(right.slip_offset_beats, 8.0);
    }
}
