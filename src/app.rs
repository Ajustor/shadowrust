use crossbeam_channel::Receiver;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

use crate::{
    audio::AudioPassthrough,
    capture::{CaptureConfig, CaptureThread, DeviceResolution},
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
    audio: Option<AudioPassthrough>,
    frame_size: (u32, u32),
    // Resolution query result channel (populated by a background thread).
    resolution_query_rx: Option<Receiver<Vec<DeviceResolution>>>,
    // FPS tracking
    last_frame: Instant,
    fps: f32,
}

impl App {
    pub fn new() -> Self {
        let mut ui_state = UiState::default();
        ui_state.width = 1920;
        ui_state.height = 1080;
        ui_state.fps = 60;
        ui_state.record_path = "capture.mp4".to_string();
        ui_state.menu_visible = true; // visible by default; Tab hides/shows it
        Self {
            state: None,
            ui_state,
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

        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("create window"),
        );

        let renderer =
            pollster::block_on(Renderer::new(Arc::clone(&window))).expect("init wgpu renderer");

        let mut state = RunningState {
            window,
            renderer,
            capture: None,
            frame_rx: None,
            recorder: None,
            audio: None,
            frame_size: (1920, 1080),
            resolution_query_rx: None,
            last_frame: Instant::now(),
            fps: 0.0,
        };

        // Auto-start capture on first available device, like Genki Arcade does.
        let (w, h, fps) = (self.ui_state.width, self.ui_state.height, self.ui_state.fps);
        let devices = crate::capture::list_devices();
        if !devices.is_empty() {
            log::info!("Auto-starting capture on device 0: {}", devices[0]);

            // Kick off a background resolution query immediately so the UI
            // has real format data as soon as possible.
            state.resolution_query_rx = Some(spawn_resolution_query(0));

            let config = CaptureConfig {
                device_index: 0,
                width: w,
                height: h,
                fps,
            };
            match CaptureThread::start(config) {
                Ok((thread, rx)) => {
                    state.frame_size = (w, h);
                    state.frame_rx = Some(rx);
                    state.capture = Some(thread);
                    self.ui_state.capturing = true;
                    self.ui_state.selected_device = 0;

                    match AudioPassthrough::start(None) {
                        Ok(audio) => {
                            state.audio = Some(audio);
                            self.ui_state.audio_active = true;
                        }
                        Err(e) => log::warn!("Auto-start audio failed: {e}"),
                    }
                }
                Err(e) => log::error!("Auto-start capture failed: {e}"),
            }
        } else {
            log::warn!("No capture device found — use the UI to scan and start manually");
        }

        self.state = Some(state);
        log::info!("Window and renderer initialised");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

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
                // ── FPS counter (exponential moving average) ──────────────────
                let now = Instant::now();
                let dt = now.duration_since(state.last_frame).as_secs_f32();
                state.last_frame = now;
                if dt > 0.0 {
                    let instant_fps = 1.0 / dt;
                    state.fps = state.fps * 0.9 + instant_fps * 0.1;
                }
                self.ui_state.fps_display = state.fps;

                // ── Background resolution query result ────────────────────────
                if let Some(rx) = &state.resolution_query_rx {
                    if let Ok(resolutions) = rx.try_recv() {
                        self.ui_state.set_device_resolutions(resolutions);
                        state.resolution_query_rx = None;
                    }
                }

                // ── Video frame ───────────────────────────────────────────────
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

                // ── Render + process UI actions ───────────────────────────────
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

/// Spawn a thread to query the camera's supported resolutions without
/// blocking the render loop. The result arrives on the returned channel.
fn spawn_resolution_query(device_index: usize) -> Receiver<Vec<DeviceResolution>> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("shadowrust-format-query".into())
        .spawn(move || {
            let resolutions = crate::capture::query_device_resolutions(device_index);
            let _ = tx.send(resolutions);
        })
        .ok();
    rx
}

pub enum UiAction {
    StartCapture {
        device_index: usize,
        width: u32,
        height: u32,
        fps: u32,
    },
    StopCapture,
    StartRecording {
        path: String,
    },
    StopRecording,
    StartAudio {
        device_hint: String,
    },
    StopAudio,
    QueryDeviceResolutions {
        device_index: usize,
    },
}

fn handle_action(action: UiAction, state: &mut RunningState) {
    match action {
        UiAction::StartCapture {
            device_index,
            width,
            height,
            fps,
        } => {
            if state.capture.is_some() {
                return;
            }
            let config = CaptureConfig {
                device_index,
                width,
                height,
                fps,
            };
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
            state.audio.take();
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

        UiAction::StartAudio { device_hint } => {
            let hint = if device_hint.is_empty() {
                None
            } else {
                Some(device_hint.as_str())
            };
            match AudioPassthrough::start(hint) {
                Ok(audio) => state.audio = Some(audio),
                Err(e) => log::error!("Failed to start audio: {e}"),
            }
        }

        UiAction::StopAudio => {
            state.audio.take();
            log::info!("Audio stopped");
        }

        UiAction::QueryDeviceResolutions { device_index } => {
            // Only one query at a time; ignore if one is already running.
            if state.resolution_query_rx.is_none() {
                state.resolution_query_rx = Some(spawn_resolution_query(device_index));
            }
        }
    }
}
