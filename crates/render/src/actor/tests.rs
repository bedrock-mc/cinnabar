use std::sync::Arc;

use bevy::math::{Mat4, Vec3};

use super::{
    ActorCullView, ActorRenderScene, ActorRenderSource, ActorSkinPixels, MAX_RENDERED_PLAYERS,
};

fn source(runtime_id: u64, x: f32, yaw_degrees: f32) -> ActorRenderSource {
    ActorRenderSource {
        runtime_id,
        unique_id: i64::try_from(runtime_id).unwrap_or(i64::MAX),
        spawn_revision: 1,
        movement_revision: 0,
        previous_position: [x, 64.0, 0.0],
        previous_pitch_degrees: 0.0,
        previous_yaw_degrees: yaw_degrees,
        previous_head_yaw_degrees: yaw_degrees,
        position: [x, 64.0, 0.0],
        pitch_degrees: 0.0,
        yaw_degrees,
        head_yaw_degrees: yaw_degrees,
        teleported: false,
        skin: None,
    }
}

fn tick_source(
    runtime_id: u64,
    previous_x: f32,
    current_x: f32,
    previous_yaw: f32,
    current_yaw: f32,
) -> ActorRenderSource {
    ActorRenderSource {
        previous_position: [previous_x, 64.0, 0.0],
        previous_pitch_degrees: 0.0,
        previous_yaw_degrees: previous_yaw,
        previous_head_yaw_degrees: previous_yaw,
        ..source(runtime_id, current_x, current_yaw)
    }
}

