mod frame;
mod render_pass;
mod setup;

use std::sync::Arc;
use winit::window::Window;

pub struct Renderer {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) config: wgpu::SurfaceConfiguration,
    pub(crate) pipeline: wgpu::RenderPipeline,
    pub(crate) bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) sampler: wgpu::Sampler,

    pub(crate) texture: Option<wgpu::Texture>,
    pub(crate) bind_group: Option<wgpu::BindGroup>,
    pub(crate) frame_size: (u32, u32),

    pub(crate) egui_renderer: egui_wgpu::Renderer,
    pub(crate) egui_state: egui_winit::State,
    pub(crate) window: Arc<Window>,
}
