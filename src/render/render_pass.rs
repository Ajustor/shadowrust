use winit::event::WindowEvent;

use crate::app::UiAction;
use crate::ui::UiState;

use super::Renderer;

impl Renderer {
    pub fn handle_window_event(&mut self, event: &WindowEvent) -> bool {
        self.egui_state
            .on_window_event(&self.window, event)
            .consumed
    }

    pub fn render(&mut self, ui_state: &mut UiState) -> Vec<UiAction> {
        let output = match self.surface.get_current_texture() {
            Ok(o) => o,
            Err(e) => {
                log::warn!("Surface error: {e:?}");
                self.surface.configure(&self.device, &self.config);
                return vec![];
            }
        };
        let view = output.texture.create_view(&Default::default());

        // --- egui CPU frame ---
        let raw_input = self.egui_state.take_egui_input(&self.window);
        let egui_ctx = self.egui_state.egui_ctx().clone();
        let full_output = egui_ctx.run(raw_input, |ctx| crate::ui::draw(ctx, ui_state));
        self.egui_state
            .handle_platform_output(&self.window, full_output.platform_output);
        let primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }

        // --- GPU: single encoder for the whole frame ---
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame-enc"),
            });

        // Video pass
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let Some(bg) = &self.bind_group {
                rpass.set_pipeline(&self.pipeline);
                rpass.set_bind_group(0, bg, &[]);
                rpass.draw(0..4, 0..1);
            }
        }

        // egui buffer upload
        let extra_cmds = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &primitives,
            &screen,
        );

        // egui render pass
        {
            let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.egui_renderer
                .render(&mut rpass.forget_lifetime(), &primitives, &screen);
        }

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.queue
            .submit(extra_cmds.into_iter().chain([encoder.finish()]));
        output.present();

        std::mem::take(&mut ui_state.pending_actions)
    }
}
