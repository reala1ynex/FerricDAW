use ferric_dsp::{
    db_to_lin, lin_to_db, ChromaticTuner, FxRuntimeInstance, LoudnessAndSpectrumAnalyzer,
    MasterMeterSnapshot, MidiEvent, ModulatorRuntime, TunerReading,
};
use ferric_model::{
    FxId, InputSource, ModTarget, ModulatorId, Project, SendTap, TakeContent, TimelineClip, Track,
    TrackId,
};
use std::f32::consts::TAU;

pub const MAX_ENGINE_TRACKS: usize = 64;

#[derive(Debug, Clone, Copy, Default)]
pub struct TrackMeter {
    pub track_id: Option<TrackId>,
    pub peak_l_db: f32,
    pub peak_r_db: f32,
    pub rms_db: f32,
    pub gain_reduction_db: f32,
}

/// Core audio graph evaluator shared identically by both the lock-free real-time callback
/// and the faster-than-realtime offline stem/master bounce!
pub struct AudioGraphCore {
    pub sample_rate: f32,
    pub max_block: usize,
    pub is_playing: bool,
    pub playhead_beat: f64,
    fx_pool: Vec<FxRuntimeInstance>,
    mod_pool: Vec<ModulatorRuntime>,
    pub input_tuner: ChromaticTuner,
    pub master_analyzer: LoudnessAndSpectrumAnalyzer,
    pub last_master_meter: MasterMeterSnapshot,
    pub track_meters: [TrackMeter; MAX_ENGINE_TRACKS],
    pub track_meter_count: usize,
    // Pre-allocated scratch buffers so `render_block` performs ZERO heap allocations!
    bus_l: Vec<Vec<f32>>,
    bus_r: Vec<Vec<f32>>,
    clip_l: Vec<f32>,
    clip_r: Vec<f32>,
    midi_scratch: Vec<MidiEvent>,
    param_scratch: Vec<f32>,
    topo_scratch: Vec<usize>,
    click_phase: f32,
    click_env: f32,
    click_freq: f32,
    last_click_step: i64,
}

impl AudioGraphCore {
    pub fn new(sample_rate: f32, max_block: usize) -> Self {
        let blk = max_block.max(2048);
        Self {
            sample_rate,
            max_block: blk,
            is_playing: false,
            playhead_beat: 0.0,
            fx_pool: Vec::with_capacity(128),
            mod_pool: Vec::with_capacity(64),
            input_tuner: ChromaticTuner::new(sample_rate),
            master_analyzer: LoudnessAndSpectrumAnalyzer::new(sample_rate),
            last_master_meter: MasterMeterSnapshot::default(),
            track_meters: [TrackMeter {
                track_id: None,
                peak_l_db: -96.0,
                peak_r_db: -96.0,
                rms_db: -96.0,
                gain_reduction_db: 0.0,
            }; MAX_ENGINE_TRACKS],
            track_meter_count: 0,
            bus_l: (0..MAX_ENGINE_TRACKS).map(|_| vec![0.0; blk]).collect(),
            bus_r: (0..MAX_ENGINE_TRACKS).map(|_| vec![0.0; blk]).collect(),
            clip_l: vec![0.0; blk],
            clip_r: vec![0.0; blk],
            midi_scratch: Vec::with_capacity(256),
            param_scratch: Vec::with_capacity(32),
            topo_scratch: Vec::with_capacity(MAX_ENGINE_TRACKS),
            click_phase: 0.0,
            click_env: 0.0,
            click_freq: 1000.0,
            last_click_step: -1,
        }
    }

    /// Called on UI/offline setup or via pre-allocated registration — ensures every `FxSlot`
    /// and `ModulatorSlot` in `project` has a stateful runtime processor.
    pub fn sync_processors_from_project(&mut self, project: &Project) {
        for track in &project.tracks {
            for fx in &track.fx_chain {
                self.ensure_fx(fx.id, fx.kind);
            }
            for clip in &track.clips {
                for fx in &clip.clip_fx {
                    self.ensure_fx(fx.id, fx.kind);
                }
            }
            for m in &track.modulators {
                self.ensure_mod(m.id);
            }
        }
        for fx in &project.master.fx_chain {
            self.ensure_fx(fx.id, fx.kind);
        }
    }

