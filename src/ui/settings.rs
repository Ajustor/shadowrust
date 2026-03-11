use crate::app::UiAction;
use crate::config::{AudioCodecPref, VideoCodecPref};
use super::state::UiState;

/// Draw the main settings panel window.
pub(super) fn draw_settings(ctx: &egui::Context, state: &mut UiState) {
    egui::Window::new("ShadowRust")
        .default_pos([16.0, 16.0])
        .resizable(false)
        .show(ctx, |ui| {
            draw_video_device_section(ui, state);
            ui.separator();
            draw_audio_section(ui, state);
            ui.separator();
            draw_resolution_section(ui, state);
            ui.separator();
            draw_capture_controls(ui, state);
            ui.separator();
            ui.label(format!("Frames dropped: {}", state.frames_dropped));
            ui.separator();
            ui.small(format!(
                "Tab: toggle panel  |  F11: Fullscreen  |  Esc: Quit | Version: {}",
                env!("CARGO_PKG_VERSION")
            ));
        });
}

fn draw_video_device_section(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("📹 Capture Device");

    if state.devices.is_empty() {
        ui.colored_label(
            egui::Color32::from_rgb(255, 180, 0),
            "⚠ No capture device detected",
        );
        if ui.button("🔄 Scan devices").clicked() {
            state.devices_loaded = false;
        }
    } else {
        let label = state
            .devices
            .get(state.selected_device)
            .cloned()
            .unwrap_or_default();
        let prev_device = state.selected_device;

        egui::ComboBox::from_id_salt("video-device")
            .selected_text(&label)
            .show_ui(ui, |ui| {
                for (i, name) in state.devices.clone().iter().enumerate() {
                    ui.selectable_value(&mut state.selected_device, i, name);
                }
            });

        if state.selected_device != prev_device {
            state.device_resolutions.clear();
            state
                .pending_actions
                .push(UiAction::QueryDeviceResolutions {
                    device_index: state.selected_device,
                });
            if state.capturing {
                state.pending_actions.push(UiAction::RestartCapture {
                    device_index: state.selected_device,
                    width: state.width,
                    height: state.height,
                    fps: state.fps,
                });
            }
        }

        if ui.button("🔄 Refresh").clicked() {
            state.devices_loaded = false;
        }
    }
}

fn draw_audio_section(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("🔊 Audio");

    ui.horizontal(|ui| {
        if state.audio_active {
            if state.muted {
                ui.colored_label(egui::Color32::GRAY, "🔇 Muted");
            } else {
                ui.colored_label(egui::Color32::from_rgb(80, 200, 80), "🔊 Live");
            }
            let mute_label = if state.muted { "Unmute" } else { "Mute" };
            if ui.button(mute_label).clicked() {
                state.muted = !state.muted;
                let vol = if state.muted { 0.0 } else { state.volume };
                state
                    .pending_actions
                    .push(UiAction::SetVolume { volume: vol });
            }
        } else if ui.button("🔊 Start audio").clicked() {
            let hint = state
                .audio_devices
                .get(state.selected_audio_device)
                .cloned()
                .unwrap_or_default();
            state
                .pending_actions
                .push(UiAction::StartAudio { device_hint: hint });
            state.audio_active = true;
        }
        if ui
            .button("🔄")
            .on_hover_text("Refresh audio devices")
            .clicked()
        {
            state.audio_devices_loaded = false;
        }
    });

    // Volume slider
    ui.horizontal(|ui| {
        ui.label("Volume:");
        let prev_vol = state.volume;
        ui.add(
            egui::Slider::new(&mut state.volume, 0.0..=2.0)
                .step_by(0.05)
                .show_value(true)
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
        );
        if (state.volume - prev_vol).abs() > 0.001 {
            state.pending_actions.push(UiAction::SetVolume {
                volume: state.volume,
            });
        }
    });
}

