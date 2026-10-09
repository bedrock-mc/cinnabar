use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::render::render_phase::DrawFunctionId;

/// Queues a depth-writing particle through the production upload and queue systems.
pub(crate) fn queue_opaque(app: &mut App) -> (DrawFunctionId, RenderPipelineDescriptor) {
    app.init_resource::<ParticlePipeline>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Opaque as u8 }>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Blend as u8 }>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Add as u8 }>>();
    app.world_mut().run_system_once(init_particle_gpu).unwrap();
    let mut frame = ParticleGpuFrame::default();
    frame.set_lists(
        DrawLists {
            opaque: vec![ParticleInstance::default()],
            ..Default::default()
        },
        [0.0; 3],
    );
    app.insert_resource(frame);
    app.world_mut()
        .run_system_once(prepare_particle_resources)
        .unwrap();
    app.world_mut().run_system_once(queue_particles).unwrap();
    let draw = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawParticles<{ ParticleMode::Opaque as u8 }>>();
    let id = app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>()
        .values()
        .flat_map(|phase| phase.items.iter())
        .find(|item| item.draw_function == draw)
        .unwrap()
        .pipeline;
    let mut cache = app.world_mut().resource_mut::<PipelineCache>();
    (
        draw,
        crate::queue_review_support::queued_descriptor(&mut cache, id).clone(),
    )
}

#[test]
fn blended_and_additive_particles_keep_their_translucent_pipeline() {
    let (mut app, _) = crate::queue_review_support::app();
    queue_opaque(&mut app);
    for material in [ParticleMode::Blend, ParticleMode::Add] {
        let world = app.world_mut();
        let id = world.resource_scope(|world, mut pipeline: Mut<ParticlePipeline>| {
            pipeline
                .variants
                .specialize(
                    world.resource::<PipelineCache>(),
                    ParticlePipelineKey {
                        msaa: Msaa::Off,
                        hdr: false,
                        material,
                    },
                )
                .unwrap()
        });
        let mut cache = world.resource_mut::<PipelineCache>();
        let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
        assert_eq!(
            descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .write_mask,
            ColorWrites::COLOR
        );
        assert!(
            !descriptor
                .depth_stencil
                .as_ref()
                .unwrap()
                .depth_write_enabled
        );
        assert!(
            descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
                .is_some()
        );
    }
}