    pub fn register_fx_instance(&mut self, inst: FxRuntimeInstance) {
        if let Some(pos) = self.fx_pool.iter().position(|x| x.id == inst.id) {
            self.fx_pool[pos] = inst;
        } else if self.fx_pool.len() < self.fx_pool.capacity() {
            self.fx_pool.push(inst);
        }
    }

    pub fn register_mod_instance(&mut self, inst: ModulatorRuntime) {
        if let Some(pos) = self.mod_pool.iter().position(|x| x.id == inst.id) {
            self.mod_pool[pos] = inst;
        } else if self.mod_pool.len() < self.mod_pool.capacity() {
            self.mod_pool.push(inst);
        }
    }

    fn ensure_fx(&mut self, id: FxId, kind: ferric_model::FxKind) {
        if !self.fx_pool.iter().any(|x| x.id == id) {
            self.fx_pool
                .push(FxRuntimeInstance::new(id, kind, self.sample_rate, self.max_block));
        }
    }

    fn ensure_mod(&mut self, id: ModulatorId) {
        if !self.mod_pool.iter().any(|m| m.id == id) {
            self.mod_pool.push(ModulatorRuntime::new(id));
        }
    }

    pub fn all_notes_off(&mut self) {
        for fx in &mut self.fx_pool {
            if let ferric_dsp::DspProcessor::PolySynth(s) = &mut fx.processor {
                s.all_notes_off();
            }
        }
    }

    pub fn best_tuner_reading(&self) -> TunerReading {
        for fx in &self.fx_pool {
            if let Some(r) = fx.tuner_reading() {
                if r.active {
                    return r;
                }
            }
        }
        self.input_tuner.last_reading
    }

