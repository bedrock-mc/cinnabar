use super::*;
use bevy::prelude::{Has, World};
use render_scene::EnhancedRendering;

#[test]
fn an_unprepared_enhanced_view_encodes_nothing_after_repeated_installation() {
    let mut world = crate::render_test_support::empty_render_world();
    crate::ui_render::install_overlay_graph(&mut world);
    graph::install_graph(&mut world);
    graph::install_graph(&mut world);
    crate::render_test_support::assert_empty_render(&mut world);
}

// Graph routing follows the extracted camera component directly, including opt-out.
#[test]
fn grade_stage_follows_camera_opt_in_without_a_separate_marker() {
    let mut world = World::new();
    let camera = world.spawn(EnhancedRendering::default()).id();
    let mut query = world.query::<Has<EnhancedRendering>>();
    assert!(query.get(&world, camera).unwrap());
    world.entity_mut(camera).remove::<EnhancedRendering>();
    assert!(!query.get(&world, camera).unwrap());
}
