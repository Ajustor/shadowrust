use crate::updater::UpdateStatus;

use super::state::UiState;

/// Draw the update notification banner at the top of the screen.
pub(super) fn draw_update_banner(ctx: &egui::Context, state: &mut UiState) {
    if state.update_dismissed {
        return;
    }
    let Some(checker) = &state.update_checker else {
        return;
    };
    if let UpdateStatus::Available { version, url } = checker.get() {
        egui::Window::new("##update")
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 8.0])
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(30, 70, 30))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 200, 80)))
                    .inner_margin(10.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(120, 255, 120),
                        format!("🆕 ShadowRust v{version} is available!"),
                    );
                    if ui.small_button("⬇ Download").clicked() {
                        super::helpers::open_url(&url);
                    }
                    if ui.small_button("✕").clicked() {
                        state.update_dismissed = true;
                    }
                });
            });
    }
}