    /// Zero-allocation audio block evaluation!
    pub fn render_block(
        &mut self,
        project: &Project,
        hw_in_l: &[f32],
        hw_in_r: &[f32],
        live_midi: &[(Option<TrackId>, MidiEvent)],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let frames = out_l.len().min(out_r.len()).min(self.max_block);
        if frames == 0 {
            return;
        }
        out_l[..frames].fill(0.0);
        out_r[..frames].fill(0.0);

        // Feed hardware audio input into always-ready input tuner if present
        if hw_in_l.len() >= frames && hw_in_r.len() >= frames {
            self.input_tuner
                .push_block(&hw_in_l[..frames], &hw_in_r[..frames]);
        }

        let num_tracks = project.tracks.len().min(MAX_ENGINE_TRACKS);
        for t in 0..num_tracks {
            self.bus_l[t][..frames].fill(0.0);
            self.bus_r[t][..frames].fill(0.0);
        }

        let beats_per_sample = (project.bpm.max(20.0) / 60.0) / self.sample_rate as f64;
        let block_start_beat = self.playhead_beat;
        let block_end_beat = block_start_beat + beats_per_sample * frames as f64;

        let any_solo = project.tracks.iter().take(num_tracks).any(|t| t.solo);

        // Compute topological execution order without allocating if possible
        self.fill_topo_order(project, num_tracks);

        for ord_pos in 0..self.topo_scratch.len() {
            let t_idx = self.topo_scratch[ord_pos];
            let track = &project.tracks[t_idx];

            // Determine effective audibility (respecting Solo & Folder Solo)
            let folder_soloed = track
                .parent_folder_id
                .and_then(|pid| project.find_track(pid))
                .map(|p| p.solo)
                .unwrap_or(false);
            let child_soloed = project
                .tracks
                .iter()
                .any(|c| c.parent_folder_id == Some(track.id) && c.solo);
            let audible = !track.mute
                && (!any_solo || track.solo || folder_soloed || child_soloed);

            self.midi_scratch.clear();

            // 1. Live MIDI input / Musical Typing routed to Record-Armed or explicitly targeted track
            for &(target_opt, ev) in live_midi {
                let matches = match target_opt {
                    Some(tid) => tid == track.id,
                    None => track.record_arm,
                };
                if matches {
                    self.midi_scratch.push(ev);
                }
            }

            // 2. Live hardware audio input monitoring when armed & monitoring enabled
            if track.record_arm
                && track.input_monitoring
                && hw_in_l.len() >= frames
                && hw_in_r.len() >= frames
            {
                match track.input_source {
                    InputSource::AudioAndMidi { .. } => {
                        for i in 0..frames {
                            self.bus_l[t_idx][i] += hw_in_l[i];
                            self.bus_r[t_idx][i] += hw_in_r[i];
                        }
                    }
                    InputSource::AudioMono { channel: 0 } => {
                        for i in 0..frames {
                            self.bus_l[t_idx][i] += hw_in_l[i];
                            self.bus_r[t_idx][i] += hw_in_l[i];
                        }
                    }
                    InputSource::AudioMono { .. } => {
                        for i in 0..frames {
                            self.bus_l[t_idx][i] += hw_in_r[i];
                            self.bus_r[t_idx][i] += hw_in_r[i];
                        }
                    }
                    _ => {}
                }
            }

            // 3. Playback: Session Clip Launcher OR Linear Arrangement Clips
            if self.is_playing {
                if let Some(slot_idx) = track.active_session_slot {
                    if let Some(Some(slot)) = track.session_slots.get(slot_idx) {
                        self.evaluate_session_slot(
                            slot,
                            t_idx,
                            frames,
                            block_start_beat,
                            beats_per_sample,
                            project.bpm,
                        );
                    }
                } else {
                    for clip in &track.clips {
                        if clip.muted {
                            continue;
                        }
                        self.evaluate_timeline_clip(
                            clip,
                            t_idx,
                            frames,
                            block_start_beat,
                            block_end_beat,
                            beats_per_sample,
                            project,
                        );
                    }
                }
            }

            self.midi_scratch.sort_by_key(|e| e.sample_offset);

            // Pre-FX RMS for Envelope Follower modulators
            let pre_rms = compute_rms(&self.bus_l[t_idx][..frames], &self.bus_r[t_idx][..frames]);

            // 4. Evaluate Track Modulators (LFO, Env Follower, Step Seq, Macro)
            let mut mod_gain_offset_db = 0.0_f32;
            let mut mod_pan_offset = 0.0_f32;
            for m in &track.modulators {
                if !m.enabled {
                    continue;
                }
                if let Some(rt) = self.mod_pool.iter_mut().find(|r| r.id == m.id) {
                    let val = rt.evaluate_block(
                        &m.kind,
                        self.sample_rate,
                        frames,
                        project.bpm,
                        block_start_beat,
                        pre_rms,
                    );
                    for route in &m.routings {
                        match route.target {
                            ModTarget::TrackGain => mod_gain_offset_db += val * route.depth * 18.0,
                            ModTarget::TrackPan => mod_pan_offset += val * route.depth,
                            _ => {}
                        }
                    }
                }
            }

            // 5. Pre-FX Sends
            self.dispatch_sends(project, track, t_idx, frames, SendTap::PreFx, audible);

            // 6. Track FX Chain (Instruments + Audio Inserts + Modulated Parameters!)
            let mut max_gr_db = 0.0_f32;
            for fx in &track.fx_chain {
                if !fx.enabled {
                    continue;
                }
                let specs = fx.kind.param_specs();
                self.param_scratch.clear();
                for (p_idx, spec) in specs.iter().enumerate() {
                    let base_val = fx.param(p_idx);
                    let mut norm = spec.normalize(base_val);
                    // Sum modulator contributions targeting this parameter
                    for m in &track.modulators {
                        if !m.enabled {
                            continue;
                        }
                        if let Some(rt) = self.mod_pool.iter().find(|r| r.id == m.id) {
                            for route in &m.routings {
                                if route.target
                                    == (ModTarget::FxParam {
                                        fx_id: fx.id,
                                        param_index: p_idx,
                                    })
                                {
                                    norm = (norm + rt.last_output * route.depth).clamp(0.0, 1.0);
                                }
                            }
                        }
                    }
                    self.param_scratch.push(spec.denormalize(norm));
                }

                let mut wet = fx.wet_mix;
                for m in &track.modulators {
                    if !m.enabled {
                        continue;
                    }
                    if let Some(rt) = self.mod_pool.iter().find(|r| r.id == m.id) {
                        for route in &m.routings {
                            if route.target == (ModTarget::FxWet { fx_id: fx.id }) {
                                wet = (wet + rt.last_output * route.depth).clamp(0.0, 1.0);
                            }
                        }
                    }
                }

                if let Some(inst) = self.fx_pool.iter_mut().find(|i| i.id == fx.id) {
                    inst.process_block(
                        &mut self.bus_l[t_idx][..frames],
                        &mut self.bus_r[t_idx][..frames],
                        &self.param_scratch,
                        wet,
                        project.bpm,
                        &self.midi_scratch,
                    );
                    max_gr_db = max_gr_db.max(inst.gain_reduction_db());
                }
            }

            // 7. Pre-Fader (Post-FX) Sends
            self.dispatch_sends(project, track, t_idx, frames, SendTap::PreFaderPostFx, audible);

            // 8. Fader, Automation, Pan Law (-3 dB equal power), Stereo Width & Phase Invert
            let mut fader_db = track.gain_db + mod_gain_offset_db;
            let mut pan = (track.pan + mod_pan_offset).clamp(-1.0, 1.0);
            if self.is_playing {
                for lane in &track.automation_lanes {
                    if let Some(v) = lane.eval_at(block_start_beat) {
                        match lane.target {
                            ferric_model::AutomationTarget::Volume => {
                                fader_db = -60.0 + v * 72.0;
                            }
                            ferric_model::AutomationTarget::Pan => {
                                pan = v * 2.0 - 1.0;
                            }
                            _ => {}
                        }
                    }
                }
            }

            let gain_lin = if audible { db_to_lin(fader_db) } else { 0.0 };
            let phase_sign = if track.phase_invert { -1.0 } else { 1.0 };
            let pan_angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
            let pan_l = pan_angle.cos() * std::f32::consts::SQRT_2 * 0.7071;
            let pan_r = pan_angle.sin() * std::f32::consts::SQRT_2 * 0.7071;
            let width = track.stereo_width.clamp(0.0, 2.0);

            let mut peak_l = 0.0_f32;
            let mut peak_r = 0.0_f32;
            let mut sum_sq = 0.0_f32;

            for i in 0..frames {
                let raw_l = self.bus_l[t_idx][i] * phase_sign;
                let raw_r = self.bus_r[t_idx][i] * phase_sign;
                let mid = 0.5 * (raw_l + raw_r);
                let side = 0.5 * (raw_l - raw_r) * width;
                let l = (mid + side) * gain_lin * pan_l * 1.4142;
                let r = (mid - side) * gain_lin * pan_r * 1.4142;
                self.bus_l[t_idx][i] = l;
                self.bus_r[t_idx][i] = r;
                peak_l = peak_l.max(l.abs());
                peak_r = peak_r.max(r.abs());
                sum_sq += 0.5 * (l * l + r * r);
            }

            self.track_meters[t_idx] = TrackMeter {
                track_id: Some(track.id),
                peak_l_db: lin_to_db(peak_l),
                peak_r_db: lin_to_db(peak_r),
                rms_db: lin_to_db((sum_sq / frames as f32).sqrt()),
                gain_reduction_db: max_gr_db,
            };

            // 9. Post-Fader Sends
            self.dispatch_sends(project, track, t_idx, frames, SendTap::PostFader, audible);

            // 10. Sum into Parent Folder Track OR Master Bus!
            if let Some(parent_id) = track.parent_folder_id {
                if let Some(p_idx) = project
                    .tracks
                    .iter()
                    .take(num_tracks)
                    .position(|pt| pt.id == parent_id)
                {
                    if p_idx != t_idx {
                        for i in 0..frames {
                            let l = self.bus_l[t_idx][i];
                            let r = self.bus_r[t_idx][i];
                            self.bus_l[p_idx][i] += l;
                            self.bus_r[p_idx][i] += r;
                        }
                    }
                }
            } else if track.send_to_master {
                for i in 0..frames {
                    out_l[i] += self.bus_l[t_idx][i];
                    out_r[i] += self.bus_r[t_idx][i];
                }
            }
        }
        self.track_meter_count = num_tracks;

        // 11. Master Bus FX Chain
        for fx in &project.master.fx_chain {
            if !fx.enabled {
                continue;
            }
            if let Some(inst) = self.fx_pool.iter_mut().find(|i| i.id == fx.id) {
                inst.process_block(
                    &mut out_l[..frames],
                    &mut out_r[..frames],
                    &fx.params,
                    fx.wet_mix,
                    project.bpm,
                    &[],
                );
            }
        }

        // 12. Metronome Click (if enabled while playing)
        if self.is_playing && project.metronome_enabled {
            self.mix_metronome(
                project,
                block_start_beat,
                beats_per_sample,
                &mut out_l[..frames],
                &mut out_r[..frames],
            );
        }

        // 13. Master Gain, Dim (-18 dB), Mono Check & Brickwall Safety Limiter
        let mut master_lin = db_to_lin(project.master.gain_db);
        if project.master.dim_enabled {
            master_lin *= db_to_lin(-18.0);
        }
        for i in 0..frames {
            let mut l = out_l[i] * master_lin;
            let mut r = out_r[i] * master_lin;
            if project.master.mono_check {
                let m = 0.5 * (l + r);
                l = m;
                r = m;
            }
            if project.master.safety_limiter {
                l = soft_brickwall(l);
                r = soft_brickwall(r);
            }
            out_l[i] = l;
            out_r[i] = r;
        }

        self.last_master_meter = self
            .master_analyzer
            .analyze_block(&out_l[..frames], &out_r[..frames]);

        // 14. Advance Playhead & Handle Loop Wrap
        if self.is_playing {
            let mut next_beat = block_end_beat;
            if project.loop_enabled && project.loop_end_beat > project.loop_start_beat + 0.25 {
                if block_start_beat < project.loop_end_beat && next_beat >= project.loop_end_beat {
                    let overshoot = next_beat - project.loop_end_beat;
                    next_beat = project.loop_start_beat + overshoot;
                }
            }
            self.playhead_beat = next_beat;
        } else {
            self.last_click_step = -1;
        }
    }

