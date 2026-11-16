//! Sequential function tests for each button/action handler.
//! Run: `ferricdaw.exe --function-test` or `tools\FunctionTest-Windows.ps1`

use crate::browser::{BrowserAction, BrowserTab};
use crate::palette::PaletteCommand;
use crate::{BottomTab, FerricDawApp, PendingDestructive};
use ferric_model::{
    synthesize_browser_sample, FxKind, MidiNote, NightCurrentStep, TakeContent, TimelineClip,
    FACTORY_PRESETS,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

type CheckFn = fn(&FerricDawApp) -> Result<(), String>;

struct Case {
    name: &'static str,
    button: &'static str,
    act: fn(&mut FerricDawApp),
    check: CheckFn,
}

fn ok(_: &FerricDawApp) -> Result<(), String> {
    Ok(())
}

fn check_tracks_ge_2(app: &FerricDawApp) -> Result<(), String> {
    if app.project.tracks.len() >= 2 {
        Ok(())
    } else {
        Err(format!(
            "expected >=2 tracks, got {}",
            app.project.tracks.len()
        ))
    }
}

fn check_organ_track(app: &FerricDawApp) -> Result<(), String> {
    let track = app.selected_track.and_then(|id| app.project.find_track(id)).ok_or("no selected track")?;
    if track.fx_chain.iter().any(|fx| fx.kind == FxKind::DrawbarOrgan) { Ok(()) } else { Err("selected track has no organ".into()) }
}

fn check_electric_piano_track(app: &FerricDawApp) -> Result<(), String> {
    let track = app.selected_track.and_then(|id| app.project.find_track(id)).ok_or("no selected track")?;
    if track.fx_chain.iter().any(|fx| fx.kind == FxKind::ElectricPiano) { Ok(()) } else { Err("selected track has no electric piano".into()) }
}

fn check_tracks_ge_3(app: &FerricDawApp) -> Result<(), String> {
    if app.project.tracks.len() >= 3 {
        Ok(())
    } else {
        Err(format!(
            "expected >=3 tracks, got {}",
            app.project.tracks.len()
        ))
    }
}

fn check_help_open(app: &FerricDawApp) -> Result<(), String> {
    if app.show_help_modal {
        Ok(())
    } else {
        Err("help modal not open".into())
    }
}

fn check_tutorial_open(app: &FerricDawApp) -> Result<(), String> {
    if app.tutorial.open {
        Ok(())
    } else {
        Err("tutorial coach not open".into())
    }
}

fn check_loop_toggled(app: &FerricDawApp) -> Result<(), String> {
    // Project defaults to loop_enabled=true; ToggleLoop flips it off.
    if !app.project.loop_enabled {
        Ok(())
    } else {
        Err("loop still enabled after toggle".into())
    }
}

fn check_metronome_on(app: &FerricDawApp) -> Result<(), String> {
    if app.project.metronome_enabled {
        Ok(())
    } else {
        Err("metronome not enabled".into())
    }
}

fn check_bounce_open(app: &FerricDawApp) -> Result<(), String> {
    if app.bounce_modal.open {
        Ok(())
    } else {
        Err("bounce modal not open".into())
    }
}

fn check_demo_loaded(app: &FerricDawApp) -> Result<(), String> {
    if app.project.tracks.len() >= 4 {
        Ok(())
    } else {
        Err(format!(
            "Afterglow playground expected many tracks, got {}",
            app.project.tracks.len()
        ))
    }
}

fn check_fx_on_selected(app: &FerricDawApp) -> Result<(), String> {
    let Some(tid) = app.selected_track else {
        return Err("no selected track".into());
    };
    let trk = app
        .project
        .find_track(tid)
        .ok_or_else(|| "selected track missing".to_string())?;
    if trk.fx_chain.is_empty() {
        Err("FX chain empty after insert".into())
    } else {
        Ok(())
    }
}

fn check_modulator(app: &FerricDawApp) -> Result<(), String> {
    let Some(tid) = app.selected_track else {
        return Err("no selected track".into());
    };
    let trk = app
        .project
        .find_track(tid)
        .ok_or_else(|| "selected track missing".to_string())?;
    if trk.modulators.is_empty() {
        Err("no modulators after Add LFO".into())
    } else {
        Ok(())
    }
}

fn check_pads_enabled(app: &FerricDawApp) -> Result<(), String> {
    if app.keyboard_pads.enabled {
        Ok(())
    } else {
        Err("keyboard pads not enabled after dual-keyboard spawn".into())
    }
}

fn check_input_gain(app: &FerricDawApp) -> Result<(), String> {
    if (app.browser.input_gain_db - 6.0).abs() < 0.01 {
        Ok(())
    } else {
        Err(format!(
            "input gain expected 6.0, got {}",
            app.browser.input_gain_db
        ))
    }
}

fn check_bpm_locked(app: &FerricDawApp) -> Result<(), String> {
    if (app.project.bpm - 128.0).abs() < 0.01 {
        Ok(())
    } else {
        Err(format!("BPM expected 128, got {}", app.project.bpm))
    }
}

fn check_offset(app: &FerricDawApp) -> Result<(), String> {
    if app.browser.recording_offset_samples == 480 {
        Ok(())
    } else {
        Err(format!(
            "offset expected 480, got {}",
            app.browser.recording_offset_samples
        ))
    }
}

fn check_sample_clip(app: &FerricDawApp) -> Result<(), String> {
    let Some(tid) = app.selected_track else {
        return Err("no selected track".into());
    };
    let trk = app
        .project
        .find_track(tid)
        .ok_or_else(|| "selected track missing".to_string())?;
    if trk.clips.iter().any(|c| !c.is_midi()) {
        Ok(())
    } else {
        Err("no audio clip after drop sample".into())
    }
}

fn check_chords_step(app: &FerricDawApp) -> Result<(), String> {
    if app.project.show_chord_lane && app.project.chord_track.len() >= 16 {
        Ok(())
    } else {
        Err(format!(
            "Chords step expected chord lane + >=16 chords (lane={}, chords={})",
            app.project.show_chord_lane,
            app.project.chord_track.len()
        ))
    }
}

fn seed_selected_track(app: &mut FerricDawApp) {
    if app.selected_track.is_none() {
        app.selected_track = app.project.tracks.first().map(|t| t.id);
    }
}

fn seed_midi_clip(app: &mut FerricDawApp) {
    seed_selected_track(app);
    let Some(tid) = app.selected_track else {
        return;
    };
    if let Some(trk) = app.project.find_track_mut(tid) {
        if trk.clips.is_empty() {
            let notes = vec![
                MidiNote::new(60, 100, 0.0, 1.0),
                MidiNote::new(64, 100, 1.0, 1.0),
            ];
            let clip = TimelineClip::new_midi("FT Notes", 0.0, 4.0, notes);
            let cid = clip.id;
            trk.clips.push(clip);
            app.selected_clip = Some((tid, cid));
        } else if let Some(c) = trk.clips.first() {
            app.selected_clip = Some((tid, c.id));
        }
    }
}

macro_rules! pal {
    ($name:literal, $btn:literal, $cmd:expr, $check:expr) => {
        Case {
            name: $name,
            button: $btn,
            act: |app| {
                seed_selected_track(app);
                app.execute_palette_command($cmd);
            },
            check: $check,
        }
    };
}

fn catalog() -> Vec<Case> {
    vec![
        pal!(
            "transport_play",
            "Transport ▶ Play",
            PaletteCommand::TogglePlay,
            ok
        ),
        pal!(
            "transport_stop_via_play",
            "Transport ■ Stop (toggle play off)",
            PaletteCommand::TogglePlay,
            ok
        ),
        pal!(
            "transport_record",
            "Transport ● Rec",
            PaletteCommand::ToggleRecord,
            ok
        ),
        pal!(
            "transport_record_off",
            "Transport Rec off",
            PaletteCommand::ToggleRecord,
            ok
        ),
        pal!(
            "transport_loop",
            "Transport Loop",
            PaletteCommand::ToggleLoop,
            check_loop_toggled
        ),
        pal!(
            "transport_click",
            "Transport Click/Metronome",
            PaletteCommand::ToggleMetronome,
            check_metronome_on
        ),
        pal!(
            "retro_capture",
            "Capture (retroactive)",
            PaletteCommand::CaptureRetroactive,
            ok
        ),
        pal!(
            "add_universal_track",
            "+ Track / Add Universal",
            PaletteCommand::AddUniversalTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_synth_track",
            "Palette: Add Synth Track",
            PaletteCommand::AddSynthTrack,
            check_tracks_ge_2
        ),
        pal!("add_organ_track", "Palette: Add Drawbar Organ", PaletteCommand::AddOrganTrack, check_organ_track),
        pal!("add_electric_piano_track", "Palette: Add Electric Piano", PaletteCommand::AddElectricPianoTrack, check_electric_piano_track),
        pal!(
            "add_mono_bass",
            "Palette: Add Mono Bass",
            PaletteCommand::AddMonoBassTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_prism",
            "Palette: Add Prism",
            PaletteCommand::AddPrismTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_host_dna",
            "Palette: Add HostDNA",
            PaletteCommand::AddHostDnaTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_pad",
            "Palette: Add Soft Pad",
            PaletteCommand::AddPadTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_perc",
            "Palette: Add Percussion",
            PaletteCommand::AddPercTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_wavetable",
            "Palette: Add Wavetable",
            PaletteCommand::AddWavetableSynthTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_sampler",
            "Palette: Add Sampler",
            PaletteCommand::AddSamplerTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_vocal",
            "Palette: Add Vocal Pitch",
            PaletteCommand::AddVocalPitchTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_drum",
            "Palette: Add Drum Kit",
            PaletteCommand::AddDrumTrack,
            check_tracks_ge_2
        ),
        pal!(
            "add_guitar_amp",
            "Palette: Add Guitar Amp",
            PaletteCommand::AddGuitarAmpTrack,
            check_tracks_ge_2
        ),
        Case {
            name: "insert_fx_eq",
            button: "Palette / + Add FX (EQ)",
            act: |app| {
                seed_selected_track(app);
                app.execute_palette_command(PaletteCommand::InsertFxOnSelected(
                    FxKind::ParametricEq,
                ));
            },
            check: check_fx_on_selected,
        },
        pal!(
            "add_lfo",
            "Palette: + Add Modulator (LFO)",
            PaletteCommand::AddLfoModulator,
            check_modulator
        ),
        Case {
            name: "split_clip",
            button: "Edit: Split at playhead",
            act: |app| {
                seed_midi_clip(app);
                app.engine.latest_telemetry.playhead_beat = 1.0;
                app.execute_palette_command(PaletteCommand::SplitSelectedClipAtPlayhead);
            },
            check: ok,
        },
        Case {
            name: "quantize_clip",
            button: "MIDI: Quantize",
            act: |app| {
                seed_midi_clip(app);
                app.execute_palette_command(PaletteCommand::QuantizeSelectedClip);
            },
            check: ok,
        },
        pal!(
            "conductor_lane",
            "Tempo Map / Conductor lane",
            PaletteCommand::ToggleConductorLane,
            ok
        ),
        pal!(
            "open_help",
            "Help / F1",
            PaletteCommand::OpenQuickstartHelp,
            check_help_open
        ),
        pal!(
            "start_tutorial",
            "Tutorial",
            PaletteCommand::StartNightCurrentTutorial,
            check_tutorial_open
        ),
        pal!(
            "open_bounce",
            "Bounce / Ctrl+B",
            PaletteCommand::OpenBounceModal,
            check_bounce_open
        ),
        Case {
            name: "bounce_shortcut_quick_release",
            button: "Ctrl+B with modifier released in the same frame",
            act: |app| {
                let ctx = eframe::egui::Context::default();
                app.bounce_modal.open = false;
                let _ = ctx.run(
                    eframe::egui::RawInput {
                        events: vec![eframe::egui::Event::Key {
                            key: eframe::egui::Key::B,
                            physical_key: Some(eframe::egui::Key::B),
                            pressed: true,
                            repeat: false,
                            modifiers: eframe::egui::Modifiers::CTRL,
                        }],
                        ..Default::default()
                    },
                    |ctx| app.handle_shortcuts_and_musical_typing(ctx),
                );
            },
            check: check_bounce_open,
        },
        pal!("undo", "Edit Undo", PaletteCommand::Undo, ok),
        pal!("redo", "Edit Redo", PaletteCommand::Redo, ok),
        Case {
            name: "load_afterglow",
            button: "Ferric → Afterglow Copper Playground",
            act: |app| {
                app.execute_destructive(PendingDestructive::LoadDemo);
            },
            check: check_demo_loaded,
        },
        Case {
            name: "new_empty",
            button: "Ferric → New Empty Session",
            act: |app| {
                app.execute_destructive(PendingDestructive::NewEmpty);
            },
            check: |app| {
                if app.project.tracks.len() == 1 {
                    Ok(())
                } else {
                    Err(format!(
                        "empty session expected 1 track, got {}",
                        app.project.tracks.len()
                    ))
                }
            },
        },
        Case {
            name: "hardware_rig_spawn",
            button: "Hardware Rig → Spawn MIDI + Line-In Rig",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::SetupHardwareRig]);
            },
            check: check_tracks_ge_3,
        },
        Case {
            name: "hardware_rig_midi_only",
            button: "Hardware Rig → MIDI only",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::SetupHardwareRigPreset(2)]);
            },
            check: check_tracks_ge_2,
        },
        Case {
            name: "hardware_rig_line_in_only",
            button: "Hardware Rig → Line-In only",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::SetupHardwareRigPreset(3)]);
            },
            check: check_tracks_ge_2,
        },
        Case {
            name: "fix_speakers_line_in",
            button: "Hardware Rig → Fix: Force Speakers + Line-In",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::FixLineInComboJackRouting]);
            },
            check: ok,
        },
        Case {
            name: "dual_keyboard_studio",
            button: "Hardware Rig → Spawn Dual Keyboard Studio",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::SpawnDualKeyboardStudio]);
            },
            check: check_pads_enabled,
        },
        Case {
            name: "rescan_midi",
            button: "USB MIDI → Rescan USB",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::RescanMidi]);
            },
            check: ok,
        },
        Case {
            name: "set_input_gain",
            button: "Audio I/O → Line-In Gain / +6 DS",
            act: |app| {
                app.browser.input_gain_db = 6.0;
                app.process_browser_actions(vec![BrowserAction::SetInputGainDb(6.0)]);
            },
            check: check_input_gain,
        },
        Case {
            name: "set_recording_offset",
            button: "Audio I/O → Recording offset",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::SetRecordingOffsetSamples(480)]);
            },
            check: check_offset,
        },
        Case {
            name: "loopback_ping",
            button: "Audio I/O → Loopback Ping",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::TriggerLoopbackPing]);
            },
            check: ok,
        },
        Case {
            name: "lock_detected_bpm",
            button: "Audio I/O → Lock BPM",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::LockDetectedBpm(128.0)]);
            },
            check: check_bpm_locked,
        },
        Case {
            name: "kaoss_note_on_off",
            button: "Kaoss / XY → note trigger",
            act: |app| {
                seed_selected_track(app);
                app.process_browser_actions(vec![
                    BrowserAction::KaossNote {
                        note: 60,
                        is_note_on: true,
                    },
                    BrowserAction::KaossNote {
                        note: 60,
                        is_note_on: false,
                    },
                ]);
            },
            check: ok,
        },
        Case {
            name: "apply_factory_preset",
            button: "Presets → Apply first factory preset",
            act: |app| {
                seed_selected_track(app);
                let preset = FACTORY_PRESETS[0].clone();
                app.process_browser_actions(vec![BrowserAction::ApplyPreset(preset)]);
            },
            check: check_fx_on_selected,
        },
        Case {
            name: "dark_techno_devices",
            button: "Presets → Load industrial and metal kits, synth, and Kick Rumble",
            act: |app| {
                seed_selected_track(app);
                for name in ["Foundry Industrial Kit", "Ironbreak Metal Kit", "Blackout Stab", "Warehouse Kick Rumble"] {
                    let preset = FACTORY_PRESETS.iter().find(|p| p.name == name).expect(name);
                    app.process_browser_actions(vec![BrowserAction::ApplyPreset(preset.clone())]);
                }
            },
            check: |app| {
                let track = app.selected_track.and_then(|id| app.project.find_track(id));
                let Some(track) = track else { return Err("No selected track".into()); };
                let expected = [
                    ("Foundry Industrial Kit", FxKind::DrumKit, Some(1.0)),
                    ("Ironbreak Metal Kit", FxKind::DrumKit, Some(2.0)),
                    ("Blackout Stab", FxKind::PolySynth, None),
                    ("Warehouse Kick Rumble", FxKind::KickRumble, None),
                ];
                for (name, kind, voice) in expected {
                    if !track.fx_chain.iter().any(|slot| {
                        slot.preset_name == name && slot.kind == kind
                            && voice.is_none_or(|voice| slot.param(6) == voice)
                    }) {
                        return Err(format!("Preset failed to load: {name}"));
                    }
                }
                Ok(())
            },
        },
        Case {
            name: "dirty_session_move_install_guard",
            button: "Move FerricDAW → restart with unsaved work",
            act: |app| {
                app.snapshot_before("Edit session before moving");
                app.request_destructive(PendingDestructive::MoveInstall);
            },
            check: |app| {
                if matches!(app.pending_destructive, Some(PendingDestructive::MoveInstall))
                    && !app.quit_for_update
                {
                    Ok(())
                } else {
                    Err("Move bypassed the unsaved-work dialog".into())
                }
            },
        },
        Case {
            name: "dirty_session_update_guard",
            button: "Updates → Install & Restart with unsaved work",
            act: |app| {
                app.snapshot_before("Edit session before updating");
                app.request_destructive(PendingDestructive::InstallUpdate);
            },
            check: |app| {
                if matches!(app.pending_destructive, Some(PendingDestructive::InstallUpdate))
                    && !app.quit_for_update
                {
                    Ok(())
                } else {
                    Err("Update bypassed the unsaved-work dialog".into())
                }
            },
        },
        Case {
            name: "drop_sample_on_track",
            button: "Samples → Drop on track",
            act: |app| {
                seed_selected_track(app);
                let (name, buf) =
                    synthesize_browser_sample(0, app.engine.sample_rate, app.project.bpm);
                app.process_browser_actions(vec![BrowserAction::DropSampleOnTrack {
                    name,
                    buffer: buf,
                }]);
            },
            check: check_sample_clip,
        },
        Case {
            name: "preview_sample",
            button: "Samples → Preview",
            act: |app| {
                let (_name, buf) =
                    synthesize_browser_sample(1, app.engine.sample_rate, app.project.bpm);
                app.process_browser_actions(vec![BrowserAction::PreviewSample(buf)]);
            },
            check: ok,
        },
        Case {
            name: "clear_plugin_quarantine",
            button: "Plugins → Clear quarantine",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::ClearPluginQuarantine]);
            },
            check: ok,
        },
        Case {
            name: "clear_playground",
            button: "Plugins → Clear playground",
            act: |app| {
                app.process_browser_actions(vec![BrowserAction::ClearPlayground]);
            },
            check: ok,
        },
        Case {
            name: "tutorial_apply_session",
            button: "Tutorial → Begin Session",
            act: |app| {
                app.execute_destructive(PendingDestructive::TutorialApply(
                    NightCurrentStep::Session,
                ));
            },
            check: ok,
        },
        Case {
            name: "tutorial_apply_chords",
            button: "Tutorial → Do this step (Chords)",
            act: |app| {
                app.execute_destructive(PendingDestructive::TutorialApply(
                    NightCurrentStep::Chords,
                ));
            },
            check: check_chords_step,
        },
        Case {
            name: "play_demo_reference",
            button: "Tutorial → Play demo WAV",
            act: |app| {
                app.play_night_current_demo_reference();
            },
            check: ok,
        },
        Case {
            name: "bottom_tab_kaoss",
            button: "Lower dock → Kaoss tab",
            act: |app| {
                app.bottom_tab = BottomTab::KaossPad;
                app.show_bottom_dock = true;
            },
            check: |app| {
                if app.bottom_tab == BottomTab::KaossPad && app.show_bottom_dock {
                    Ok(())
                } else {
                    Err("Kaoss dock tab not active".into())
                }
            },
        },
        Case {
            name: "bottom_tab_mix",
            button: "Lower dock → Mix tab",
            act: |app| {
                app.bottom_tab = BottomTab::MixingConsole;
            },
            check: |app| {
                if app.bottom_tab == BottomTab::MixingConsole {
                    Ok(())
                } else {
                    Err("Mix tab not active".into())
                }
            },
        },
        Case {
            name: "browser_tab_samples",
            button: "Browser → Samples",
            act: |app| {
                app.browser.tab = BrowserTab::SamplesAndLoops;
                app.browser.visible = true;
            },
            check: |app| {
                if app.browser.tab == BrowserTab::SamplesAndLoops {
                    Ok(())
                } else {
                    Err("Samples tab not active".into())
                }
            },
        },
        Case {
            name: "browser_tab_presets",
            button: "Browser → Presets / Plugins",
            act: |app| {
                app.browser.tab = BrowserTab::PresetsAndClap;
            },
            check: |app| {
                if app.browser.tab == BrowserTab::PresetsAndClap {
                    Ok(())
                } else {
                    Err("Presets tab not active".into())
                }
            },
        },
        Case {
            name: "browser_tab_hardware",
            button: "Browser → Hardware Rig",
            act: |app| {
                app.browser.tab = BrowserTab::HardwareRig;
            },
            check: |app| {
                if app.browser.tab == BrowserTab::HardwareRig {
                    Ok(())
                } else {
                    Err("Hardware Rig tab not active".into())
                }
            },
        },
        Case {
            name: "write_xy_toggle",
            button: "Kaoss → Write XY automation",
            act: |app| {
                app.kaoss_pad.write_xy_automation = true;
            },
            check: |app| {
                if app.kaoss_pad.write_xy_automation {
                    Ok(())
                } else {
                    Err("Write XY not enabled".into())
                }
            },
        },
        Case {
            name: "midi_backspace_delete",
            button: "Piano roll → Backspace delete notes",
            act: |app| {
                seed_midi_clip(app);
                let Some((_, cid)) = app.selected_clip else {
                    return;
                };
                let Some(tid) = app.selected_track else {
                    return;
                };
                if let Some(clip) = app
                    .project
                    .find_track_mut(tid)
                    .and_then(|t| t.find_clip_mut(cid))
                {
                    let active = clip.active_take.min(clip.takes.len().saturating_sub(1));
                    if let Some(TakeContent::Midi { notes, .. }) =
                        clip.takes.get_mut(active).map(|t| &mut t.content)
                    {
                        notes.clear();
                    }
                }
            },
            check: |app| {
                let Some(tid) = app.selected_track else {
                    return Err("no track".into());
                };
                let Some((_, cid)) = app.selected_clip else {
                    return Err("no clip".into());
                };
                let clip = app
                    .project
                    .find_track(tid)
                    .and_then(|t| t.clips.iter().find(|c| c.id == cid))
                    .ok_or("missing clip")?;
                let active = clip.active_take.min(clip.takes.len().saturating_sub(1));
                match clip.takes.get(active).map(|t| &t.content) {
                    Some(TakeContent::Midi { notes, .. }) if notes.is_empty() => Ok(()),
                    _ => Err("notes remain after Backspace simulation".into()),
                }
            },
        },
    ]
}

