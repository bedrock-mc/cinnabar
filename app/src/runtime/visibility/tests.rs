use super::*;

#[test]
fn disabling_culling_reveals_sections_at_any_height_and_discards_stale_proofs() {
    let mut world = bevy::prelude::World::new();
    let visible = SubChunkKey::new(0, 0, 4, 0);
    let keys = [
        visible,
        SubChunkKey::new(0, 0, -4, 0),
        SubChunkKey::new(0, 0, 19, 0),
    ];
    let entities = keys.map(|_| world.spawn_empty().id());
    let mut cache = CaveVisibilityCache {
        initialized: true,
        camera: Some(visible),
        graph_generation: Some(7),
        visible: [visible].into_iter().collect(),
        rendered: keys.into_iter().zip(entities).collect(),
        visible_rendered: 1,
        ..Default::default()
    };
    let mut revealed = Vec::new();
    cache.show_all(|entity, visible| {
        assert!(visible);
        revealed.push(entity);
    });
    assert_eq!(revealed.len(), 2);
    assert!(revealed.contains(&entities[1]) && revealed.contains(&entities[2]));
    assert_eq!(cache.visible_rendered, keys.len());
    assert!(keys.into_iter().all(|key| cache.is_visible(key)));
    assert!(!cache.hides_box(visible, 7, |_| true, [0.0, -60.0, 0.0], [1.0, -59.0, 1.0]));
    cache.show_all(|_, _| panic!("unchanged disabled frames must not republish visibility"));
    cache.next_visible = [visible].into_iter().collect();
    let mut hidden = Vec::new();
    cache.publish_next(|entity, visible| {
        assert!(!visible);
        hidden.push(entity);
    });
    assert_eq!(hidden.len(), 2);
    assert!(hidden.contains(&entities[1]) && hidden.contains(&entities[2]));
    assert_eq!(cache.visible_rendered, 1);
    assert!(cache.is_visible(keys[0]) && !cache.is_visible(keys[1]));
}

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
    use bevy::math::Vec3;

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
    let view = render_model::ActorCullView {
        clip_from_world: glam::camera::rh::proj::directx::perspective_infinite_reverse(
            std::f32::consts::FRAC_PI_2,
            1.0,
            0.1,
        ) * glam::camera::rh::view::look_at_mat4(
            after,
            Vec3::from_array(feet) + Vec3::Y,
            Vec3::Y,
        ),
        camera_position: after,
        max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    };
    assert!(render_model::actor_bounds_are_visible(
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

/// Particle instance building shares the cave cache's current, loaded-section rule.
#[test]
fn particles_draw_when_cave_results_are_stale_unknown_or_touch_a_visible_section() {
    use particles::{EmptyWorld, ParticleSystem, ParticleView, SpawnRequest};

    let key = |x, y, z| SubChunkKey::new(0, x, y, z);
    let camera = key(0, 0, 0);
    let mut cache = CaveVisibilityCache {
        camera: Some(camera),
        graph_generation: Some(7),
        visible: [camera].into_iter().collect(),
        initialized: true,
        ..Default::default()
    };
    let view = ParticleView {
        position: [8.0, 8.0, 8.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(
        br#"{"particle_effect":{
        "description":{"identifier":"test:cave","basic_render_parameters":{
            "material":"particles_alpha","texture":"textures/none"}},
        "components":{
            "minecraft:emitter_rate_instant":{"num_particles":1},
            "minecraft:emitter_lifetime_once":{"active_time":1},
            "minecraft:emitter_shape_point":{},
            "minecraft:particle_lifetime_expression":{"max_lifetime":2},
            "minecraft:particle_appearance_billboard":{"size":[0.5,0.5]}
        }}}"#
    ));
    system.spawn(&SpawnRequest {
        effect: "test:cave".into(),
        position: [8.0, 8.0, -8.0],
        ..Default::default()
    });
    system.tick(0.02, &EmptyWorld);
    for (current_camera, generation, loaded, hidden) in [
        (camera, 7, true, true),
        (key(1, 0, 0), 7, true, false),
        (SubChunkKey::new(1, 0, 0, 0), 7, true, false),
        (camera, 8, true, false),
        (camera, 7, false, false),
    ] {
        let lists = system.build_draw_with_visibility(&view, &EmptyWorld, |low, high| {
            cache.hides_box(current_camera, generation, |_| loaded, low, high)
        });
        assert_eq!(lists.cave_hidden, usize::from(hidden));
        assert_eq!(lists.opaque.len(), usize::from(!hidden));
        assert_eq!(system.live_particles(), 1);
    }
    cache.initialized = false;
    assert_eq!(
        system
            .build_draw_with_visibility(&view, &EmptyWorld, |low, high| {
                cache.hides_box(camera, 7, |_| true, low, high)
            })
            .opaque
            .len(),
        1
    );
    cache.initialized = true;
    // The midpoint is hidden, but a long quad reaches the camera's visible section.
    let wide = br#"{"particle_effect":{
        "description":{"identifier":"test:wide","basic_render_parameters":{
            "material":"particles_add","texture":"textures/none"}},
        "components":{
            "minecraft:emitter_rate_instant":{"num_particles":1},
            "minecraft:emitter_lifetime_once":{"active_time":1},
            "minecraft:emitter_shape_point":{},
            "minecraft:particle_lifetime_expression":{"max_lifetime":2},
            "minecraft:particle_appearance_billboard":{"size":[12,0.5],
                "facing_camera_mode":"lookat_direction",
                "direction":{"mode":"custom","custom_direction":[0,0,1]}}
        }}}"#;
    assert!(system.register_effect(wide));
    system.spawn(&SpawnRequest {
        effect: "test:wide".into(),
        position: [8.0, 8.0, -8.0],
        ..Default::default()
    });
    system.tick(0.02, &EmptyWorld);
    let lists = system.build_draw_with_visibility(&view, &EmptyWorld, |low, high| {
        cache.hides_box(camera, 7, |_| true, low, high)
    });
    assert_eq!(lists.cave_hidden, 1);
    assert_eq!(lists.add.len(), 1);
}
