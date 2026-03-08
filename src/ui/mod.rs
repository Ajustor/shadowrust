use crate::app::UiAction;
use crate::audio::AudioPassthrough;
use crate::capture::list_devices;

/// Resolution presets — (label, width, height). (0,0) = Custom.
const RESOLUTION_PRESETS: &[(&str, u32, u32)] = &[
    ("1080p FHD — 1920×1080", 1920, 1080),
    ("1440p QHD — 2560×1440", 2560, 1440),
    ("4K UHD  — 3840×2160", 3840, 2160),
    ("Custom", 0, 0),
];

#[derive(Default)]
pub struct UiState {
    pub capturing: bool,
    pub recording: bool,
    pub audio_active: bool,
    pub selected_device: usize,
    pub selected_audio_device: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
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
    // Resolution preset index
    resolution_idx: usize,
}

impl UiState {
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

pub fn draw(ctx: &egui::Context, state: &mut UiState) {
    state.load_video_devices();
    state.load_audio_devices();

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
                let label = state
                    .devices
                    .get(state.selected_device)
                    .cloned()
                    .unwrap_or_default();

                egui::ComboBox::from_id_salt("video-device")
                    .selected_text(&label)
                    .show_ui(ui, |ui| {
                        for (i, name) in state.devices.clone().iter().enumerate() {
                            ui.selectable_value(&mut state.selected_device, i, name);
                        }
                    });

                if ui.button("🔄 Refresh").clicked() {
                    state.devices_loaded = false;
                }
            }

            ui.separator();

            // ── Audio device ─────────────────────────────────────────────────
            ui.heading("�� Audio Device");

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

            ui.separator();

            // ── Settings ─────────────────────────────────────────────────────
            ui.heading("⚙️ Settings");

            // Resolution preset
            let preset_label = RESOLUTION_PRESETS
                .get(state.resolution_idx)
                .map(|(l, _, _)| *l)
                .unwrap_or("1080p FHD — 1920×1080");

            egui::ComboBox::from_label("Resolution")
                .selected_text(preset_label)
                .show_ui(ui, |ui| {
                    for (i, (label, w, h)) in RESOLUTION_PRESETS.iter().enumerate() {
                        if ui
                            .selectable_value(&mut state.resolution_idx, i, *label)
                            .clicked()
                            && *w != 0
                        {
                            state.width = *w;
                            state.height = *h;
                        }
                    }
                });

            // Show drag values only when "Custom" is selected
            let is_custom = RESOLUTION_PRESETS
                .get(state.resolution_idx)
                .map(|(_, w, _)| *w == 0)
                .unwrap_or(false);

            if is_custom {
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
            ui.heading("📊 Stats");
            ui.label(format!("Latency: {:.1} ms", state.latency_ms));
            ui.label(format!("Frames dropped: {}", state.frames_dropped));

            ui.separator();
            ui.small("F11: Fullscreen  |  Esc: Quit");
        });
}