fn draw_resolution_section(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("⚙️ Settings");

    let prev_w = state.width;
    let prev_h = state.height;
    let prev_fps = state.fps;

    if !state.device_resolutions.is_empty() {
        let label = state
            .device_resolutions
            .get(state.selected_resolution_idx)
            .map(|r| {
                if state.custom_resolution {
                    "Custom".to_string()
                } else {
                    r.label.clone()
                }
            })
            .unwrap_or_default();

        egui::ComboBox::from_label("Resolution")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (i, r) in state.device_resolutions.clone().iter().enumerate() {
                    if ui
                        .selectable_value(&mut state.selected_resolution_idx, i, &r.label)
                        .clicked()
                    {
                        state.width = r.width;
                        state.height = r.height;
                        state.fps = r.max_fps;
                        state.custom_resolution = false;
                    }
                }
                ui.separator();
                if ui
                    .selectable_label(state.custom_resolution, "Custom")
                    .clicked()
                {
                    state.custom_resolution = true;
                }
            });
    } else {
        const PRESETS: &[(&str, u32, u32, u32)] = &[
            ("1080p — 1920×1080 @ 60fps", 1920, 1080, 60),
            ("1440p — 2560×1440 @ 60fps", 2560, 1440, 60),
            ("4K    — 3840×2160 @ 30fps", 3840, 2160, 30),
            ("Custom", 0, 0, 0),
        ];
        let preset_label = if state.custom_resolution {
            "Custom"
        } else {
            PRESETS
                .get(state.selected_resolution_idx)
                .map(|(l, _, _, _)| *l)
                .unwrap_or("1080p")
        };
        egui::ComboBox::from_label("Resolution")
            .selected_text(preset_label)
            .show_ui(ui, |ui| {
                for (i, (label, w, h, fps)) in PRESETS.iter().enumerate() {
                    if ui
                        .selectable_value(&mut state.selected_resolution_idx, i, *label)
                        .clicked()
                        && *w != 0
                    {
                        state.width = *w;
                        state.height = *h;
                        state.fps = *fps;
                        state.custom_resolution = false;
                    }
                }
            });
        if state.selected_resolution_idx == PRESETS.len() - 1 {
            state.custom_resolution = true;
        }
    }

    if state.custom_resolution {
        ui.horizontal(|ui| {
            ui.label("Width:");
            ui.add(
                egui::DragValue::new(&mut state.width)
                    .range(320..=3840)
                    .speed(8.0),
            );
            ui.label("Height:");
            ui.add(
                egui::DragValue::new(&mut state.height)
                    .range(240..=2160)
                    .speed(8.0),
            );
        });

        ui.horizontal(|ui| {
            ui.label("FPS:");
            egui::ComboBox::from_id_salt("fps")
                .selected_text(state.fps.to_string())
                .show_ui(ui, |ui| {
                    for &f in &[30u32, 60, 120] {
                        ui.selectable_value(&mut state.fps, f, f.to_string());
                    }
                });
        });
    }

    // If resolution changed while capturing → restart automatically
    let res_changed = state.width != prev_w || state.height != prev_h || state.fps != prev_fps;
    if res_changed && state.capturing && !state.custom_resolution {
        state.pending_actions.push(UiAction::RestartCapture {
            device_index: state.selected_device,
            width: state.width,
            height: state.height,
            fps: state.fps,
        });
    }
}

fn draw_capture_controls(ui: &mut egui::Ui, state: &mut UiState) {
    if !state.capturing {
        if ui.button("▶ Start Capture").clicked() {
            state.pending_actions.push(UiAction::StartCapture {
                device_index: state.selected_device,
                width: state.width,
                height: state.height,
                fps: state.fps,
            });
            state.capturing = true;
        }
    } else {
        if ui.button("⏹ Stop Capture").clicked() {
            if state.recording {
                state.pending_actions.push(UiAction::StopRecording);
                state.recording = false;
            }
            state.pending_actions.push(UiAction::StopCapture);
            state.capturing = false;
            state.audio_active = false;
        }

        ui.separator();
        ui.heading("🔴 Recording");

        ui.horizontal(|ui| {
            ui.label("Output file:");
            ui.text_edit_singleline(&mut state.record_path);
        });

        // ── Codec selection ───────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label("Vidéo codec:");
            let video_label = state.video_codec.label();
            egui::ComboBox::from_id_salt("video-codec")
                .selected_text(video_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut state.video_codec,
                        VideoCodecPref::H264Auto,
                        VideoCodecPref::H264Auto.label(),
                    );
                    ui.selectable_value(
                        &mut state.video_codec,
                        VideoCodecPref::H264Sw,
                        VideoCodecPref::H264Sw.label(),
                    );
                    ui.selectable_value(
                        &mut state.video_codec,
                        VideoCodecPref::H265Auto,
                        VideoCodecPref::H265Auto.label(),
                    );
                    ui.selectable_value(
                        &mut state.video_codec,
                        VideoCodecPref::H265Sw,
                        VideoCodecPref::H265Sw.label(),
                    );
                });
        });

        ui.horizontal(|ui| {
            ui.label("Audio codec:");
            egui::ComboBox::from_id_salt("audio-codec")
                .selected_text(state.audio_codec.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut state.audio_codec,
                        AudioCodecPref::Aac,
                        AudioCodecPref::Aac.label(),
                    );
                    ui.selectable_value(
                        &mut state.audio_codec,
                        AudioCodecPref::Opus,
                        AudioCodecPref::Opus.label(),
                    );
                });
        });
        if matches!(state.audio_codec, AudioCodecPref::Opus) {
            ui.small("ℹ Opus recommandé avec .mkv");
        }

        if !state.recording {
            if ui.button("⏺ Start Recording").clicked() {
                state.pending_actions.push(UiAction::StartRecording {
                    path: state.record_path.clone(),
                });
                state.recording = true;
            }
        } else {
            ui.colored_label(egui::Color32::RED, "● Recording…");
            if ui.button("⏹ Stop Recording").clicked() {
                state.pending_actions.push(UiAction::StopRecording);
                state.recording = false;
            }
        }
    }
}
