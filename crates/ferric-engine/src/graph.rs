use ferric_dsp::{
    db_to_lin, lin_to_db, sample_wsola_decoupled, ArpRuntime, ChromaticTuner, FxRuntimeInstance,
    LoudnessAndSpectrumAnalyzer, MasterMeterSnapshot, MidiEvent, MidiExpression, ModulatorRuntime,
    OnsetBpmDetector, TunerReading, VocalPitchTelemetry,
};
use ferric_model::{
    eval_midi_cc_curve, AutomationTarget, MidiCcTarget, ModTarget, ModulatorId,
    Project, SendTap, TakeContent, TimeStretchMode, TimelineClip, Track, TrackId,
};
use std::f32::consts::TAU;

pub const MAX_ENGINE_TRACKS: usize = 64;
const PDC_RING_LEN: usize = 8192; // Power of 2 for zero-alloc Plugin Delay Compensation!

#[derive(Debug, Clone, Copy, Default)]
pub struct TrackMeter {
    pub track_id: Option<TrackId>,
    pub peak_l_db: f32,
    pub peak_r_db: f32,
    pub rms_db: f32,
    pub gain_reduction_db: f32,
    pub pdc_samples: u16,
}

#[derive(Clone)]
struct PdcDelayLine {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_pos: usize,
}

impl PdcDelayLine {
    fn new() -> Self {
        Self {
            buf_l: vec![0.0; PDC_RING_LEN],
            buf_r: vec![0.0; PDC_RING_LEN],
            write_pos: 0,
        }
    }

    #[inline]
    fn delay_block(&mut self, left: &mut [f32], right: &mut [f32], delay_samples: usize) {
        if delay_samples == 0 {
            return;
        }
        let d = delay_samples.min(PDC_RING_LEN - 2);
        let mask = PDC_RING_LEN - 1;
        let frames = left.len().min(right.len());
        for i in 0..frames {
            self.buf_l[self.write_pos] = left[i];
            self.buf_r[self.write_pos] = right[i];
            let read_idx = (self.write_pos + PDC_RING_LEN - d) & mask;
            left[i] = self.buf_l[read_idx];
            right[i] = self.buf_r[read_idx];
            self.write_pos = (self.write_pos + 1) & mask;
        }
    }
}

/// Pre-reserved pool sizes so registering instances on the audio thread does
/// not reallocate in practice.
pub const FX_POOL_CAPACITY: usize = 1024;
pub const MOD_POOL_CAPACITY: usize = 256;

pub struct AudioGraphCore {
    pub sample_rate: f32,
    pub max_block: usize,
    pub is_playing: bool,
    pub playhead_beat: f64,
    pub count_in_beats_remaining: f64,
    pub input_gain_lin: f32,
    pub sustain_pedal_down: bool,
    pub live_pitch_bend_semi: f32,
    pub live_mod_wheel: f32,
    pub live_expression: f32,
    pub live_brightness: f32,
    clip_midi_expr: MidiExpression,
    held_sustain_notes: [bool; 128],
    /// Boxed so replaced instances can be handed back to a non-RT thread
    /// for dropping instead of being freed inside the audio callback (audit H6).
    fx_pool: Vec<Box<FxRuntimeInstance>>,
    #[allow(clippy::vec_box)] // see fx_pool: boxes travel through the RT garbage ring
    mod_pool: Vec<Box<ModulatorRuntime>>,
    arp_lanes: Vec<ArpRuntime>,
    pub input_tuner: ChromaticTuner,
    pub input_bpm_detector: OnsetBpmDetector,
    pub master_analyzer: LoudnessAndSpectrumAnalyzer,
    pub last_master_meter: MasterMeterSnapshot,
    pub last_vocal_pitch: VocalPitchTelemetry,
    pub total_pdc_samples: usize,
    pub track_meters: [TrackMeter; MAX_ENGINE_TRACKS],
    pub track_meter_count: usize,
    bus_l: Vec<Vec<f32>>,
    bus_r: Vec<Vec<f32>>,
    /// Sidechain post-FX tap buffers so muted/unsoloed kick tracks can STILL trigger sidechains!
    sc_l: Vec<Vec<f32>>,
    sc_r: Vec<Vec<f32>>,
    pdc_lines: Vec<PdcDelayLine>,
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
            count_in_beats_remaining: 0.0,
            input_gain_lin: 1.0,
            sustain_pedal_down: false,
            live_pitch_bend_semi: 0.0,
            live_mod_wheel: 0.0,
            live_expression: 1.0,
            live_brightness: 0.0,
            clip_midi_expr: MidiExpression::default(),
            held_sustain_notes: [false; 128],
            fx_pool: Vec::with_capacity(FX_POOL_CAPACITY),
            mod_pool: Vec::with_capacity(MOD_POOL_CAPACITY),
            arp_lanes: (0..MAX_ENGINE_TRACKS).map(|_| ArpRuntime::new()).collect(),
            input_tuner: ChromaticTuner::new(sample_rate),
            input_bpm_detector: OnsetBpmDetector::new(sample_rate),
            master_analyzer: LoudnessAndSpectrumAnalyzer::new(sample_rate),
            last_master_meter: MasterMeterSnapshot::default(),
            last_vocal_pitch: VocalPitchTelemetry::default(),
            total_pdc_samples: 0,
            track_meters: [TrackMeter::default(); MAX_ENGINE_TRACKS],
            track_meter_count: 0,
            bus_l: (0..MAX_ENGINE_TRACKS).map(|_| vec![0.0; blk]).collect(),
            bus_r: (0..MAX_ENGINE_TRACKS).map(|_| vec![0.0; blk]).collect(),
            sc_l: (0..MAX_ENGINE_TRACKS).map(|_| vec![0.0; blk]).collect(),
            sc_r: (0..MAX_ENGINE_TRACKS).map(|_| vec![0.0; blk]).collect(),
            pdc_lines: (0..MAX_ENGINE_TRACKS).map(|_| PdcDelayLine::new()).collect(),
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

