//! Reflection captures draw transparent terrain without taking the gameplay view's sort.

use super::transparent_strafe::{fixture_sized, spawn_view};
use super::*;
use bevy::{
    core_pipeline::core_3d::Transparent3d,
    render::render_phase::{DrawFunctions, ViewSortedRenderPhases},
};
use render_scene::ChunkRenderInstance;

/// A capture view draws every water sub-chunk it sees while the gameplay view keeps the sort.
#[test]
fn reflection_capture_view_queues_its_visible_water() {
    let mut fixture = fixture_sized(2, 3);
    let (capture, capture_retained) = spawn_view(&mut fixture.app);
    let visible = fixture
        .surfaces
        .iter()
        .map(|&(entity, _)| (entity, MainEntity::from(entity)))
        .collect();
    let world = fixture.app.world_mut();
    let mut entity = world.entity_mut(capture);
    entity.insert(render_scene::EnhancedRendering {
        reflection_capture: true,
        ..Default::default()
    });
    entity
        .get_mut::<RenderVisibleEntities>()
        .unwrap()
        .classes
        .insert(
            std::any::TypeId::of::<ChunkRenderInstance>(),
            bevy::render::view::RenderVisibleEntitiesClass {
                entities_cpu_culling: visible,
                ..Default::default()
            },
        );
    world
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .prepare_for_new_frame(capture_retained);
    fixture.frame_looking(Vec3::new(0.0, 80.0, -48.0), Vec3::Z);

    let world = fixture.app.world();
    assert_eq!(
        world.resource::<TransparentSortRuntime>().view_entity,
        Some(fixture.view)
    );
    let direct = world
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawTransparentLiquidDirectCommands>();
    let drawn = world
        .resource::<ViewSortedRenderPhases<Transparent3d>>()
        .get(&capture_retained)
        .unwrap()
        .items
        .values()
        .map(|item| {
            assert_eq!(item.draw_function, direct);
            item.entity.0
        })
        .collect::<BTreeSet<_>>();
    let surfaces = fixture
        .surfaces
        .iter()
        .map(|&(entity, _)| entity)
        .collect::<BTreeSet<_>>();
    assert_eq!(drawn, surfaces);
}
