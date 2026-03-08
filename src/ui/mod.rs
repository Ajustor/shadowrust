use crate::app::UiAction;
use crate::capture::list_devices;

#[derive(Default)]
pub struct UiState {
    pub capturing: bool,
    pub recording: bool,
    pub selected_device: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub record_path: String,
    pub latency_ms: f32,
    pub frames_dropped: u64,
    pub pending_actions: Vec<UiAction>,
    devices: Vec<String>,
    devices_loaded: bool,
}

impl UiState {
    fn ensure_defaults(&mut self) {
        if self.width == 0 {
            self.width = 1920;
        }
        if self.height == 0 {
            self.height = 1080;
        }
        if self.fps == 0 {
            self.fps = 60;
        }
        if self.record_path.is_empty() {
            self.record_path = "capture.mp4".to_string();
        }
    }

    fn load_devices(&mut self) {
        if !self.devices_loaded {
            self.devices = list_devices();
            self.devices_loaded = true;
        }
    }
}

pub fn draw(ctx: &egui::Context, state: &mut UiState) {
    state.ensure_defaults();
    state.load_devices();

    egui::Window::new("ShadowRust")
        .default_pos([16.0, 16.0])
        .resizable(false)
        .show(ctx, |ui| {
            ui.heading("📹 Capture Device");

            // Device selector
            let devices = state.devices.clone();
            let selected = state.selected_device;
            let label = devices
                .get(selected)
                .cloned()
                .unwrap_or_else(|| "No device found".to_string());

            egui::ComboBox::from_label("Device")
                .selected_text(&label)
                .show_ui(ui, |ui| {
                    for (i, name) in devices.iter().enumerate() {
                        ui.selectable_value(&mut state.selected_device, i, name);
                    }
                });

            if ui.button("🔄 Refresh devices").clicked() {
                state.devices_loaded = false;
            }

            ui.separator();
            ui.heading("⚙️ Settings");

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
                egui::ComboBox::from_label("")
                    .selected_text(state.fps.to_string())
                    .show_ui(ui, |ui| {
                        for &f in &[30u32, 60, 120] {
                            ui.selectable_value(&mut state.fps, f, f.to_string());
                        }
                    });
            });

            ui.separator();

            // Capture controls
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
