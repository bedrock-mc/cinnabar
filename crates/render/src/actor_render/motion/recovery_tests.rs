use super::*;
use bevy::{
    math::{Mat4, UVec4},
    render::texture::CachedTexture,
};
use std::sync::Arc;

struct Fixture {
    device: RenderDevice,
    queue: RenderQueue,
    cache: PipelineCache,
    pipeline: ActorPipeline,
    gpu: ActorGpu,
    frame: ActorRenderFrame,
    state: Option<MotionView>,
}

impl Fixture {
    /// Supplies a valid actor publication and real bindings on a no-op GPU.
    fn new() -> Self {
        let (mut app, _) = crate::queue_review_support::app();
        let world = app.world_mut();
        world.init_resource::<ActorPipeline>();
        let device = world.resource::<RenderDevice>().clone();
        let queue = world.resource::<RenderQueue>().clone();
        let mut gpu = super::super::create_actor_gpu(&device, &queue).unwrap();
        let mut frame = ActorRenderFrame::default();
        frame.rig = crate::motion_blur::actor_history::tests::frame(Vec3::ZERO, [0.0, 1.0, 1.0]);
        gpu.spans.push(crate::actor::gpu::ActorDrawSpan {
            material: assets::EntityRenderMaterial::Default as u32,
            page: 0,
            first: 0,
            count: 1,
            vertex_count: 3,
        });
        Self {
            device,
            queue,
            gpu,
            frame,
            state: None,
            cache: world.remove_resource::<PipelineCache>().unwrap(),
            pipeline: world.remove_resource::<ActorPipeline>().unwrap(),
        }
    }

    /// Runs the production upload and binding preparation against the current publication.
    fn prepare(&mut self, depth: &ViewDepthStencilTexture, continuous: bool) {
        self.gpu.instance_count = self.frame.rig.instances.len() as u32;
        self.gpu.instances = Arc::clone(&self.frame.rig.instances);
        self.gpu.frame_generation = self.frame.rig.frame_generation;
        let camera =
            CameraHistory::new(Mat4::IDENTITY, Mat4::IDENTITY, UVec4::new(0, 0, 16, 16), 0);
        prepare(
            &mut self.state,
            Some(&self.frame),
            Some(&self.gpu),
            Some(&self.pipeline),
            camera,
            camera,
            continuous,
            depth,
            false,
            false,
            &self.device,
            &self.queue,
            &self.cache,
        );
    }
}

/// Matches the scene depth attachment whose sample count controls downstream motion layouts.
fn depth(device: &RenderDevice, samples: u32) -> ViewDepthStencilTexture {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("motion recovery depth"),
        size: Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    ViewDepthStencilTexture::new(
        CachedTexture {
            default_view: texture.create_view(&Default::default()),
            texture,
        },
        Some(0.0),
        None,
    )
}

#[test]
fn failed_msaa_replacement_discards_incompatible_motion_view() {
    for (old_samples, new_samples) in [(1, 4), (4, 1)] {
        let mut fixture = Fixture::new();
        fixture.prepare(&depth(&fixture.device, old_samples), false);
        assert_eq!(
            fixture.state.as_ref().unwrap().texture.sample_count(),
            old_samples
        );
        let _failure = allocation::inject(|label, _| label == "rendered actor velocities");
        fixture.prepare(&depth(&fixture.device, new_samples), true);
        assert!(
            fixture.state.is_none(),
            "an incompatible texture must not reach downstream resolve bindings"
        );
    }
}
