//! Cross-crate observations of the production actor preparation systems.
use super::*;
use bevy::{
    diagnostic::FrameCount,
    ecs::system::RunSystemOnce,
    prelude::{GlobalTransform, Mat4},
};
use render_model::{ActorDrawManifestEntry, ActorTexturePage};

/// Retains production artwork state across benchmark frames.
#[derive(Default)]
pub struct ActorArtworkBenchmark(super::artwork::GpuArtwork);

impl ActorArtworkBenchmark {
    /// Prepares the pages requested by a synthetic frame through the normal demand path.
    pub fn prepare(
        &mut self,
        pages: &render_model::ActorArtworkPages,
        demanded: &[render_model::ActorArtworkPageId],
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        self.0.prepare_demanded(pages, demanded, device, queue)
    }
}

/// Prepared draw spans and the immutable artwork they sample.
#[derive(Debug, PartialEq, Eq)]
pub struct PublicationActorPreparation {
    pub artwork_identity: [u8; 32],
    pub instances: u32,
    pub geometry_ready: bool,
    pub artwork_current: bool,
    pub spans: Vec<(render_model::ActorArtworkPageId, std::ops::Range<u32>)>,
}

/// Reads the render world's actual prepared resources without creating or repairing them.
pub fn publication_actor_preparation(app: &App) -> PublicationActorPreparation {
    let gpu = app.sub_app(RenderApp).world().resource::<ActorGpu>();
    PublicationActorPreparation {
        artwork_identity: gpu.artwork_identity,
        instances: gpu.instance_count,
        geometry_ready: gpu.geometry_ready,
        artwork_current: gpu.artwork_current,
        spans: gpu
            .spans
            .iter()
            .map(|span| (span.page, span.first..span.first + span.count))
            .collect(),
    }
}

/// One prepared instance and the artwork binding its draw span selects.
#[doc(hidden)]
#[derive(Debug)]
pub struct PreparedActorDraw {
    pub entry: ActorDrawManifestEntry,
    pub page: render_model::ActorArtworkPageId,
    pub vertex_count: u32,
    pub artwork_bound: bool,
}

/// Runs real uploads, bone residency, span construction and bindings on a validation device.
#[doc(hidden)]
pub fn prepare_actor_test_frame(frame: ActorRenderFrame) -> Vec<PreparedActorDraw> {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..Default::default()
        },
        ..Default::default()
    });
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let mut world = World::new();
    world.insert_resource(PipelineCache::new(device.clone(), false));
    world.insert_resource(device);
    world.insert_resource(queue);
    world.insert_resource(frame);
    world.init_resource::<ActorPipeline>();
    world.init_resource::<ActorRuntimeWitness>();
    world.init_resource::<ActorPresentationGate>();
    world.init_resource::<ActorDrawTracker>();
    world.init_resource::<ViewUniforms>();
    world.init_resource::<FrameCount>();
    let view = world.spawn_empty().id();
    world
        .entity_mut(view)
        .insert(bevy::render::view::ExtractedView {
            retained_view_entity: bevy::render::view::RetainedViewEntity::new(view.into(), None, 0),
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::IDENTITY,
            clip_from_world: None,
            target_format: render_scene::SCENE_COLOR_FORMAT,
            viewport: bevy::math::UVec4::new(0, 0, 1, 1),
            color_grading: default(),
            invert_culling: false,
        });
    world.run_system_once(init_actor_gpu).unwrap();
    world
        .run_system_once(bevy::render::view::prepare_view_uniforms)
        .unwrap();
    world.run_system_once(prepare_actor_resources).unwrap();
    world.run_system_once(prepare_actor_bind_group).unwrap();
    let gpu = world.resource::<ActorGpu>();
    let frame = world.resource::<ActorRenderFrame>();
    gpu.spans
        .iter()
        .flat_map(|span| {
            let artwork_bound = if span.page == 0 {
                gpu.bind_group.is_some()
            } else {
                gpu.artwork_current
                    && frame
                        .artwork
                        .pages
                        .get(usize::from(span.page) - 1)
                        .is_some_and(|page: &ActorTexturePage| {
                            gpu.artwork.page_is_bound(span.page, page)
                        })
            };
            (span.first..span.first + span.count).map(move |index| PreparedActorDraw {
                entry: gpu.manifest[index as usize].clone(),
                page: span.page,
                vertex_count: span.vertex_count,
                artwork_bound,
            })
        })
        .collect()
}
