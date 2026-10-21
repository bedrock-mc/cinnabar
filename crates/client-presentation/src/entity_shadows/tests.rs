use super::*;
use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldEvent,
};
use render_api::PoseGeneration;
use std::sync::Arc;

fn view() -> ActorCullView {
    let camera = Vec3::new(0.0, 70.0, 0.0);
    let clip_from_view =
        glam::camera::rh::proj::directx::perspective_infinite_reverse(1.2, 1.0, 0.05);
    let view_from_world = glam::camera::rh::view::look_to_mat4(camera, Vec3::NEG_Z, Vec3::Y);
    ActorCullView {
        clip_from_world: clip_from_view * view_from_world,
        camera_position: camera,
        max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
    }
}

fn at(x: f32, y: f32, z: f32) -> EntityShadow {
    EntityShadow {
        feet: [x, y, z],
        radius: 0.6,
    }
}

#[test]
fn casters_ahead_are_kept_and_casters_behind_are_culled() {
    assert!(volume_may_be_visible(&at(0.0, 66.0, -8.0), false, view()));
    assert!(!volume_may_be_visible(&at(0.0, 66.0, 8.0), false, view()));
}

/// A shadow whose volume reaches into view is kept even when its feet are just off screen.
#[test]
fn a_volume_reaching_into_view_is_kept() {
    // The view spans y 68.6..71.4 two blocks ahead; the volume hangs 1.8 below the feet.
    assert!(volume_may_be_visible(&at(0.0, 72.0, -2.0), false, view()));
    assert!(!volume_may_be_visible(&at(0.0, 74.0, -2.0), false, view()));
}

#[test]
fn entities_outside_the_candidate_cube_are_culled_but_players_are_not() {
    let far = at(
        0.0,
        64.0,
        -(render_model::ACTOR_CANDIDATE_RADIUS_BLOCKS + 4.0),
    );
    assert!(!volume_may_be_visible(&far, false, view()));
    assert_eq!(
        volume_may_be_visible(&far, true, view()),
        render_model::ACTOR_CANDIDATE_RADIUS_BLOCKS + 4.0
            <= render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS
    );
}

fn spawn(stream: &mut WorldStream, sequence: u64, runtime_id: u64, identifier: &str, x: f32) {
    spawn_kind(
        stream,
        sequence,
        runtime_id,
        ActorKind::Entity {
            identifier: format!("minecraft:{identifier}").into(),
        },
        x,
    );
}

