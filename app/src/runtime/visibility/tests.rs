use super::*;

/// An actor is hidden only when every sub-chunk its box touches is known and not visible.
#[test]
fn a_box_is_hidden_only_when_all_its_known_sub_chunks_are_invisible() {
    let key = |x, y, z| SubChunkKey::new(0, x, y, z);
    let cache = CaveVisibilityCache {
        camera: Some(key(0, 4, 0)),
        graph_generation: Some(7),
        visible: [key(1, 4, 0)].into_iter().collect(),
        initialized: true,
        ..CaveVisibilityCache::default()
    };
    let known = |key: SubChunkKey| key.y < 8;
    let hides = |low, high| cache.hides_box(key(0, 4, 0), 7, known, low, high);
    assert!(hides([-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
    // Straddling into the visible neighbour, or reaching an unknown sub-chunk, draws it.
    assert!(!hides([15.5, 64.0, 4.0], [16.5, 66.0, 5.0]));
    assert!(!hides([-8.0, 127.0, 4.0], [-7.0, 129.0, 5.0]));
    // A stale graph or another dimension never hides anything.
    assert!(!cache.hides_box(key(0, 4, 0), 8, known, [-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
    assert!(!cache.hides_box(
        SubChunkKey::new(1, 0, 4, 0),
        7,
        known,
        [-8.0, 64.0, 4.0],
        [-7.0, 66.0, 5.0]
    ));
}

#[test]
fn actor_frustum_crossing_a_camera_cell_never_uses_the_previous_cave_result() {
    use bevy::math::{Mat4, Vec3};

    let before = Vec3::new(15.9, 65.0, 3.0);
    let after = Vec3::new(16.1, 65.0, 3.0);
    let camera_key = |position| camera_sub_chunk_key(0, position);
    let mut cache = CaveVisibilityCache {
        camera: Some(camera_key(before)),
        graph_generation: Some(7),
        visible: [camera_key(before)].into_iter().collect(),
        initialized: true,
        ..CaveVisibilityCache::default()
    };
    let bounds = assets::SkinGeometryBounds::default();
    let feet = [17.0, 64.0, 4.0];
    let (low, high) = bounds.at(feet, 1.0);
    let view = render::ActorCullView {
        clip_from_world: Mat4::perspective_infinite_reverse_rh(
            std::f32::consts::FRAC_PI_2,
            1.0,
            0.1,
        ) * Mat4::look_at_rh(after, Vec3::from_array(feet) + Vec3::Y, Vec3::Y),
        camera_position: after,
        max_distance: render::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    };
    assert!(render::actor_bounds_are_visible(
        feet,
        1.0,
        bounds,
        Some(view)
    ));
    assert!(cache.hides_box(camera_key(before), 7, |_| true, low, high));
    assert!(
        !cache.hides_box(camera_key(view.camera_position), 7, |_| true, low, high),
        "a drawable actor must survive until the new camera's cave result is ready"
    );
    cache.camera = Some(camera_key(after));
    cache.visible = [camera_key(after)].into_iter().collect();
    assert!(!cache.hides_box(camera_key(view.camera_position), 7, |_| true, low, high));
    assert!(cache.hides_box(
        camera_key(view.camera_position),
        7,
        |_| true,
        [-2.0, 64.0, 4.0],
        [-1.0, 66.0, 5.0]
    ));
}

/// Only entities whose key entered or left the visible set are written.
#[test]
fn publishing_touches_only_entities_whose_visibility_flipped() {
    let key = |x| SubChunkKey::new(0, x, 0, 0);
    let entity = |x| Entity::from_raw_u32(x as u32 + 1).unwrap();
    let mut cache = CaveVisibilityCache {
        rendered: (0..4).map(|x| (key(x), entity(x))).collect(),
        visible_rendered: 4,
        ..CaveVisibilityCache::default()
    };
    let mut writes = Vec::new();
    cache.next_visible = [key(0), key(1), key(9)].into_iter().collect();
    cache.publish_next(|entity, visible| writes.push((entity, visible)));
    writes.sort_by_key(|(entity, _)| entity.index());
    assert_eq!(writes, [(entity(2), false), (entity(3), false)]);
    assert_eq!(cache.visible_rendered, 2);

    writes.clear();
    cache.next_visible = [key(1), key(2), key(9)].into_iter().collect();
    cache.publish_next(|entity, visible| writes.push((entity, visible)));
    writes.sort_by_key(|(entity, _)| entity.index());
    assert_eq!(writes, [(entity(0), false), (entity(2), true)]);
    assert_eq!(cache.visible_rendered, 2);

    writes.clear();
    cache.next_visible = cache.visible.clone();
    cache.publish_next(|entity, visible| writes.push((entity, visible)));
    assert!(writes.is_empty());
}