    pub fn sync_processors_from_project(&mut self, project: &Project) {
        for track in &project.tracks {
            for fx in &track.fx_chain {
                self.ensure_fx(fx);
            }
            for clip in &track.clips {
                for fx in &clip.clip_fx {
                    self.ensure_fx(fx);
                }
            }
            for m in &track.modulators {
                self.ensure_mod(m.id);
            }
        }
        for fx in &project.master.fx_chain {
            self.ensure_fx(fx);
        }
    }

    /// Non-real-time convenience (offline render, tests): the replaced
    /// instance, if any, is dropped here.
    pub fn register_fx_instance(&mut self, inst: FxRuntimeInstance) {
        drop(self.register_fx_boxed(Box::new(inst)));
    }

    /// Real-time safe: installs `inst` and returns the instance it replaced so
    /// the caller can drop it off the audio thread. Does not allocate unless
    /// more than `FX_POOL_CAPACITY` distinct FX were ever registered.
    pub fn register_fx_boxed(
        &mut self,
        inst: Box<FxRuntimeInstance>,
    ) -> Option<Box<FxRuntimeInstance>> {
        if let Some(pos) = self.fx_pool.iter().position(|x| x.id == inst.id) {
            Some(std::mem::replace(&mut self.fx_pool[pos], inst))
        } else {
            self.fx_pool.push(inst);
            None
        }
    }

    pub fn register_mod_instance(&mut self, inst: ModulatorRuntime) {
        drop(self.register_mod_boxed(Box::new(inst)));
    }

    /// Real-time safe counterpart of [`Self::register_mod_instance`].
    pub fn register_mod_boxed(
        &mut self,
        inst: Box<ModulatorRuntime>,
    ) -> Option<Box<ModulatorRuntime>> {
        if let Some(pos) = self.mod_pool.iter().position(|x| x.id == inst.id) {
            Some(std::mem::replace(&mut self.mod_pool[pos], inst))
        } else {
            self.mod_pool.push(inst);
            None
        }
    }

    fn ensure_fx(&mut self, fx: &ferric_model::FxSlot) {
        if let Some(existing) = self.fx_pool.iter_mut().find(|x| x.id == fx.id) {
            if matches!(
                fx.kind,
                ferric_model::FxKind::SamplePlayer
                    | ferric_model::FxKind::WavetableSynth
                    | ferric_model::FxKind::ConvolutionIr
            ) && fx.sampler_buffer.is_some()
            {
                existing.set_sampler_data(fx.sampler_buffer.clone(), fx.sampler_slices.clone());
            }
            return;
        }
        let mut inst = FxRuntimeInstance::new(fx.id, fx.kind, self.sample_rate, self.max_block);
        if fx.sampler_buffer.is_some() {
            inst.set_sampler_data(fx.sampler_buffer.clone(), fx.sampler_slices.clone());
        }
        self.fx_pool.push(Box::new(inst));
    }

