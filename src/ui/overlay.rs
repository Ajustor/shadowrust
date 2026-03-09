use super::state::UiState;

/// Draw the always-visible FPS / hint overlay in the top-right corner.
pub(super) fn draw_overlay(ctx: &egui::Context, state: &UiState) {
    egui::Window::new("##fps")
        .title_bar(false)
        .resizable(false)
        .movable(false)
        .anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0])
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::from_black_alpha(140))
                .inner_margin(6.0),
        )
        .show(ctx, |ui| {
            if state.capturing {
                ui.colored_label(
                    egui::Color32::from_rgb(180, 255, 180),
                    format!("{:.1} FPS (capture)", state.fps_display),
                );
            } else {
                ui.colored_label(egui::Color32::GRAY, "No capture");
            }
            if !state.menu_visible {
                ui.small(egui::RichText::new("Tab — show settings").color(egui::Color32::GRAY));
            }
        });
}
