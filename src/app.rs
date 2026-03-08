
use crossbeam_channel::Receiver;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

use crate::{
    capture::{CaptureConfig, CaptureThread},
    record::Recorder,
    render::Renderer,
    ui::UiState,
};

pub struct App {
    state: Option<RunningState>,
    ui_state: UiState,
}

struct RunningState {
    window: Arc<Window>,
    renderer: Renderer,
    capture: Option<CaptureThread>,
    frame_rx: Option<Receiver<Vec<u8>>>,
    recorder: Option<Recorder>,
    frame_size: (u32, u32),
}

impl App {
    pub fn new() -> Self {
        Self {
            state: None,
            ui_state: UiState::default(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window_attrs = Window::default_attributes()
            .with_title("ShadowRust — Genki ShadowCast 2")
            .with_inner_size(winit::dpi::LogicalSize::new(1920u32, 1080u32));

        let window = Arc::new(event_loop.create_window(window_attrs).expect("create window"));

        let renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))
            .expect("init wgpu renderer");

        self.state = Some(RunningState {
            window,
            renderer,
            capture: None,
            frame_rx: None,
            recorder: None,
            frame_size: (1920, 1080),
        });

        log::info!("Window and renderer initialised");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else { return };

        let consumed = state.renderer.handle_window_event(&event);
        if consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Close requested");
                event_loop.exit();
            }

            WindowEvent::KeyboardInput { event: ref key, .. } => {
                use winit::keyboard::{KeyCode, PhysicalKey};
                if key.state == winit::event::ElementState::Pressed {
                    match key.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                        PhysicalKey::Code(KeyCode::F11) => {
                            let is_full = state.window.fullscreen().is_some();
                            state.window.set_fullscreen(if is_full {
                                None
                            } else {
                                Some(winit::window::Fullscreen::Borderless(None))
                            });
                        }
                        _ => {}
                    }
                }
            }

            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
            }

            WindowEvent::RedrawRequested => {
                // Drain capture channel to latest frame only
                if let Some(rx) = &state.frame_rx {
                    let mut latest = None;
                    while let Ok(frame) = rx.try_recv() {
                        latest = Some(frame);
                    }
                    if let Some(frame) = latest {
                        if let Some(rec) = &mut state.recorder {
                            rec.push_frame(&frame, state.frame_size);
                        }
                        state.renderer.update_frame(&frame, state.frame_size);
                    }
                }

                // Render + collect UI actions (no self borrow conflict: handle_action is a free fn)
                let actions = state.renderer.render(&mut self.ui_state);
                for action in actions {
                    handle_action(action, state);
                }

                state.window.request_redraw();
            }

            _ => {}
        }
    }
}

pub enum UiAction {
    StartCapture { device_index: usize, width: u32, height: u32, fps: u32 },
    StopCapture,
    StartRecording { path: String },
    StopRecording,
}

fn handle_action(action: UiAction, state: &mut RunningState) {
    match action {
        UiAction::StartCapture { device_index, width, height, fps } => {
            if state.capture.is_some() {
                return;
            }
            let config = CaptureConfig { device_index, width, height, fps };
            match CaptureThread::start(config) {
                Ok((thread, rx)) => {
                    state.frame_size = (width, height);
                    state.frame_rx = Some(rx);
                    state.capture = Some(thread);
                    log::info!("Capture started: {width}x{height}@{fps}");
                }
                Err(e) => log::error!("Failed to start capture: {e}"),
            }
        }

        UiAction::StopCapture => {
            state.capture.take();
            state.frame_rx.take();
            log::info!("Capture stopped");
        }

        UiAction::StartRecording { path } => {
            let (w, h) = state.frame_size;
            match Recorder::new(&path, w, h, 60) {
                Ok(rec) => {
                    state.recorder = Some(rec);
                    log::info!("Recording → {path}");
                }
                Err(e) => log::error!("Failed to start recording: {e}"),
            }
        }

        UiAction::StopRecording => {
            if let Some(rec) = state.recorder.take() {
                if let Err(e) = rec.finish() {
                    log::error!("Failed to finalise recording: {e}");
                } else {
                    log::info!("Recording saved");
                }
            }
        }
    }
}