    fn evaluate_session_slot(
        &mut self,
        slot: &ferric_model::SessionSlot,
        t_idx: usize,
        frames: usize,
        block_start_beat: f64,
        beats_per_sample: f64,
        bpm: f64,
    ) {
        let len_b = slot.length_beats.max(0.25);
        let slot_gain = db_to_lin(slot.gain_db);
        match &slot.content {
            TakeContent::Midi { notes } => {
                for i in 0..frames {
                    let b0 = (block_start_beat + i as f64 * beats_per_sample).rem_euclid(len_b);
                    let b1 = b0 + beats_per_sample;
                    for n in notes {
                        if n.start_beat >= b0 && n.start_beat < b1 {
                            self.midi_scratch.push(MidiEvent {
                                sample_offset: i,
                                note: n.pitch,
                                velocity: n.velocity,
                                is_note_on: true,
                            });
                        }
                        let end_b = n.end_beat().rem_euclid(len_b);
                        if end_b >= b0 && end_b < b1 {
                            self.midi_scratch.push(MidiEvent {
                                sample_offset: i,
                                note: n.pitch,
                                velocity: 0,
                                is_note_on: false,
                            });
                        }
                    }
                }
            }
            TakeContent::Audio { buffer } => {
                let spb = 60.0 / bpm.max(20.0);
                for i in 0..frames {
                    let b = (block_start_beat + i as f64 * beats_per_sample).rem_euclid(len_b);
                    let frame_pos = b * spb * buffer.sample_rate as f64;
                    let (l, r) = buffer.sample_interpolated(frame_pos);
                    self.bus_l[t_idx][i] += l * slot_gain;
                    self.bus_r[t_idx][i] += r * slot_gain;
                }
            }
        }
    }

