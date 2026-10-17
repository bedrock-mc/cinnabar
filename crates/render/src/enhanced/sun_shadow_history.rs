//! Receiver-owned sunlight visibility, resolved beside lamp shadows.

use bevy::render::{
    render_resource::*,
    renderer::{RenderContext, RenderDevice, RenderQueue},
};
use render_scene::enhanced::RenderedWorld;
use std::sync::atomic::{AtomicBool, Ordering};

const RESPONSE_SECONDS: f32 = 0.06;

pub(crate) struct SunShadowHistory {
    output: Texture,
    history: Texture,
    pub view: TextureView,
    pub history_view: TextureView,
    pub parameters: Buffer,
    uploaded: Option<[f32; 4]>,
    source: Option<(Option<RenderedWorld>, (u64, u64), TextureViewId)>,
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

    /// Rejects retained visibility when terrain or actor caster inputs change.
    pub fn prepare(
        &mut self,
        queue: &RenderQueue,
        camera_valid: bool,
        world: Option<RenderedWorld>,
        caster_revisions: (u64, u64),
        materials: TextureViewId,
        delta: f32,
    ) {
        let source = (world, caster_revisions, materials);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// World transfers and actor cutout changes reject retained sunlight visibility.
    #[test]
    fn world_and_actor_coverage_changes_reject_sunlight_history() {
        let instance = crate::enhanced::validation::native_instance();
        let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default()))
        else {
            eprintln!("missing fixture: native GPU for sunlight history world binding");
            return;
        };
        let (device, queue) =
            bevy::tasks::block_on(adapter.request_device(&Default::default())).unwrap();
        let queue = RenderQueue::new(queue);
        let mut history = SunShadowHistory::new(&RenderDevice::from(device), [8, 8]);
        let materials = history.view.id();
        let retained = |history: &mut SunShadowHistory, world| {
            history.submitted.store(true, Ordering::Relaxed);
            history.prepare(&queue, true, Some(world), (1, 0), materials, 1.0 / 60.0);
            history.uploaded.unwrap()[0] == 1.0
        };
        let mut frame = render_scene::ActorRenderFrame::default();
        frame.rig.instances = std::sync::Arc::from([render_model::ActorGpuInstance {
            uv_anim: render_model::IDENTITY_UV_ANIM,
            dissolve_multiplier: 1.0,
            ..Default::default()
        }]);
        let mut observe_actor = |frame: &render_scene::ActorRenderFrame| {
            history.submitted.store(true, Ordering::Relaxed);
            history.prepare(
                &queue,
                true,
                None,
                (1, crate::enhanced::actor_signature::frame_signature(frame)),
                materials,
                1.0 / 60.0,
            );
            history.uploaded.unwrap()[0] == 1.0
        };
        assert!(!observe_actor(&frame));
        assert!(observe_actor(&frame));
        std::sync::Arc::make_mut(&mut frame.rig.instances)[0].uv_anim[0] = 0.25;
        assert!(
            !observe_actor(&frame),
            "UV coverage rejects old sunlight visibility"
        );
        assert!(observe_actor(&frame));
        std::sync::Arc::make_mut(&mut frame.rig.instances)[0].dissolve_multiplier = 0.5;
        assert!(
            !observe_actor(&frame),
            "dissolve coverage rejects old sunlight visibility"
        );
        let overworld = RenderedWorld {
            session_generation: 1,
            dimension: 0,
        };
        retained(&mut history, overworld);
        assert!(retained(&mut history, overworld));
        let transferred = RenderedWorld {
            session_generation: 2,
            ..overworld
        };
        assert!(!retained(&mut history, transferred));
        assert!(retained(&mut history, transferred));
        assert!(!retained(
            &mut history,
            RenderedWorld {
                dimension: 1,
                ..transferred
            }
        ));
    }
}