fn broad_view(max_distance: f32) -> ActorCullView {
    ActorCullView {
        clip_from_world: Mat4::from_scale(Vec3::splat(0.001)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance,
    }
}

#[test]
fn frame_interpolation_samples_adjacent_actor_ticks() {
    let mut scene = ActorRenderScene::default();
    let frame = scene.update(0.5, None, [tick_source(7, 3.0, 6.0, 0.0, 0.0)]);

    assert_eq!(frame.instances.len(), 1);
    assert!((frame.instances[0].position[0] - 4.5).abs() < 1e-5);
}

#[test]
fn frame_republication_changes_only_with_partial_tick() {
    let source = tick_source(7, 3.0, 6.0, 0.0, 0.0);
    let mut scene = ActorRenderScene::default();
    assert_eq!(
        scene.update(0.0, None, [source.clone()]).instances[0].position[0],
        3.0
    );
    assert_eq!(
        scene.update(0.5, None, [source.clone()]).instances[0].position[0],
        4.5
    );
    assert_eq!(
        scene.update(1.0, None, [source]).instances[0].position[0],
        6.0
    );
}

#[test]
fn frame_angles_take_the_shortest_path_between_tick_poses() {
    let mut scene = ActorRenderScene::default();
    let frame = scene.update(0.5, None, [tick_source(7, 0.0, 0.0, 350.0, 10.0)]);

    assert!(frame.instances[0].yaw_radians.abs() < 1e-5);
}

#[test]
fn teleport_equal_endpoints_never_cross_the_old_position() {
    let mut scene = ActorRenderScene::default();
    for alpha in [0.0, 0.5, 1.0] {
        let frame = scene.update(alpha, None, [tick_source(7, 100.0, 100.0, 90.0, 90.0)]);
        assert_eq!(frame.instances[0].position[0], 100.0);
    }
}

#[test]
fn actor_culling_rejects_wholly_outside_frustum_but_keeps_edge_intersections() {
    let view = ActorCullView {
        clip_from_world: Mat4::from_translation(Vec3::new(0.0, -64.0, 0.0)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance: 192.0,
    };
    let mut scene = ActorRenderScene::default();
    let frame = scene.update(
        1.0,
        Some(view),
        [
            tick_source(1, 0.0, 0.0, 0.0, 0.0),
            tick_source(2, 1.4, 1.4, 0.0, 0.0),
            tick_source(3, 3.0, 3.0, 0.0, 0.0),
        ],
    );

    assert_eq!(
        frame
            .instances
            .iter()
            .map(|actor| actor.runtime_id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn actor_culling_rejects_positions_beyond_the_distance_cap() {
    let mut scene = ActorRenderScene::default();
    let frame = scene.update(
        1.0,
        Some(broad_view(192.0)),
        [
            tick_source(1, 191.0, 191.0, 0.0, 0.0),
            tick_source(2, 193.0, 193.0, 0.0, 0.0),
        ],
    );

    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.instances[0].runtime_id, 1);
}

#[test]
fn culling_occurs_before_the_visible_actor_cap() {
    let mut sources = (0..u64::try_from(MAX_RENDERED_PLAYERS).unwrap())
        .map(|id| tick_source(id, 500.0, 500.0, 0.0, 0.0))
        .collect::<Vec<_>>();
    sources.push(tick_source(999, 0.0, 0.0, 0.0, 0.0));
    let mut scene = ActorRenderScene::default();
    let frame = scene.update(1.0, Some(broad_view(192.0)), sources);

    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.instances[0].runtime_id, 999);
}

#[test]
fn scene_reset_clears_the_published_frame() {
    let mut scene = ActorRenderScene::default();
    scene.update(1.0, None, [source(7, 10.0, 0.0)]);
    scene.reset();
    assert!(scene.frame().instances.is_empty());
}

#[test]
fn scene_rejects_non_finite_sources_and_truncates_stably() {
    let mut sources = (0..u64::try_from(MAX_RENDERED_PLAYERS + 2).unwrap())
        .rev()
        .map(|id| source(id, id as f32, 0.0))
        .collect::<Vec<_>>();
    sources.push(source(u64::MAX, f32::NAN, 0.0));
    let mut scene = ActorRenderScene::default();
    let frame = scene.update(1.0, None, sources);

    assert_eq!(frame.instances.len(), MAX_RENDERED_PLAYERS);
    assert_eq!(frame.instances.first().unwrap().runtime_id, 0);
    assert_eq!(
        frame.instances.last().unwrap().runtime_id,
        u64::try_from(MAX_RENDERED_PLAYERS - 1).unwrap()
    );
}

#[test]
fn high_resolution_standard_skin_is_nearest_sampled_and_invalid_skin_uses_authored_default() {
    let mut rgba8 = vec![0; 128 * 128 * 4];
    rgba8[0..4].copy_from_slice(&[1, 2, 3, 255]);
    let valid = ActorSkinPixels {
        width: 128,
        height: 128,
        rgba8: rgba8.into(),
    };
    let invalid = ActorSkinPixels {
        width: 64,
        height: 64,
        rgba8: vec![0_u8; 4].into(),
    };
    let mut first = source(1, 0.0, 0.0);
    first.skin = Some(valid);
    let mut second = source(2, 0.0, 0.0);
    second.skin = Some(invalid);
    let expected_default = super::default_actor_skin_rgba8();
    let mut scene = ActorRenderScene::default();
    let frame = scene.update(1.0, None, [first, second]);

    let skin = |index: usize| {
        frame
            .player_skin(frame.instances[index].skin_layer)
            .expect("drawn skins are resident")
    };
    assert_eq!(&skin(0)[0..4], &[1, 2, 3, 255]);
    assert_eq!(skin(0).len(), super::STANDARD_SKIN_BYTES);
    assert_eq!(skin(1), &expected_default);
}

/// New skin models and item meshes share one catalog rebuild instead of one each.
#[test]
fn batched_geometries_rebuild_the_catalog_once() {
    let mut builder = super::ActorRigFrameBuilder::new([]).unwrap();
    let cuboid = |slot| {
        super::ActorRigGeometry::synthetic_cuboid(
            render_model::skin_rig_id(slot),
            [0.0; 3],
            [slot as f32 + 1.0, 1.0, 1.0],
            1,
        )
        .unwrap()
    };
    let before = builder.geometry_vertices().len();
    builder
        .insert_geometries((0..8).map(cuboid).collect())
        .unwrap();
    assert!((0..8).all(|slot| builder.contains_geometry(render_model::skin_rig_id(slot))));
    assert_eq!(builder.geometry_vertices().len(), before + 8 * 36);
    let mut duplicate = vec![cuboid(9)];
    duplicate.push(
        super::ActorRigGeometry::synthetic_cuboid(
            super::EntityRigId(u32::MAX),
            [0.0; 3],
            [1.0; 3],
            1,
        )
        .unwrap(),
    );
    assert!(builder.insert_geometries(duplicate).is_err());
    assert!(!builder.contains_geometry(render_model::skin_rig_id(9)));
}

#[test]
fn review_render_geometry_replacement_keeps_configured_artwork() {
    let (pages, _) = super::ActorArtworkPages::default().with_equipment_rasters(&[
        render_model::equipment::EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([1, 2, 3, 255]),
        },
    ]);
    let mut scene = ActorRenderScene::default();
    scene.configure_artwork(pages);
    let before = Arc::clone(&scene.frame.artwork);
    scene.replace_pack_entities(None).unwrap();
    assert!(Arc::ptr_eq(&before, &scene.frame.artwork));
}

#[test]
fn review_render_teleport_samples_the_destination_for_local_and_remote_actors() {
    let mut scene = ActorRenderScene::default();
    let mut actor = tick_source(7, 0.0, 100.0, 0.0, 90.0);
    actor.teleported = true;
    for alpha in [0.0, 0.5, 1.0] {
        let frame = scene.update(alpha, None, [actor.clone()]);
        assert_eq!(frame.instances[0].position[0], 100.0);
        assert!((frame.instances[0].yaw_radians - 90.0_f32.to_radians()).abs() < 1e-5);
        let frame = scene.update_with_local(alpha, None, [], Some(actor.clone()));
        assert_eq!(frame.instances[0].position[0], 100.0);
    }
}
