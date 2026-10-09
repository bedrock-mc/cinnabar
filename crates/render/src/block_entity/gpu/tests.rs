use super::*;
use bevy::{ecs::system::RunSystemOnce, render::render_phase::SortedPhaseItem};

#[test]
fn vertex_lists_track_counts_without_a_device() {
    let list = VertexList::new();
    assert_eq!(list.count, 0);
    assert!(list.buffer.is_none() && list.bind_group.is_none());
}

/// Queues a real crack draw beside a foreground alpha-blended draw.
fn overlapping_draws(distance: f32) -> (Vec<Transparent3d>, BlendState, DrawFunctionId) {
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
    let particle_draw = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawOverlayCommands>();
    let mut items = std::mem::take(
        &mut app
            .world_mut()
            .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
            .get_mut(&view)
            .unwrap()
            .items,
    );
    let crack = &items[0];
    let pipeline = crack.pipeline;
    items.push(Transparent3d {
        entity: crack.entity,
        pipeline,
        draw_function: particle_draw,
        distance,
        batch_range: 0..1,
        extra_index: PhaseItemExtraIndex::None,
        indexed: false,
    });
    Transparent3d::sort(&mut items);
    let mut cache = app.world_mut().resource_mut::<PipelineCache>();
    let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, pipeline);
    let blend = descriptor.fragment.as_ref().unwrap().targets[0]
        .as_ref()
        .unwrap()
        .blend
        .unwrap();
    (items, blend, particle_draw)
}

/// Evaluates the configured overlay blend for an opaque black crack texel.
fn crack_pixel(destination: [f32; 3], blend: BlendState) -> [f32; 3] {
    assert_eq!(blend.color.src_factor, BlendFactor::Dst);
    assert_eq!(blend.color.dst_factor, BlendFactor::Src);
    assert_eq!(blend.color.operation, BlendOperation::Add);
    destination.map(|channel| 0.0 * channel + channel * 0.0)
}

#[test]
fn foreground_particles_cover_cracks_without_being_darkened_by_them() {
    let particle = [0.7, 0.4, 0.2];
    for distance in [-0.01, -2.0, -128.0] {
        let (items, crack_blend, particle_draw) = overlapping_draws(distance);
        for alpha in [1.0, 0.5] {
            let mut pixel = [0.6, 0.6, 0.6];
            for item in &items {
                pixel = if item.draw_function == particle_draw {
                    std::array::from_fn(|axis| particle[axis] * alpha + pixel[axis] * (1.0 - alpha))
                } else {
                    crack_pixel(pixel, crack_blend)
                };
            }
            assert_eq!(
                pixel,
                particle.map(|channel| channel * alpha),
                "foreground particles must cover the already cracked block at {distance}"
            );
        }
    }
}
