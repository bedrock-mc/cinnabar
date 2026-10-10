//! Receiver-owned sunlight visibility, resolved beside lamp shadows.

use bevy::render::{
    render_resource::*,
    renderer::{RenderContext, RenderDevice, RenderQueue},
};
use std::sync::atomic::{AtomicBool, Ordering};

const RESPONSE_SECONDS: f32 = 0.06;

pub(crate) struct SunShadowHistory {
    output: Texture,
    history: Texture,
    pub view: TextureView,
    pub history_view: TextureView,
    pub parameters: Buffer,
    uploaded: Option<[f32; 4]>,
    source: Option<(Option<i32>, u64, TextureViewId)>,
    submitted: AtomicBool,
}

impl SunShadowHistory {
    pub fn new(device: &RenderDevice, size: [u32; 2]) -> Self {
        let create = |label, copy| {
            device.create_texture(&TextureDescriptor {
                label: Some(label),
                size: Extent3d {
                    width: size[0].max(1),
                    height: size[1].max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba16Float,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | copy,
                view_formats: &[],
            })
        };
        let output = create("resolved sunlight visibility", TextureUsages::COPY_SRC);
        let history = create("previous sunlight visibility", TextureUsages::COPY_DST);
        let view = output.create_view(&Default::default());
        let history_view = history.create_view(&Default::default());
        Self {
            output,
            history,
            view,
            history_view,
            parameters: device.create_buffer(&BufferDescriptor {
                label: Some("sunlight visibility history policy"),
                size: 16,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            uploaded: None,
            source: None,
            submitted: AtomicBool::new(false),
        }
    }

    pub fn prepare(
        &mut self,
        queue: &RenderQueue,
        camera_valid: bool,
        dimension: Option<i32>,
        geometry_revision: u64,
        materials: TextureViewId,
        delta: f32,
    ) {
        let source = (dimension, geometry_revision, materials);
        let valid = self.submitted.swap(false, Ordering::Relaxed)
            && camera_valid
            && self.source == Some(source);
        self.source = Some(source);
        let update = if delta.is_finite() && delta > 0.0 {
            1.0 - (-delta / RESPONSE_SECONDS).exp()
        } else {
            1.0
        };
        let parameters = [f32::from(u8::from(valid)), update, 0.0, 0.0];
        if self.uploaded != Some(parameters) {
            queue.write_buffer(&self.parameters, 0, bytemuck::bytes_of(&parameters));
            self.uploaded = Some(parameters);
        }
    }

    pub fn store(&self, context: &mut RenderContext) {
        context.command_encoder().copy_texture_to_texture(
            self.output.as_image_copy(),
            self.history.as_image_copy(),
            self.output.size(),
        );
        self.submitted.store(true, Ordering::Relaxed);
    }
}
