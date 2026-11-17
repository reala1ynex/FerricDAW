use ferric_dsp::{ExternalPluginAdapter, MidiEvent};
use ferric_engine::{
    scan_all_external_plugins, FloatingPluginWindow, HostedClapInstance, HostedVst3Instance,
    NativePluginGuiView, PluginQuarantine,
};
use ferric_model::ExternalPluginFormat;
use std::path::PathBuf;

fn workspace_plugins_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("plugins")
}

#[test]
fn verify_real_downloaded_clap_and_vst3_plugins_end_to_end() {
    let plugins_dir = workspace_plugins_dir();
    if !plugins_dir.exists() {
        eprintln!("Skipping real plugin test: {:?} not present", plugins_dir);
        return;
    }

    let mut quarantine = PluginQuarantine::default();
    let discovered = scan_all_external_plugins(std::slice::from_ref(&plugins_dir), &mut quarantine);
    println!(
        "\n=== FerricDAW Real Plugin Scanner Found {} Plugins ===",
        discovered.len()
    );
    for p in &discovered {
        println!(
            "  [{:?}] '{}' by '{}' (id: {}) -> {}",
            p.format,
            p.name,
            p.vendor,
            p.id,
            p.bundle_path.display()
        );
    }

    let clap_plugins: Vec<_> = discovered
        .iter()
        .filter(|p| p.format == ExternalPluginFormat::Clap)
        .collect();
    let vst3_plugins: Vec<_> = discovered
        .iter()
        .filter(|p| p.format == ExternalPluginFormat::Vst3)
        .collect();

    assert!(
        !clap_plugins.is_empty(),
        "Expected at least 1 real .clap plugin in {:?}",
        plugins_dir
    );
    assert!(
        !vst3_plugins.is_empty(),
        "Expected at least 1 real .vst3 plugin in {:?}",
        plugins_dir
    );

    // -----------------------------------------------------------------------
    // 1. Test Real Third-Party .CLAP Plugin(s) (Audio/MIDI Synth + Params + State + HWND GUI!)
    // -----------------------------------------------------------------------
    for cp in &clap_plugins {
        println!("\n--- Testing Real CLAP Plugin: {} ({}) ---", cp.name, cp.id);
        let mut loaded = HostedClapInstance::load_full(
            &cp.bundle_path,
            &cp.id,
            48_000.0,
            512,
            None,
        )
        .unwrap_or_else(|e| panic!("Failed to load real CLAP plugin {}: {}", cp.name, e));

        println!(
            "  Discovered {} real CLAP parameters (latency: {} samples)",
            loaded.discovered_params.len(),
            loaded.audio_processor.latency_samples()
        );
        for (i, param) in loaded.discovered_params.iter().take(6).enumerate() {
            println!(
                "    Param[{}]: '{}' (id={}) range=[{} .. {}] default={}",
                i, param.name, param.plugin_param_id, param.min, param.max, param.default
            );
        }
        assert!(
            !loaded.discovered_params.is_empty(),
            "Real CLAP plugin '{}' should expose parameters",
            cp.name
        );

        let params_vec: Vec<f32> = loaded.discovered_params.iter().map(|p| p.default).collect();

        // Trigger a C-major triad (MIDI 60, 64, 67) and render 16 blocks (8192 frames = ~170ms)
        let note_on_events = [
            MidiEvent {
                sample_offset: 0,
                note: 60,
                velocity: 112,
                is_note_on: true,
            },
            MidiEvent {
                sample_offset: 0,
                note: 64,
                velocity: 108,
                is_note_on: true,
            },
            MidiEvent {
                sample_offset: 0,
                note: 67,
                velocity: 108,
                is_note_on: true,
            },
        ];

        let mut max_peak = 0.0_f32;
        let mut total_rms_energy = 0.0_f64;
        for blk in 0..16 {
            let mut l = vec![0.0_f32; 512];
            let mut r = vec![0.0_f32; 512];
            let evs: &[MidiEvent] = if blk == 0 { &note_on_events } else { &[] };
            loaded
                .audio_processor
                .process(&mut l, &mut r, &params_vec, 120.0, evs);

            assert!(
                l.iter().all(|x| x.is_finite()) && r.iter().all(|x| x.is_finite()),
                "CLAP output contained NaN/Inf!"
            );
            for i in 0..512 {
                max_peak = max_peak.max(l[i].abs()).max(r[i].abs());
                total_rms_energy += (l[i] * l[i] + r[i] * r[i]) as f64;
            }
        }
        println!(
            "  CLAP Rendered C-Major Chord Audio -> Peak: {:.4} ({:.1} dBFS), Energy: {:.4}",
            max_peak,
            20.0 * max_peak.max(1e-6).log10(),
            total_rms_energy
        );
        assert!(
            max_peak > 0.005,
            "Expected real synthesized audio from CLAP synth '{}', got peak {}",
            cp.name,
            max_peak
        );

        // Scientifically verify real-time Parameter Automation (`CLAP_EVENT_PARAM_VALUE`)!
        if cp.name == "Six Sines" || cp.name == "Dexed" {
            let mut mod_bundle = HostedClapInstance::load_full(
                &cp.bundle_path,
                &cp.id,
                48_000.0,
                512,
                None,
            )
            .unwrap();
            let mut mod_params = params_vec.clone();
            // Zero out Output/Level parameters to verify real-time parameter event modulation!
            for (idx, desc) in mod_bundle.discovered_params.iter().enumerate() {
                let nm = desc.name.to_lowercase();
                if nm.contains("output") || nm.contains("level") || nm.contains("cutoff") {
                    mod_params[idx] = desc.min;
                }
            }
            let mut mod_peak = 0.0_f32;
            for blk in 0..20 {
                let mut ml = vec![0.0_f32; 512];
                let mut mr = vec![0.0_f32; 512];
                let p_slice = if blk < 2 { &params_vec } else { &mod_params };
                let evs: &[MidiEvent] = if blk == 6 { &note_on_events } else { &[] };
                mod_bundle
                    .audio_processor
                    .process(&mut ml, &mut mr, p_slice, 120.0, evs);
                ferric_engine::plugin_window::pump_win32_messages();
                mod_bundle.gui_and_state.poll_main_thread();
                if blk >= 6 {
                    for i in 0..512 {
                        mod_peak = mod_peak.max(ml[i].abs()).max(mr[i].abs());
                    }
                }
            }
            println!(
                "  CLAP [{}] Parameter Modulation Verified: Default Peak={:.4} vs Modulated Peak={:.5}",
                cp.name, max_peak, mod_peak
            );
            assert!(
                mod_peak < max_peak * 0.25,
                "Expected CLAP parameter events on '{}' to dramatically change DSP output ({} vs {})!",
                cp.name,
                mod_peak,
                max_peak
            );
        }

        // Test Binary Patch State Save & Reload (`clap.state`)
        if let Some(state_bytes) = loaded.gui_and_state.save_state_bytes() {
            println!(
                "  CLAP clap.state saved {} bytes — verifying round-trip load...",
                state_bytes.len()
            );
            assert!(!state_bytes.is_empty());
            assert!(loaded.gui_and_state.load_state_bytes(&state_bytes));
        }

        // Test Native Win32 HWND Editor Window Open & Clean Close (`clap.gui`)
        let mut win = FloatingPluginWindow::new_headless(
            uuid::Uuid::new_v4(),
            format!("Test CLAP — {}", cp.name),
            Box::new(loaded.gui_and_state),
        );
        win.open_window()
            .unwrap_or_else(|e| panic!("Failed to open Win32 HWND editor for CLAP {}: {}", cp.name, e));
        win.poll();
        assert!(win.is_editor_open);
        println!("  CLAP Native Win32 HWND GUI opened & attached cleanly!");
        win.close_window();
        assert!(!win.is_editor_open);
    }

    // -----------------------------------------------------------------------
    // 2. Test Real Third-Party .VST3 Plugin(s) (COM Audio/MIDI Synth + Params + State + HWND GUI!)
    // -----------------------------------------------------------------------
    for vp in &vst3_plugins {
        println!("\n--- Testing Real VST3 Plugin: {} ({}) ---", vp.name, vp.id);
        let mut loaded = HostedVst3Instance::load_full(
            &vp.bundle_path,
            &vp.id,
            48_000.0,
            512,
            None,
        )
        .unwrap_or_else(|e| panic!("Failed to load real VST3 plugin {}: {}", vp.name, e));

        println!(
            "  Discovered {} real VST3 parameters (latency: {} samples)",
            loaded.discovered_params.len(),
            loaded.audio_processor.latency_samples()
        );
        for (i, param) in loaded.discovered_params.iter().take(6).enumerate() {
            println!(
                "    Param[{}]: '{}' (id={}) default={:.3}",
                i, param.name, param.plugin_param_id, param.default
            );
        }
        assert!(
            !loaded.discovered_params.is_empty(),
            "Real VST3 plugin '{}' should expose parameters",
            vp.name
        );

        let params_vec: Vec<f32> = loaded.discovered_params.iter().map(|p| p.default).collect();
        let note_on_events = [
            MidiEvent {
                sample_offset: 0,
                note: 60,
                velocity: 112,
                is_note_on: true,
            },
            MidiEvent {
                sample_offset: 0,
                note: 64,
                velocity: 108,
                is_note_on: true,
            },
            MidiEvent {
                sample_offset: 0,
                note: 67,
                velocity: 108,
                is_note_on: true,
            },
        ];

        let mut max_peak = 0.0_f32;
        let mut total_rms_energy = 0.0_f64;
        for blk in 0..16 {
            let mut l = vec![0.0_f32; 512];
            let mut r = vec![0.0_f32; 512];
            let evs: &[MidiEvent] = if blk == 0 { &note_on_events } else { &[] };
            loaded
                .audio_processor
                .process(&mut l, &mut r, &params_vec, 120.0, evs);

            assert!(
                l.iter().all(|x| x.is_finite()) && r.iter().all(|x| x.is_finite()),
                "VST3 output contained NaN/Inf!"
            );
            for i in 0..512 {
                max_peak = max_peak.max(l[i].abs()).max(r[i].abs());
                total_rms_energy += (l[i] * l[i] + r[i] * r[i]) as f64;
            }
        }
        println!(
            "  VST3 Rendered C-Major Chord Audio -> Peak: {:.4} ({:.1} dBFS), Energy: {:.4}",
            max_peak,
            20.0 * max_peak.max(1e-6).log10(),
            total_rms_energy
        );
        assert!(
            max_peak > 0.005,
            "Expected real synthesized audio from VST3 synth '{}', got peak {}",
            vp.name,
            max_peak
        );

        // Scientifically verify real-time VST3 `IParameterChanges`!
        if vp.name == "Six Sines" || vp.name == "Dexed" {
            let mut mod_bundle = HostedVst3Instance::load_full(
                &vp.bundle_path,
                &vp.id,
                48_000.0,
                512,
                None,
            )
            .unwrap();
            let mut mod_params = params_vec.clone();
            for (idx, desc) in mod_bundle.discovered_params.iter().enumerate() {
                let nm = desc.name.to_lowercase();
                if nm.contains("output") || nm.contains("level") || nm.contains("cutoff") {
                    mod_params[idx] = desc.min;
                }
            }
            let mut mod_peak = 0.0_f32;
            for blk in 0..20 {
                let mut ml = vec![0.0_f32; 512];
                let mut mr = vec![0.0_f32; 512];
                let p_slice = if blk < 2 { &params_vec } else { &mod_params };
                let evs: &[MidiEvent] = if blk == 6 { &note_on_events } else { &[] };
                mod_bundle
                    .audio_processor
                    .process(&mut ml, &mut mr, p_slice, 120.0, evs);
                ferric_engine::plugin_window::pump_win32_messages();
                if blk >= 6 {
                    for i in 0..512 {
                        mod_peak = mod_peak.max(ml[i].abs()).max(mr[i].abs());
                    }
                }
            }
            println!(
                "  VST3 [{}] Parameter Modulation Verified: Default Peak={:.4} vs Modulated Peak={:.5}",
                vp.name, max_peak, mod_peak
            );
            assert!(
                mod_peak < max_peak * 0.25,
                "Expected VST3 IParameterChanges on '{}' to dramatically change DSP output ({} vs {})!",
                vp.name,
                mod_peak,
                max_peak
            );
        }

        // Test VST3 `IBStream` Binary Patch State Save & Reload
        if let Some(state_bytes) = loaded.gui_and_state.save_state_bytes() {
            println!(
                "  VST3 IBStream state saved {} bytes — verifying round-trip load...",
                state_bytes.len()
            );
            assert!(state_bytes.len() > 12);
            assert!(loaded.gui_and_state.load_state_bytes(&state_bytes));
        }

        // Test VST3 `IPlugView` Native Win32 HWND Editor Open & Clean Close
        let mut win = FloatingPluginWindow::new_headless(
            uuid::Uuid::new_v4(),
            format!("Test VST3 — {}", vp.name),
            Box::new(loaded.gui_and_state),
        );
        win.open_window()
            .unwrap_or_else(|e| panic!("Failed to open Win32 HWND editor for VST3 {}: {}", vp.name, e));
        win.poll();
        assert!(win.is_editor_open);
        println!("  VST3 Native Win32 HWND GUI opened & attached cleanly!");
        win.close_window();
        assert!(!win.is_editor_open);
    }

    // -----------------------------------------------------------------------
    // 3. Full FerricDAW Multi-Track Project Integration Render (Dexed CLAP + Six Sines VST3 -> WAV!)
    // -----------------------------------------------------------------------
    let dexed_clap = clap_plugins.iter().find(|p| p.name == "Dexed").unwrap();
    let six_sines_vst3 = vst3_plugins.iter().find(|p| p.name == "Six Sines").unwrap();

    let mut proj = ferric_model::Project::new_empty("Real Third-Party CLAP + VST3 Mix");
    proj.bpm = 120.0;

    let t1_id = proj.add_track("01 Dexed DX7 (CLAP)");
    let mut dex_slot = ferric_model::FxSlot::new_external_plugin(
        ExternalPluginFormat::Clap,
        dexed_clap.bundle_path.clone(),
        dexed_clap.id.clone(),
        dexed_clap.name.clone(),
    );
    let dex_bundle =
        HostedClapInstance::load_full(&dexed_clap.bundle_path, &dexed_clap.id, 48_000.0, 512, None)
            .unwrap();
    dex_slot.apply_discovered_params(dex_bundle.discovered_params);
    let dex_fx_id = dex_slot.id;
    proj.find_track_mut(t1_id).unwrap().fx_chain.push(dex_slot);
    proj.find_track_mut(t1_id)
        .unwrap()
        .clips
        .push(ferric_model::TimelineClip::new_midi(
            "DX7 Chords",
            0.0,
            4.0,
            vec![
                ferric_model::MidiNote::new(60, 108, 0.0, 2.0),
                ferric_model::MidiNote::new(64, 104, 0.0, 2.0),
                ferric_model::MidiNote::new(67, 104, 0.0, 2.0),
            ],
        ));

    let t2_id = proj.add_track("02 Six Sines (VST3)");
    let mut six_slot = ferric_model::FxSlot::new_external_plugin(
        ExternalPluginFormat::Vst3,
        six_sines_vst3.bundle_path.clone(),
        six_sines_vst3.id.clone(),
        six_sines_vst3.name.clone(),
    );
    let six_bundle = HostedVst3Instance::load_full(
        &six_sines_vst3.bundle_path,
        &six_sines_vst3.id,
        48_000.0,
        512,
        None,
    )
    .unwrap();
    six_slot.apply_discovered_params(six_bundle.discovered_params);
    let six_fx_id = six_slot.id;
    proj.find_track_mut(t2_id).unwrap().fx_chain.push(six_slot);
    proj.find_track_mut(t2_id)
        .unwrap()
        .clips
        .push(ferric_model::TimelineClip::new_midi(
            "Six Sines Lead",
            0.0,
            4.0,
            vec![
                ferric_model::MidiNote::new(72, 110, 0.5, 1.5),
                ferric_model::MidiNote::new(76, 110, 2.0, 1.5),
            ],
        ));

    let mut graph = ferric_engine::AudioGraphCore::new(48_000.0, 512);
    graph.sync_processors_from_project(&proj);
    graph.register_fx_instance(ferric_dsp::FxRuntimeInstance::new_external_clap(
        dex_fx_id,
        Box::new(dex_bundle.audio_processor),
        512,
    ));
    graph.register_fx_instance(ferric_dsp::FxRuntimeInstance::new_external_clap(
        six_fx_id,
        Box::new(six_bundle.audio_processor),
        512,
    ));
    graph.is_playing = true;

    let total_frames = 48_000 * 2; // 2.0 seconds @ 120 BPM = 4 beats
    let mut mix_l = Vec::with_capacity(total_frames);
    let mut mix_r = Vec::with_capacity(total_frames);
    let mut blk_l = [0.0_f32; 512];
    let mut blk_r = [0.0_f32; 512];
    let zero_in = [0.0_f32; 512];

    for _ in 0..(total_frames / 512) {
        graph.render_block(&proj, &[&zero_in, &zero_in], &[], &mut blk_l, &mut blk_r);
        mix_l.extend_from_slice(&blk_l);
        mix_r.extend_from_slice(&blk_r);
    }

    let wav_out = plugins_dir.join("verified_dexed_clap_plus_six_sines_vst3_mix.wav");
    ferric_engine::write_wav_file(&wav_out, 48_000, ferric_engine::BitDepth::Pcm24, &mix_l, &mix_r)
        .expect("write combined plugin mix wav");
    let mix_peak = mix_l.iter().fold(0.0_f32, |a, &x| a.max(x.abs()));
    println!(
        "\n=== Full FerricDAW Graph Mix (Dexed.clap + Six Sines.vst3) -> {} (Peak: {:.3}) ===",
        wav_out.display(),
        mix_peak
    );
    assert!(mix_peak > 0.05 && mix_peak <= 1.0);
}
