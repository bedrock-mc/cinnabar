use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn vertex_lists_track_counts_without_a_device() {
    let list = VertexList::new();
    assert_eq!(list.count, 0);
    assert!(list.buffer.is_none() && list.bind_group.is_none());
}

/// Queues the production crack and cutout particle draws beside translucent terrain.
fn overlapping_draws() -> (
    Vec<Transparent3d>,
    RenderPipelineDescriptor,
    DrawFunctionId,
    DrawFunctionId,
) {
    let (mut app, view) = crate::queue_review_support::app();
    app.init_resource::<BlockEntityPipeline>()
        .add_render_command::<Transparent3d, DrawCrackCommands>()
        .add_render_command::<Transparent3d, DrawOverlayCommands>();
    app.world_mut().run_system_once(init_gpu).unwrap();
    let device = app.world().resource::<RenderDevice>();
    let layout = device.create_bind_group_layout("queue fixture", &[]);
    let group = device.create_bind_group("queue fixture", &layout, &[]);
    {
        let mut gpu = app.world_mut().resource_mut::<BlockEntityGpu>();
        gpu.crack.count = 6;
        gpu.crack.bind_group = Some(group);
    }
    app.world_mut().run_system_once(queue_crack).unwrap();
    let (particle_draw, particle_pipeline) = crate::particle_render::tests::queue_opaque(&mut app);
    let terrain_draw = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawOverlayCommands>();
    let mut phase = std::mem::take(
        app.world_mut()
            .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
            .get_mut(&view)
            .unwrap(),
    );
    let crack = phase.items.first().unwrap().1;
    let extra = Transparent3d {
        sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
        entity: crack.entity,
        pipeline: crack.pipeline,
        draw_function: terrain_draw,
        distance: -2.0,
        batch_range: 0..1,
        extra_index: PhaseItemExtraIndex::None,
        indexed: false,
    };
    crate::transparent_phase::add(&mut phase, extra);
    phase.sort();
    let items = phase.items.into_values().collect();
    (items, particle_pipeline, particle_draw, terrain_draw)
}

#[test]
fn mining_particles_occlude_cracks_and_cracks_remain_on_translucent_targets() {
    let (items, pipeline, particle_draw, terrain_draw) = overlapping_draws();
    let depth = pipeline.depth_stencil.unwrap();
    assert_eq!(depth.depth_compare, Some(CompareFunction::GreaterEqual));
    assert!(
        pipeline.fragment.unwrap().targets[0]
            .as_ref()
            .unwrap()
            .blend
            .is_none()
    );
    let particle = [0.7, 0.4, 0.2];
    for particle_depth in [0.3, 0.7] {
        let mut pixel = [0.6, 0.6, 0.6];
        let mut z = 0.0;
        for item in &items {
            let fragment_z = if item.draw_function == particle_draw {
                particle_depth
            } else if item.draw_function == terrain_draw {
                0.4
            } else {
                0.41
            };
            if fragment_z < z {
                continue;
            }
            if item.draw_function == particle_draw {
                pixel = particle;
                if depth.depth_write_enabled == Some(true) {
                    z = fragment_z;
                }
            } else if item.draw_function == terrain_draw {
                pixel = pixel.map(|channel| 0.5 * 0.6 + 0.5 * channel);
            } else {
                pixel = [0.0; 3];
            }
        }
        assert_eq!(
            pixel,
            if particle_depth > 0.4 {
                particle
            } else {
                [0.0; 3]
            }
        );
    }
}