fn spawn_kind(stream: &mut WorldStream, sequence: u64, runtime_id: u64, kind: ActorKind, x: f32) {
    stream
        .submit(
            sequence,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: runtime_id as i64,
                runtime_id,
                kind,
                position: [x, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([ActorMetadata {
                    key: 53,
                    value: ActorMetadataValue::Float(0.5),
                }]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
}

/// A rig the frame did not draw (capacity overflow, no drawable route) leaves no floating
/// shadow; dropped items, drawn by their own renderer, keep theirs.
#[test]
fn only_drawn_bodies_and_dropped_items_cast() {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0, 65.62, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    spawn(&mut stream, 1, 5, "cow", 2.0);
    spawn(&mut stream, 2, 6, "cow", 4.0);
    spawn(&mut stream, 3, 7, "item", 6.0);
    let mut staging = Vec::new();
    let mut scene = EntityShadowScene::default();
    publish_entity_shadows(
        Some(&stream),
        1.0,
        None,
        None,
        &[5],
        &mut staging,
        &mut scene,
    );
    let mut casters: Vec<f32> = scene
        .0
        .shadows
        .iter()
        .map(|shadow| shadow.feet[0])
        .collect();
    casters.sort_by(f32::total_cmp);
    assert_eq!(casters, [2.0, 6.0]);
}

#[test]
fn local_shadow_follows_body_visibility_without_hiding_other_casters() {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0, 65.62, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    spawn_kind(
        &mut stream,
        1,
        1,
        ActorKind::Player {
            uuid: [1; 16],
            username: "local".into(),
        },
        0.0,
    );
    spawn(&mut stream, 2, 5, "cow", 2.0);
    spawn(&mut stream, 3, 7, "item", 6.0);
    let local = LocalShadowSource {
        runtime_id: stream.local_player_runtime_id(),
        visible: true,
        feet: Some([9.0, 64.0, 0.0]),
        spectator: false,
    };
    let mut staging = Vec::new();
    let mut scene = EntityShadowScene::default();

    for (drawn, expected) in [
        (&[5][..], vec![2.0, 6.0]),
        (&[1, 5][..], vec![2.0, 6.0, 9.0]),
        (&[5][..], vec![2.0, 6.0]),
    ] {
        publish_entity_shadows(
            Some(&stream),
            1.0,
            Some(local),
            None,
            drawn,
            &mut staging,
            &mut scene,
        );
        let mut casters: Vec<_> = scene
            .0
            .shadows
            .iter()
            .map(|shadow| shadow.feet[0])
            .collect();
        casters.sort_by(f32::total_cmp);
        assert_eq!(casters, expected);
    }

    publish_entity_shadows(
        Some(&stream),
        1.0,
        Some(LocalShadowSource {
            spectator: true,
            ..local
        }),
        None,
        &[1, 5],
        &mut staging,
        &mut scene,
    );
    assert!(scene.0.shadows.iter().all(|shadow| shadow.feet[0] != 9.0));

    publish_entity_shadows(
        Some(&stream),
        1.0,
        Some(LocalShadowSource {
            visible: false,
            ..local
        }),
        None,
        &[1, 5],
        &mut staging,
        &mut scene,
    );
    let mut casters: Vec<_> = scene
        .0
        .shadows
        .iter()
        .map(|shadow| shadow.feet[0])
        .collect();
    casters.sort_by(f32::total_cmp);
    assert_eq!(casters, [2.0, 6.0]);
}

/// Builds a manifest entry whose actor namespace and layer are independently controlled.
fn manifest_entry(
    session_id: u64,
    runtime_id: u64,
    layer: u8,
) -> render_model::ActorDrawManifestEntry {
    render_model::ActorDrawManifestEntry {
        identity: render_model::ActorRenderIdentity {
            session_id,
            runtime_id,
            layer,
            dimension: 0,
            spawn_revision: 1,
            ingress_sequence: 1,
            source_tick: None,
            movement_revision: 1,
            pose_generation: PoseGeneration::new(1),
        },
        rig: render_model::EntityRigId(0),
        completed_tick: 1,
        reset_generation: 1,
        route: render_model::ActorRigRoute::Compiled,
        instance_index: 0,
        previous_bone_base: 0,
        current_bone_base: 0,
        bone_count: 1,
    }
}

#[test]
fn regression_display_runtime_ids_cannot_admit_undrawn_network_shadows() {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0, 65.62, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    spawn(&mut stream, 1, 5, "cow", 2.0);
    spawn(&mut stream, 2, 6, "cow", 4.0);
    let session = stream.authority().actor_session_id();
    let mut frame = ActorRenderFrame::default();
    frame.rig.manifest = Arc::from([
        manifest_entry(session + 1, 5, ACTOR_LAYER_BODY),
        manifest_entry(session, 5, 1),
        manifest_entry(session, 6, ACTOR_LAYER_BODY),
    ]);
    let mut drawn = vec![99];
    drawn_bodies(&frame, Some(session), &mut drawn);
    let mut staging = Vec::new();
    let mut scene = EntityShadowScene::default();
    publish_entity_shadows(
        Some(&stream),
        1.0,
        None,
        None,
        &drawn,
        &mut staging,
        &mut scene,
    );
    assert_eq!(scene.0.shadows.len(), 1);
    assert_eq!(scene.0.shadows[0].feet[0], 4.0);
    drawn_bodies(&frame, None, &mut drawn);
    assert!(
        drawn.is_empty(),
        "a disconnected network session admits no bodies"
    );
}

/// Coverage-only source meshes never leak shadows, and visibility resumes when coverage ends.
#[test]
fn coverage_only_remote_bodies_do_not_cast_ordinary_shadows() {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0, 65.62, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    for (sequence, runtime_id, x) in [(1, 5, 2.0), (2, 6, 4.0)] {
        spawn_kind(
            &mut stream,
            sequence,
            runtime_id,
            ActorKind::Player {
                uuid: [runtime_id as u8; 16],
                username: "fixture".into(),
            },
            x,
        );
    }
    let session = stream.authority().actor_session_id();
    let mut frame = ActorRenderFrame::default();
    frame.rig.manifest = Arc::from([
        manifest_entry(session, 5, ACTOR_LAYER_BODY),
        manifest_entry(session, 6, ACTOR_LAYER_BODY),
    ]);
    let mut drawn = Vec::new();
    let mut staging = Vec::new();
    let mut scene = EntityShadowScene::default();
    for (hidden, expected) in [
        (vec![6], vec![2.0]),
        (vec![], vec![2.0, 4.0]),
        (vec![5], vec![4.0]),
        (vec![5, 6], vec![]),
        (vec![], vec![2.0, 4.0]),
    ] {
        frame.mask_only_runtime_ids = hidden.into();
        drawn_bodies(&frame, Some(session), &mut drawn);
        publish_entity_shadows(
            Some(&stream),
            1.0,
            None,
            None,
            &drawn,
            &mut staging,
            &mut scene,
        );
        let mut actual: Vec<_> = scene
            .0
            .shadows
            .iter()
            .map(|shadow| shadow.feet[0])
            .collect();
        actual.sort_by(f32::total_cmp);
        assert_eq!(
            actual, expected,
            "mask-only IDs: {:?}",
            frame.mask_only_runtime_ids
        );
    }
}