/// Run every button function test sequentially. Returns process exit code.
pub fn run() -> i32 {
    // Release builds use windows_subsystem=windows — attach a console so PASS/FAIL prints.
    #[cfg(windows)]
    unsafe {
        let _ = windows_sys::Win32::System::Console::AllocConsole();
    }
    println!("FerricDAW function test — each button handler, then next");
    println!("========================================================");
    let cases = catalog();
    let total = cases.len();
    let mut passed = 0usize;
    let mut failed = 0usize;

    for (i, case) in cases.into_iter().enumerate() {
        let n = i + 1;
        print!("[{n:02}/{total}] {btn} … ", btn = case.button);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut app = FerricDawApp::new_core(None);
            app.show_first_run = false;
            (case.act)(&mut app);
            (case.check)(&app)
        }));
        match result {
            Ok(Ok(())) => {
                println!("PASS ({})", case.name);
                passed += 1;
            }
            Ok(Err(e)) => {
                println!("FAIL ({}) — {e}", case.name);
                failed += 1;
            }
            Err(_) => {
                println!("FAIL ({}) — panicked", case.name);
                failed += 1;
            }
        }
    }

    println!("========================================================");
    println!("PASS {passed}  FAIL {failed}  TOTAL {total}");
    if failed == 0 {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod save_guard_tests {
    use super::*;

    #[test]
    fn failed_save_keeps_pending_action_and_session() {
        let mut app = FerricDawApp::new_core(None);
        app.project.add_track("Do not discard");
        let count = app.project.tracks.len();
        app.pending_destructive = Some(PendingDestructive::NewEmpty);
        // A directory cannot be opened as a project file; no user data is overwritten.
        let saved = app.save_project_to_path(&std::env::temp_dir());
        assert!(!saved);
        app.continue_after_save(saved, PendingDestructive::NewEmpty);
        assert!(app.pending_destructive.is_some());
        assert_eq!(app.project.tracks.len(), count);
        assert!(app.status_banner.starts_with("Save error:"));
    }

    #[test]
    fn successful_save_continues_pending_transition() {
        let mut app = FerricDawApp::new_core(None);
        app.project.add_track("Saved track");
        app.pending_destructive = Some(PendingDestructive::NewEmpty);
        let path = std::env::temp_dir().join(format!("ferric-save-guard-{}.ferric", std::process::id()));
        let saved = app.save_project_to_path(&path);
        assert!(saved, "{}", app.status_banner);
        app.continue_after_save(saved, PendingDestructive::NewEmpty);
        assert!(app.pending_destructive.is_none());
        assert!(app.project.tracks.iter().all(|track| track.name != "Saved track"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn autosave_does_not_clear_unsaved_project_guard() {
        let mut app = FerricDawApp::new_core(None);
        app.snapshot_before("Change test session");
        // Recovery autosave clears only this flag, without saving the user's project.
        app.dirty_since_autosave = false;
        assert!(app.session_has_user_work());
        app.request_destructive(PendingDestructive::NewEmpty);
        assert!(app.pending_destructive.is_some());
    }
}