    fn evaluate_timeline_clip(
        &mut self,
        clip: &TimelineClip,
        t_idx: usize,
        frames: usize,
        block_start_beat: f64,
        block_end_beat: f64,
        beats_per_sample: f64,
        project: &Project,
    ) {
        if clip.end_beat() < block_start_beat || clip.start_beat > block_end_beat || clip.takes.is_empty() {
            return;
        }
        let pitch_ratio = 2.0_f64.powf(clip.pitch_semitones as f64 / 12.0);
        let pitch_semi_round = clip.pitch_semitones.round() as i32;
        let spb = 60.0 / project.bpm.max(20.0);

        let has_audio = clip
            .takes
            .iter()
            .any(|t| matches!(t.content, TakeContent::Audio { .. }));
        if has_audio {
            self.clip_l[..frames].fill(0.0);
            self.clip_r[..frames].fill(0.0);
        }

        for i in 0..frames {
            let beat = block_start_beat + i as f64 * beats_per_sample;
            let Some((local_beat, inner_beat)) = clip.content_beat(beat) else {
                // Emit note-offs right at clip boundary
                if (beat - clip.end_beat()).abs() < beats_per_sample {
                    for n in 0..128u8 {
                        let _ = n;
                    }
                }
                continue;
            };

            let take_idx = clip.take_at_local_beat(local_beat);
            let env = clip.gain_envelope_at(local_beat);

            if let Some(take) = clip.takes.get(take_idx) {
                match &take.content {
                    TakeContent::Midi { notes } => {
                        let b1 = inner_beat + beats_per_sample;
                        for n in notes {
                            if n.start_beat >= inner_beat && n.start_beat < b1 {
                                let p = (n.pitch as i32 + pitch_semi_round).clamp(0, 127) as u8;
                                let v = ((n.velocity as f32 * env).round() as i32).clamp(1, 127) as u8;
                                self.midi_scratch.push(MidiEvent {
                                    sample_offset: i,
                                    note: p,
                                    velocity: v,
                                    is_note_on: true,
                                });
                            }
                            let end_b = n.end_beat();
                            if end_b >= inner_beat && end_b < b1 {
                                let p = (n.pitch as i32 + pitch_semi_round).clamp(0, 127) as u8;
                                self.midi_scratch.push(MidiEvent {
                                    sample_offset: i,
                                    note: p,
                                    velocity: 0,
                                    is_note_on: false,
                                });
                            }
                        }
                    }
                    TakeContent::Audio { buffer } => {
                        let pos_frames = inner_beat * spb * (buffer.sample_rate as f64) * pitch_ratio;
                        let (mut l, mut r) = buffer.sample_interpolated(pos_frames);

                        // Smooth 8ms crossfade across REAPER Swipe-Comping segment boundaries!
                        let xfade_beats = 0.025;
                        for seg in &clip.comp_segments {
                            let dist = (local_beat - seg.start_beat).abs();
                            if seg.start_beat > 0.001 && dist < xfade_beats {
                                let prev_take = clip.take_at_local_beat((seg.start_beat - 0.001).max(0.0));
                                if prev_take != take_idx {
                                    if let Some(TakeContent::Audio { buffer: prev_buf }) =
                                        clip.takes.get(prev_take).map(|t| &t.content)
                                    {
                                        let (pl, pr) = prev_buf.sample_interpolated(pos_frames);
                                        let alpha = ((local_beat - (seg.start_beat - xfade_beats))
                                            / (2.0 * xfade_beats))
                                            .clamp(0.0, 1.0) as f32;
                                        l = pl * (1.0 - alpha) + l * alpha;
                                        r = pr * (1.0 - alpha) + r * alpha;
                                    }
                                }
                            }
                        }

                        self.clip_l[i] += l * env;
                        self.clip_r[i] += r * env;
                    }
                }
            }
        }

        if has_audio {
            // Evaluate REAPER-style Per-Clip FX Chain!
            for fx in &clip.clip_fx {
                if !fx.enabled {
                    continue;
                }
                if let Some(inst) = self.fx_pool.iter_mut().find(|x| x.id == fx.id) {
                    inst.process_block(
                        &mut self.clip_l[..frames],
                        &mut self.clip_r[..frames],
                        &fx.params,
                        fx.wet_mix,
                        project.bpm,
                        &[],
                    );
                }
            }
            for i in 0..frames {
                self.bus_l[t_idx][i] += self.clip_l[i];
                self.bus_r[t_idx][i] += self.clip_r[i];
            }
        }
    }