    fn ensure_mod(&mut self, id: ModulatorId) {
        if !self.mod_pool.iter().any(|m| m.id == id) {
            self.mod_pool.push(Box::new(ModulatorRuntime::new(id)));
        }
    }

    pub fn set_sustain_pedal(&mut self, down: bool, out_release: &mut Vec<(Option<TrackId>, MidiEvent)>) {
        self.sustain_pedal_down = down;
        if !down {
            for (pitch, held) in self.held_sustain_notes.iter_mut().enumerate() {
                if *held {
                    *held = false;
                    out_release.push((
                        None,
                        MidiEvent {
                            sample_offset: 0,
                            note: pitch as u8,
                            velocity: 0,
                            is_note_on: false,
                        },
                    ));
                }
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        self.sustain_pedal_down = false;
        self.held_sustain_notes = [false; 128];
        for arp in &mut self.arp_lanes {
            arp.reset();
        }
        for fx in &mut self.fx_pool {
            match &mut fx.processor {
                ferric_dsp::DspProcessor::PolySynth(s) => s.all_notes_off(),
                ferric_dsp::DspProcessor::WavetableSynth(w) => w.all_notes_off(),
                ferric_dsp::DspProcessor::SamplePlayer(sp) => sp.all_notes_off(),
                ferric_dsp::DspProcessor::DrawbarOrgan(organ) => organ.all_notes_off(),
                ferric_dsp::DspProcessor::ElectricPiano(piano) => piano.all_notes_off(),
                ferric_dsp::DspProcessor::ExternalClap(ext) => ext.reset(),
                _ => {}
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

    #[inline]
    pub fn track_post_fader_slices(&self, t_idx: usize, frames: usize) -> (&[f32], &[f32]) {
        (&self.bus_l[t_idx][..frames], &self.bus_r[t_idx][..frames])
    }

    /// Zero-allocation real-time audio block evaluation with PDC, Freeze, WSOLA & Sidechains!
    ///
    /// `hw_ins` is a planar multi-channel hardware input matrix (channel 0 = In 1, …).
    /// Legacy stereo callers may pass `&[hw_l, hw_r]`.
    pub fn render_block(
        &mut self,
        project: &Project,
        hw_ins: &[&[f32]],
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

        let hw_in_l = hw_ins.first().copied().unwrap_or(&[]);
        let hw_in_r = hw_ins.get(1).copied().unwrap_or(hw_in_l);
        if hw_in_l.len() >= frames && hw_in_r.len() >= frames {
            self.input_tuner
                .push_block(&hw_in_l[..frames], &hw_in_r[..frames]);
            self.input_bpm_detector
                .push_block(&hw_in_l[..frames], &hw_in_r[..frames]);
        }

        let num_tracks = project.tracks.len().min(MAX_ENGINE_TRACKS);
        for t in 0..num_tracks {
            self.bus_l[t][..frames].fill(0.0);
            self.bus_r[t][..frames].fill(0.0);
            self.sc_l[t][..frames].fill(0.0);
            self.sc_r[t][..frames].fill(0.0);
        }

        // Compute Automatic PDC (Plugin Delay Compensation) latencies per track
        let mut track_latencies = [0usize; MAX_ENGINE_TRACKS];
        let mut max_pdc = 0usize;
        for (idx, trk) in project.tracks.iter().take(num_tracks).enumerate() {
            if !trk.frozen {
                let lat: usize = trk
                    .fx_chain
                    .iter()
                    .filter(|f| f.enabled)
                    .filter_map(|f| self.fx_pool.iter().find(|i| i.id == f.id))
                    .map(|i| i.latency_samples())
                    .sum();
                track_latencies[idx] = lat;
                if trk.parent_folder_id.is_none() {
                    max_pdc = max_pdc.max(lat);
                }
            }
        }
        self.total_pdc_samples = max_pdc;

        let current_bpm = project.bpm_at_beat(self.playhead_beat).max(20.0);
        let beats_per_sample = (current_bpm / 60.0) / self.sample_rate as f64;
        let in_count_in = self.is_playing && self.count_in_beats_remaining > 1e-5;
        let arrangement_playing = self.is_playing && !in_count_in;
        let block_start_beat = self.playhead_beat;
        let block_end_beat = block_start_beat + beats_per_sample * frames as f64;
        let any_track_solo = project.tracks.iter().take(num_tracks).any(|t| t.solo);
        let any_vca_solo = project.vca_groups.iter().any(|v| v.solo);
        let any_solo = any_track_solo || any_vca_solo;

        self.fill_topo_order(project, num_tracks);
        self.last_vocal_pitch = VocalPitchTelemetry::default();

        for ord_pos in 0..self.topo_scratch.len() {
            let t_idx = self.topo_scratch[ord_pos];
            let track = &project.tracks[t_idx];

            let vca = track
                .vca_group_id
                .and_then(|vid| project.vca_groups.iter().find(|v| v.id == vid));
            let vca_muted = vca.map(|v| v.mute).unwrap_or(false);
            let vca_soloed = vca.map(|v| v.solo).unwrap_or(false);
            let vca_gain_db = vca.map(|v| v.gain_db).unwrap_or(0.0);

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
                && !vca_muted
                && (!any_solo || track.solo || folder_soloed || child_soloed || vca_soloed);

            let mut max_gr_db = 0.0_f32;

            // FAST PATH: 1-Click Frozen Track streams pre-rendered 32-bit stem at ~0.0% DSP CPU!
            if track.frozen {
                if arrangement_playing {
                    if let Some(ref fbuf) = track.frozen_buffer {
                        let start_sec = project.beats_to_seconds(block_start_beat);
                        let dt_sec = 1.0 / self.sample_rate as f64;
                        for i in 0..frames {
                            let pos = (start_sec + i as f64 * dt_sec) * fbuf.sample_rate as f64;
                            let (l, r) = fbuf.sample_interpolated(pos);
                            self.bus_l[t_idx][i] += l;
                            self.bus_r[t_idx][i] += r;
                        }
                    }
                }
            } else {
                self.midi_scratch.clear();
                self.clip_midi_expr = MidiExpression {
                    pitch_bend_semitones: self.live_pitch_bend_semi,
                    mod_wheel: self.live_mod_wheel,
                    expression: self.live_expression,
                    brightness_offset: self.live_brightness,
                };

                // 1. Live MIDI input (including Sustain Pedal CC64)
                for &(target_opt, ev) in live_midi {
                    let matches = match target_opt {
                        Some(tid) => tid == track.id,
                        None => track.record_arm,
                    };
                    if matches {
                        if !ev.is_note_on && self.sustain_pedal_down {
                            self.held_sustain_notes[(ev.note as usize) & 127] = true;
                        } else {
                            self.midi_scratch.push(ev);
                        }
                    }
                }

                // 2. Live hardware audio input monitoring (multi-channel matrix + Line-In)
                if track.record_arm && track.input_monitoring {
                    let ig = self.input_gain_lin;
                    match track.input_source.hardware_channels() {
                        Some((l_ch, Some(r_ch))) => {
                            let ls = hw_ins.get(l_ch as usize).copied().unwrap_or(hw_in_l);
                            let rs = hw_ins.get(r_ch as usize).copied().unwrap_or(hw_in_r);
                            if ls.len() >= frames && rs.len() >= frames {
                                for i in 0..frames {
                                    self.bus_l[t_idx][i] += ls[i] * ig;
                                    self.bus_r[t_idx][i] += rs[i] * ig;
                                }
                            }
                        }
                        Some((ch, None)) => {
                            let ms = hw_ins.get(ch as usize).copied().unwrap_or(hw_in_l);
                            if ms.len() >= frames {
                                for i in 0..frames {
                                    let m = ms[i] * ig;
                                    self.bus_l[t_idx][i] += m;
                                    self.bus_r[t_idx][i] += m;
                                }
                            }
                        }
                        None => {}
                    }
                }

                // 3. Session Clip Launcher OR Linear Arrangement Clips
                if arrangement_playing {
                    if let Some(slot_idx) = track.active_session_slot {
                        if let Some(Some(slot)) = track.session_slots.get(slot_idx) {
                            self.evaluate_session_slot(
                                slot,
                                t_idx,
                                frames,
                                block_start_beat,
                                beats_per_sample,
                                current_bpm,
                            );
                        }
                    } else {
                        for (c_idx, clip) in track.clips.iter().enumerate() {
                            if !clip.muted {
                                self.evaluate_timeline_clip(
                                    project,
                                    track,
                                    c_idx,
                                    clip,
                                    t_idx,
                                    frames,
                                    block_start_beat,
                                    block_end_beat,
                                    beats_per_sample,
                                    current_bpm,
                                );
                            }
                        }
                    }
                }

                self.midi_scratch.sort_by_key(|e| e.sample_offset);

                // 3a. Chord-Track Harmonic Follow: snap MIDI pitches to active chord tones!
                if track.follow_chord_track {
                    if let Some(ch) = project.chord_at_beat(block_start_beat) {
                        for ev in &mut self.midi_scratch {
                            ev.note = ch.snap_pitch_to_chord(ev.note);
                        }
                    }
                }

                // 3b. Track MIDI Arpeggiator / Note-Repeat!
                self.arp_lanes[t_idx].process_midi_block(
                    &track.arpeggiator,
                    frames,
                    self.sample_rate,
                    current_bpm,
                    arrangement_playing,
                    block_start_beat,
                    &mut self.midi_scratch,
                );
                self.midi_scratch.sort_by_key(|e| e.sample_offset);
                let pre_rms =
                    compute_rms(&self.bus_l[t_idx][..frames], &self.bus_r[t_idx][..frames]);

                // 4. Modulators
                for m in &track.modulators {
                    if m.enabled {
                        if let Some(rt) = self.mod_pool.iter_mut().find(|r| r.id == m.id) {
                            rt.evaluate_block(
                                &m.kind,
                                self.sample_rate,
                                frames,
                                current_bpm,
                                block_start_beat,
                                pre_rms,
                            );
                        }
                    }
                }

                // 5. Pre-FX Sends
                self.dispatch_sends(project, track, t_idx, frames, SendTap::PreFx, audible);

                // 6. Track FX Chain (with Bezier Parameter Automation + Modulators + External Sidechain + CLAP/VST3 Dynamic Params!)
                for fx in &track.fx_chain {
                    if !fx.enabled {
                        continue;
                    }
                    let p_count = fx.param_count();
                    self.param_scratch.clear();
                    for p_idx in 0..p_count {
                        let base_val = fx.param(p_idx);
                        let mut norm = fx.normalize_param(p_idx, base_val);

                        // Apply curved Bezier Parameter Automation if Read/Touch/Latch is active!
                        if self.is_playing
                            && track.automation_mode != ferric_model::AutomationMode::Off
                        {
                            for lane in &track.automation_lanes {
                                if lane.target
                                    == (AutomationTarget::FxParam {
                                        fx_id: fx.id,
                                        param_index: p_idx,
                                    })
                                {
                                    if let Some(auto_norm) = lane.eval_at(block_start_beat) {
                                        norm = auto_norm;
                                    }
                                }
                            }
                        }

                        // Sum Modulator contributions
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
                        self.param_scratch.push(fx.denormalize_param(p_idx, norm));
                    }

                    let mut wet = fx.wet_mix;
                    if self.is_playing
                        && track.automation_mode != ferric_model::AutomationMode::Off
                    {
                        for lane in &track.automation_lanes {
                            if lane.target == (AutomationTarget::FxWet { fx_id: fx.id }) {
                                if let Some(v) = lane.eval_at(block_start_beat) {
                                    wet = v;
                                }
                            }
                        }
                    }

                    // Resolve optional External Sidechain Key Track!
                    let sc_idx_opt = fx.sidechain_source.and_then(|sc_id| {
                        project
                            .tracks
                            .iter()
                            .take(num_tracks)
                            .position(|pt| pt.id == sc_id)
                    });

                    if let Some(inst) = self.fx_pool.iter_mut().find(|i| i.id == fx.id) {
                        let sc_slices = sc_idx_opt
                            .map(|si| (&self.sc_l[si][..frames], &self.sc_r[si][..frames]));
                        inst.process_block_full(
                            &mut self.bus_l[t_idx][..frames],
                            &mut self.bus_r[t_idx][..frames],
                            &self.param_scratch,
                            wet,
                            current_bpm,
                            &self.midi_scratch,
                            self.clip_midi_expr,
                            sc_slices,
                        );
                        if fx.is_external_plugin() && fx.plugin_output_trim_db.abs() > 0.01 {
                            let trim = db_to_lin(fx.plugin_output_trim_db);
                            for s in &mut self.bus_l[t_idx][..frames] {
                                *s *= trim;
                            }
                            for s in &mut self.bus_r[t_idx][..frames] {
                                *s *= trim;
                            }
                        }
                        max_gr_db = max_gr_db.max(inst.gain_reduction_db());
                        if let Some(vp) = inst.vocal_pitch_telemetry() {
                            if vp.active {
                                self.last_vocal_pitch = vp;
                            }
                        }
                    }
                }
            }

            // Save Post-FX signal into `sc_l`/`sc_r` for any downstream Sidechain Compressors/Gates!
            self.sc_l[t_idx][..frames].copy_from_slice(&self.bus_l[t_idx][..frames]);
            self.sc_r[t_idx][..frames].copy_from_slice(&self.bus_r[t_idx][..frames]);

            // Apply Automatic PDC Delay alignment for parallel tracks heading to Master!
            if track.parent_folder_id.is_none() && max_pdc > track_latencies[t_idx] {
                let comp = max_pdc - track_latencies[t_idx];
                self.pdc_lines[t_idx].delay_block(
                    &mut self.bus_l[t_idx][..frames],
                    &mut self.bus_r[t_idx][..frames],
                    comp,
                );
            }

            // 7. Pre-Fader Sends
            self.dispatch_sends(
                project,
                track,
                t_idx,
                frames,
                SendTap::PreFaderPostFx,
                audible,
            );

            // 8. Fader, Automation, Modulators, Pan & Width
            let mut mod_gain_offset_db = 0.0_f32;
            let mut mod_pan_offset = 0.0_f32;
            for m in &track.modulators {
                if m.enabled {
                    if let Some(rt) = self.mod_pool.iter().find(|r| r.id == m.id) {
                        for route in &m.routings {
                            match route.target {
                                ModTarget::TrackGain => {
                                    mod_gain_offset_db += rt.last_output * route.depth * 18.0
                                }
                                ModTarget::TrackPan => mod_pan_offset += rt.last_output * route.depth,
                                _ => {}
                            }
                        }
                    }
                }
            }

            let mut fader_db = track.gain_db + mod_gain_offset_db + vca_gain_db;
            let mut pan = (track.pan + mod_pan_offset).clamp(-1.0, 1.0);
            let mut width = track.stereo_width.clamp(0.0, 2.0);
            if self.is_playing && track.automation_mode != ferric_model::AutomationMode::Off {
                for lane in &track.automation_lanes {
                    if let Some(v) = lane.eval_at(block_start_beat) {
                        match lane.target {
                            AutomationTarget::Volume => {
                                fader_db = -60.0 + v * 72.0 + vca_gain_db
                            }
                            AutomationTarget::Pan => pan = v * 2.0 - 1.0,
                            AutomationTarget::StereoWidth => width = v * 2.0,
                            _ => {}
                        }
                    }
                }
            }

            let gain_lin = if audible { db_to_lin(fader_db) } else { 0.0 };
            let phase_sign = if track.phase_invert { -1.0 } else { 1.0 };
            let pan_angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
            let pan_l = pan_angle.cos();
            let pan_r = pan_angle.sin();

            let mut peak_l = 0.0_f32;
            let mut peak_r = 0.0_f32;
            let mut sum_sq = 0.0_f32;

            for i in 0..frames {
                let raw_l = self.bus_l[t_idx][i] * phase_sign;
                let raw_r = self.bus_r[t_idx][i] * phase_sign;
                let mid = 0.5 * (raw_l + raw_r);
                let side = 0.5 * (raw_l - raw_r) * width;
                let l = (mid + side) * gain_lin * pan_l * std::f32::consts::SQRT_2;
                let r = (mid - side) * gain_lin * pan_r * std::f32::consts::SQRT_2;
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
                pdc_samples: track_latencies[t_idx].min(u16::MAX as usize) as u16,
            };

            // 9. Post-Fader Sends
            self.dispatch_sends(project, track, t_idx, frames, SendTap::PostFader, audible);

            // 10. Sum into Parent Folder OR Master Bus
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
            if fx.enabled {
                if let Some(inst) = self.fx_pool.iter_mut().find(|i| i.id == fx.id) {
                    inst.process_block(
                        &mut out_l[..frames],
                        &mut out_r[..frames],
                        &fx.params,
                        fx.wet_mix,
                        current_bpm,
                        &[],
                    );
                }
            }
        }

        // 12. Metronome Click (always clicks during Count-In pre-roll, or when Click is enabled!)
        if self.is_playing && (in_count_in || project.metronome_enabled) {
            let click_ref_beat = if in_count_in {
                let bpb = project
                    .time_signature_at_beat(block_start_beat)
                    .beats_per_bar();
                let total_ci = (project.count_in_bars.max(1) as f64) * bpb;
                total_ci - self.count_in_beats_remaining
            } else {
                block_start_beat
            };
            self.mix_metronome(
                project,
                click_ref_beat,
                beats_per_sample,
                &mut out_l[..frames],
                &mut out_r[..frames],
            );
        }

        // 13. Master Gain, Dim, Mono Check & Safety Limiter
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

        if self.is_playing {
            if in_count_in {
                let step_b = beats_per_sample * frames as f64;
                if self.count_in_beats_remaining <= step_b {
                    self.count_in_beats_remaining = 0.0;
                    self.last_click_step = -1;
                } else {
                    self.count_in_beats_remaining -= step_b;
                }
            } else {
                let mut next_beat = block_end_beat;
                if project.loop_enabled
                    && project.loop_end_beat > project.loop_start_beat + 0.25
                    && block_start_beat < project.loop_end_beat
                    && next_beat >= project.loop_end_beat
                {
                    let overshoot = next_beat - project.loop_end_beat;
                    next_beat = project.loop_start_beat + overshoot;
                }
                self.playhead_beat = next_beat;
            }
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
            TakeContent::Midi { notes, .. } => {
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

    #[allow(clippy::too_many_arguments)]
    fn evaluate_timeline_clip(
        &mut self,
        project: &Project,
        track: &Track,
        clip_idx: usize,
        clip: &TimelineClip,
        t_idx: usize,
        frames: usize,
        block_start_beat: f64,
        block_end_beat: f64,
        beats_per_sample: f64,
        current_bpm: f64,
    ) {
        if clip.end_beat() < block_start_beat
            || clip.start_beat > block_end_beat
            || clip.takes.is_empty()
        {
            return;
        }
        let varispeed_ratio = 2.0_f64.powf(clip.pitch_semitones as f64 / 12.0);
        let pitch_semi_round = clip.pitch_semitones.round() as i32;
        let spb = 60.0 / current_bpm.max(20.0);

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
                continue;
            };

            let take_idx = clip.take_at_local_beat(local_beat);
            // Combine Clip Gain + Fade In/Out + Automatic Equal-Power Overlap Crossfade!
            let env = clip.gain_envelope_at(local_beat) * track.auto_crossfade_gain(clip_idx, beat);

            if let Some(take) = clip.takes.get(take_idx) {
                match &take.content {
                    TakeContent::Midi { notes, cc_points } => {
                        if i == 0 && !cc_points.is_empty() {
                            if let Some(pb) =
                                eval_midi_cc_curve(cc_points, MidiCcTarget::PitchBend, inner_beat)
                            {
                                self.clip_midi_expr.pitch_bend_semitones += (pb - 0.5) * 4.0;
                            }
                            if let Some(mw) =
                                eval_midi_cc_curve(cc_points, MidiCcTarget::ModWheel, inner_beat)
                            {
                                self.clip_midi_expr.mod_wheel =
                                    self.clip_midi_expr.mod_wheel.max(mw);
                            }
                            if let Some(ex) =
                                eval_midi_cc_curve(cc_points, MidiCcTarget::Expression, inner_beat)
                            {
                                self.clip_midi_expr.expression *= ex;
                            }
                            if let Some(br) = eval_midi_cc_curve(
                                cc_points,
                                MidiCcTarget::FilterBrightness,
                                inner_beat,
                            ) {
                                self.clip_midi_expr.brightness_offset += (br - 0.5) * 2.0;
                            }
                        }
                        let b1 = inner_beat
                            + beats_per_sample * (clip.stretch_rate.clamp(0.125, 8.0) as f64);
                        // DNA Feel is the live project macro; track.groove_amount is the
                        // per-track pocket when DNA Feel is at 0 (e.g. after Apply + Feel=0).
                        // Do NOT multiply — Apply copies dna_feel into groove_amount.
                        let (groove_tmpl, feel) = if project.dna_feel > 0.001 {
                            (
                                project.dna_groove_template,
                                project.dna_feel.clamp(0.0, 1.0),
                            )
                        } else {
                            (track.groove_template, track.groove_amount.clamp(0.0, 1.0))
                        };
                        let dna_seed = project.id.as_u128() as u32;
                        for n in notes {
                            let note_abs = clip.start_beat + n.start_beat;
                            let (beat_off, groove_vel) =
                                groove_tmpl.transform_note(note_abs, feel);
                            let dna_vel = project.dna_accent_shape.velocity_scale(
                                note_abs,
                                project.dna_velocity,
                                dna_seed,
                            );
                            let eff_start = n.start_beat + beat_off;
                            let eff_end = n.end_beat() + beat_off;
                            if eff_start >= inner_beat && eff_start < b1 {
                                let p = (n.pitch as i32 + pitch_semi_round).clamp(0, 127) as u8;
                                let v = ((n.velocity as f32 * env * groove_vel * dna_vel).round()
                                    as i32)
                                    .clamp(1, 127) as u8;
                                self.midi_scratch.push(MidiEvent {
                                    sample_offset: i,
                                    note: p,
                                    velocity: v,
                                    is_note_on: true,
                                });
                            }
                            if eff_end >= inner_beat && eff_end < b1 {
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
                        let source_sec = clip.map_beat_to_source_seconds(inner_beat, spb);
                        let eff_pitch_semi = clip.effective_pitch_semitones_at(local_beat);
                        let (mut l, mut r) = match clip.stretch_mode {
                            TimeStretchMode::ElastiqueWsola => {
                                sample_wsola_decoupled(buffer, source_sec, eff_pitch_semi)
                            }
                            TimeStretchMode::TapeVarispeed => {
                                let ratio = 2.0_f64.powf(eff_pitch_semi as f64 / 12.0);
                                let _ = varispeed_ratio;
                                buffer.sample_interpolated(
                                    source_sec * (buffer.sample_rate as f64) * ratio,
                                )
                            }
                        };

                        // Smooth crossfade across Swipe-Comping boundaries
                        let xfade_beats = 0.025;
                        for seg in &clip.comp_segments {
                            let dist = (local_beat - seg.start_beat).abs();
                            if seg.start_beat > 0.001 && dist < xfade_beats {
                                let prev_take =
                                    clip.take_at_local_beat((seg.start_beat - 0.001).max(0.0));
                                if prev_take != take_idx {
                                    if let Some(TakeContent::Audio { buffer: prev_buf }) =
                                        clip.takes.get(prev_take).map(|t| &t.content)
                                    {
                                        let (pl, pr) = sample_wsola_decoupled(
                                            prev_buf,
                                            source_sec,
                                            eff_pitch_semi,
                                        );
                                        let alpha = ((local_beat - (seg.start_beat - xfade_beats))
                                            / (2.0 * xfade_beats))
                                            .clamp(0.0, 1.0)
                                            as f32;
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
            for fx in &clip.clip_fx {
                if fx.enabled {
                    if let Some(inst) = self.fx_pool.iter_mut().find(|x| x.id == fx.id) {
                        inst.process_block(
                            &mut self.clip_l[..frames],
                            &mut self.clip_r[..frames],
                            &fx.params,
                            fx.wet_mix,
                            current_bpm,
                            &[],
                        );
                    }
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
                let pl = pan_angle.cos() * std::f32::consts::SQRT_2 * g;
                let pr = pan_angle.sin() * std::f32::consts::SQRT_2 * g;
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
        for i in 0..out_l.len() {
            let beat = block_start_beat + i as f64 * beats_per_sample;
            let step = beat.floor() as i64;
            if step != self.last_click_step && (beat - step as f64) < 0.05 {
                self.last_click_step = step;
                self.click_env = 1.0;
                self.click_phase = 0.0;
                let (_bar, in_bar_beat, _ts) = project.bar_and_beat_at(beat);
                self.click_freq = if in_bar_beat < 0.15 {
                    1560.0 // Bar downbeat accent (respects mid-song 3/4, 7/8, 6/8 meter changes!)
                } else {
                    1040.0
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
        for pass in 0..5 {
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
                        || ot.sends.iter().any(|s| s.target_track_id == t.id)
                        || t.fx_chain.iter().any(|f| f.sidechain_source == Some(ot.id));
                    if feeds_this {
                        deps_ready = false;
                        break;
                    }
                }
                if deps_ready || pass == 4 {
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
