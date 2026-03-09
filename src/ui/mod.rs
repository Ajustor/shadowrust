mod helpers;
mod overlay;
mod settings;
mod state;
mod update_banner;

pub use state::UiState;

use crate::updater::UpdateChecker;

pub fn draw(ctx: &egui::Context, state: &mut UiState) {
    // Tab key handled here so it works even when egui has keyboard focus.
    if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
        state.menu_visible = !state.menu_visible;
    }

    state.load_video_devices();
    state.load_audio_devices();

    // ── Start update checker on first draw ────────────────────────────────────
    if state.update_checker.is_none() {
        state.update_checker = Some(UpdateChecker::start());
    }

    // ── Update notification banner ────────────────────────────────────────────
    update_banner::draw_update_banner(ctx, state);

    // ── Always-visible FPS / hint overlay ────────────────────────────────────
    overlay::draw_overlay(ctx, state);

    if !state.menu_visible {
        return;
    }

    // ── Settings panel ────────────────────────────────────────────────────────
    settings::draw_settings(ctx, state);
}
