// Hide the terminal window in release builds on Windows.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod capture;
mod decode;
mod record;
mod render;
mod ui;

use anyhow::Result;
use app::App;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("ShadowRust starting…");

    // macOS requires requesting camera permission before any AVFoundation call.
    #[cfg(target_os = "macos")]
    {
        nokhwa::nokhwa_initialize(|granted| {
            if granted {
                log::info!("Camera permission granted");
            } else {
                log::error!("Camera permission denied — video capture will not work");
            }
        });
        // Give AVFoundation a moment to process the permission grant.
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    let event_loop = winit::event_loop::EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App::new();
    event_loop.run_app(&mut app)?;

    Ok(())
}
