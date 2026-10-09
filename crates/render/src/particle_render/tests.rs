use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::render::render_phase::DrawFunctionId;
use bevy::render::render_phase::SortedPhaseItem;

/// Uploads and queues all material lists through the production systems.
fn queue_lists(app: &mut App, lists: DrawLists, camera: [f32; 3]) {
    app.init_resource::<ParticlePipeline>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Opaque as u8 }>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Blend as u8 }>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Add as u8 }>>();
    app.world_mut().run_system_once(init_particle_gpu).unwrap();
    let mut frame = ParticleGpuFrame::default();
    frame.set_lists(lists, camera);
    app.insert_resource(frame);
    app.world_mut()
        .run_system_once(prepare_particle_resources)
        .unwrap();
    app.world_mut().run_system_once(queue_particles).unwrap();
}

/// Queues a depth-writing particle through the production upload and queue systems.
pub(crate) fn queue_opaque(app: &mut App) -> (DrawFunctionId, RenderPipelineDescriptor) {
    queue_lists(
        app,
        DrawLists {
            opaque: vec![ParticleInstance::default()],
            ..Default::default()
        },
        [0.0; 3],
    );
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
fn additive_particles_compose_at_their_depth_with_only_cutout_particles_present() {
    for distance in [-10.0, -2.0] {
        let (mut app, view) = crate::queue_review_support::app();
        queue_lists(
            &mut app,
            DrawLists {
                opaque: vec![ParticleInstance {
                    center_light: [100.0, 0.0, -1.0, 0.0],
                    ..Default::default()
                }],
                add: vec![ParticleInstance {
                    center_light: [0.0, 0.0, distance, 0.0],
                    ..Default::default()
                }],
                ..Default::default()
            },
            [0.0; 3],
        );
        let functions = app
            .world()
            .resource::<DrawFunctions<Transparent3d>>()
            .read();
        let add = functions.id::<DrawParticles<{ ParticleMode::Add as u8 }>>();
        let terrain = functions.id::<DrawParticles<{ ParticleMode::Blend as u8 }>>();
        drop(functions);
        let mut items = std::mem::take(
            &mut app
                .world_mut()
                .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
                .get_mut(&view)
                .unwrap()
                .items,
        );
        let item = items.iter().find(|item| item.draw_function == add).unwrap();
        items.push(Transparent3d {
            entity: item.entity,
            pipeline: item.pipeline,
            draw_function: terrain,
            distance: -5.0,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
        Transparent3d::sort(&mut items);
        let mut pixel = 0.2;
        let mut depth = 0.0;
        for item in items {
            if item.draw_function == terrain {
                pixel = 0.1 * 0.5 + pixel * 0.5;
                depth = 0.5;
            } else if item.draw_function == add {
                let particle_depth = if distance < -5.0 { 0.25 } else { 0.75 };
                if particle_depth >= depth {
                    pixel += 0.6;
                }
            }
        }
        let expected: f32 = if distance < -5.0 { 0.45 } else { 0.75 };
        assert!(
            (pixel - expected).abs() < 0.0001,
            "additive color must compose through translucent terrain at its own depth"
        );
    }
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
