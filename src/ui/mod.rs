use crate::app::UiAction;
use crate::audio::AudioPassthrough;
use crate::capture::{DeviceResolution, list_devices};

#[derive(Default)]
pub struct UiState {
    pub capturing: bool,
    pub recording: bool,
    pub audio_active: bool,
    pub menu_visible: bool,
    pub selected_device: usize,
    pub selected_audio_device: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub fps_display: f32,       // updated every frame by app.rs
    pub record_path: String,
    pub latency_ms: f32,
    pub frames_dropped: u64,
    pub pending_actions: Vec<UiAction>,
    // Video devices
    devices: Vec<String>,
    devices_loaded: bool,
    // Audio input devices
    audio_devices: Vec<String>,
    audio_devices_loaded: bool,
    // Resolutions queried from the driver (empty until query completes)
    device_resolutions: Vec<DeviceResolution>,
    selected_resolution_idx: usize,
    // Whether to show custom width/height drag values
    custom_resolution: bool,
}

impl UiState {
    /// Receive driver-reported resolutions from the background query thread.
    pub fn set_device_resolutions(&mut self, resolutions: Vec<DeviceResolution>) {
        self.device_resolutions = resolutions;
        // Pre-select 1080p if available, otherwise the largest.
        self.selected_resolution_idx = self
            .device_resolutions
            .iter()
            .rposition(|r| r.height == 1080)
            .unwrap_or(self.device_resolutions.len().saturating_sub(1));
        if let Some(r) = self.device_resolutions.get(self.selected_resolution_idx) {
            self.width = r.width;
            self.height = r.height;
            self.fps = r.max_fps;
        }
        self.custom_resolution = false;
    }

    fn load_video_devices(&mut self) {
        if !self.devices_loaded {
            self.devices = list_devices();
            self.devices_loaded = true;
        }
    }

    fn load_audio_devices(&mut self) {
        if !self.audio_devices_loaded {
            self.audio_devices = AudioPassthrough::list_input_devices();
            self.audio_devices_loaded = true;
        }
    }
}

/// The `fps` field on `UiState` is kept in sync from app.rs.
/// Rename to avoid shadowing the struct field below.
impl UiState {
    // fps_display is written by app.rs; this alias makes intent clear.
}

pub fn draw(ctx: &egui::Context, state: &mut UiState) {
    state.load_video_devices();
    state.load_audio_devices();

    // ── Always-visible FPS / hint overlay ────────────────────────────────────
    egui::Window::new("##fps")
        .title_bar(false)
        .resizable(false)
        .movable(false)
        .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
        .frame(egui::Frame::none().fill(egui::Color32::from_black_alpha(140)).inner_margin(6.0))
        .show(ctx, |ui| {
            ui.colored_label(
                egui::Color32::from_rgb(180, 255, 180),
                format!("{:.1} FPS", state.fps_display),
            );
            if !state.menu_visible {
                ui.small(egui::RichText::new("Tab — show settings").color(egui::Color32::GRAY));
            }
        });

    if !state.menu_visible {
        return;
    }

    // ── Settings panel ────────────────────────────────────────────────────────
    egui::Window::new("ShadowRust")
        .default_pos([16.0, 16.0])
        .resizable(false)
        .show(ctx, |ui| {
            // ── Video device ─────────────────────────────────────────────────
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
                let label = state.devices.get(state.selected_device).cloned().unwrap_or_default();
                let prev = state.selected_device;

                egui::ComboBox::from_id_salt("video-device")
                    .selected_text(&label)
                    .show_ui(ui, |ui| {
                        for (i, name) in state.devices.clone().iter().enumerate() {
                            ui.selectable_value(&mut state.selected_device, i, name);
                        }
                    });

                // When device changes, query its resolutions.
                if state.selected_device != prev {
                    state.device_resolutions.clear();
                    state.pending_actions.push(UiAction::QueryDeviceResolutions {
                        device_index: state.selected_device,
                    });
                }

                if ui.button("🔄 Refresh").clicked() {
                    state.devices_loaded = false;
                }
            }

            ui.separator();

            // ── Audio device ─────────────────────────────────────────────────
            ui.heading("🔊 Audio Device");

            if state.audio_devices.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 180, 0),
                    "⚠ No audio input device detected",
                );
            } else {
                let audio_label = state
                    .audio_devices
                    .get(state.selected_audio_device)
                    .cloned()
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("audio-device")
                    .selected_text(&audio_label)
                    .show_ui(ui, |ui| {
                        for (i, name) in state.audio_devices.clone().iter().enumerate() {
                            ui.selectable_value(&mut state.selected_audio_device, i, name);
                        }
                    });
            }

            ui.horizontal(|ui| {
                if state.audio_active {
                    ui.colored_label(egui::Color32::from_rgb(80, 200, 80), "🔊 Audio live");
                    if ui.button("🔇 Mute").clicked() {
                        state.pending_actions.push(UiAction::StopAudio);
                        state.audio_active = false;
                    }
                } else if ui.button("🔊 Start audio").clicked() {
                    let hint = state
                        .audio_devices
                        .get(state.selected_audio_device)
                        .cloned()
                        .unwrap_or_default();
                    state.pending_actions.push(UiAction::StartAudio { device_hint: hint });
                    state.audio_active = true;
                }
                if ui.button("🔄").on_hover_text("Refresh audio devices").clicked() {
                    state.audio_devices_loaded = false;
                }
            });

            ui.separator();

            // ── Settings ─────────────────────────────────────────────────────
            ui.heading("⚙️ Settings");

            // Resolution — driver-reported if available, fallback to presets.
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
                        // Custom entry at the bottom.
                        ui.separator();
                        if ui.selectable_label(state.custom_resolution, "Custom").clicked() {
                            state.custom_resolution = true;
                        }
                    });
            } else {
                // Still waiting for query, or no device — show static presets.
                const PRESETS: &[(&str, u32, u32, u32)] = &[
                    ("1080p FHD — 1920×1080 @ 60fps", 1920, 1080, 60),
                    ("1440p QHD — 2560×1440 @ 60fps", 2560, 1440, 60),
                    ("4K UHD  — 3840×2160 @ 30fps", 3840, 2160, 30),
                    ("Custom", 0, 0, 0),
                ];
                let preset_idx_ref = &mut state.selected_resolution_idx;
                let preset_label = if state.custom_resolution {
                    "Custom"
                } else {
                    PRESETS.get(*preset_idx_ref).map(|(l, _, _, _)| *l).unwrap_or("1080p FHD")
                };
                egui::ComboBox::from_label("Resolution")
                    .selected_text(preset_label)
                    .show_ui(ui, |ui| {
                        for (i, (label, w, h, fps)) in PRESETS.iter().enumerate() {
                            if ui
                                .selectable_value(preset_idx_ref, i, *label)
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

            // Custom drag values (shown when "Custom" is selected).
            if state.custom_resolution {
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut state.width).range(320..=3840).speed(8.0));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut state.height).range(240..=2160).speed(8.0));
                });
            }

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

            ui.separator();

            // ── Capture controls ─────────────────────────────────────────────
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

            ui.separator();
            ui.label(format!("Frames dropped: {}", state.frames_dropped));
            ui.separator();
            ui.small("Tab: toggle this panel  |  F11: Fullscreen  |  Esc: Quit");
        });
}