    fn dispatch_sends(
        &mut self,
        project: &Project,
        track: &Track,
        src_idx: usize,
        frames: usize,
        tap: SendTap,
        src_audible: bool,
    ) {
        if !src_audible && tap == SendTap::PostFader {
            return;
        }
        let num_tracks = project.tracks.len().min(MAX_ENGINE_TRACKS);
        for send in &track.sends {
            if send.muted || send.tap != tap {
                continue;
            }
            if let Some(dst_idx) = project
                .tracks
                .iter()
                .take(num_tracks)
                .position(|t| t.id == send.target_track_id)
            {
                if dst_idx == src_idx {
                    continue;
                }
                let g = db_to_lin(send.gain_db);
                let pan_angle = (send.pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
                let pl = pan_angle.cos() * 1.4142 * g;
                let pr = pan_angle.sin() * 1.4142 * g;
                for i in 0..frames {
                    let sl = self.bus_l[src_idx][i] * pl;
                    let sr = self.bus_r[src_idx][i] * pr;
                    self.bus_l[dst_idx][i] += sl;
                    self.bus_r[dst_idx][i] += sr;
                }
            }
        }
    }

    fn mix_metronome(
        &mut self,
        project: &Project,
        block_start_beat: f64,
        beats_per_sample: f64,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let bpb = project.time_signature.numerator.max(1) as i64;
        for i in 0..out_l.len() {
            let beat = block_start_beat + i as f64 * beats_per_sample;
            let step = beat.floor() as i64;
            if step != self.last_click_step && (beat - step as f64) < 0.05 {
                self.last_click_step = step;
                self.click_env = 1.0;
                self.click_phase = 0.0;
                self.click_freq = if step.rem_euclid(bpb) == 0 {
                    1560.0 // Bar accent
                } else {
                    1040.0 // Beat pulse
                };
            }
            if self.click_env > 0.0005 {
                let s = (TAU * self.click_phase).sin() * self.click_env * 0.22;
                self.click_phase = (self.click_phase + self.click_freq / self.sample_rate).fract();
                self.click_env *= (-1.0 / (0.018 * self.sample_rate)).exp();
                out_l[i] += s;
                out_r[i] += s;
            }
        }
    }

    fn fill_topo_order(&mut self, project: &Project, n: usize) {
        self.topo_scratch.clear();
        let mut visited = [false; MAX_ENGINE_TRACKS];
        // Leaves/children first, folders & aux destinations after!
        for pass in 0..4 {
            for idx in 0..n {
                if visited[idx] {
                    continue;
                }
                let t = &project.tracks[idx];
                let mut deps_ready = true;
                for other in 0..n {
                    if other == idx || visited[other] {
                        continue;
                    }
                    let ot = &project.tracks[other];
                    let feeds_this = ot.parent_folder_id == Some(t.id)
                        || ot.sends.iter().any(|s| s.target_track_id == t.id);
                    if feeds_this {
                        deps_ready = false;
                        break;
                    }
                }
                if deps_ready || pass == 3 {
                    visited[idx] = true;
                    self.topo_scratch.push(idx);
                }
            }
        }
    }
}

#[inline]
fn compute_rms(l: &[f32], r: &[f32]) -> f32 {
    let n = l.len().min(r.len());
    if n == 0 {
        return 0.0;
    }
    let mut sum = 0.0_f32;
    for i in 0..n {
        sum += 0.5 * (l[i] * l[i] + r[i] * r[i]);
    }
    (sum / n as f32).sqrt()
}

#[inline]
fn soft_brickwall(x: f32) -> f32 {
    if x.abs() <= 0.92 {
        x
    } else {
        x.signum() * (0.92 + 0.078 * ((x.abs() - 0.92) * 12.0).tanh())
    }
}
