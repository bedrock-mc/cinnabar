//! Tests the real camera writer/frame publication, not a trusted receipt factory.
use bevy::prelude::*;
use client_presentation::local_player_camera_receipt::{
    CameraPublicationAttempt, begin_camera_publication_attempt,
};
use std::{path::Path, sync::Arc, time::Duration};
use {
    crate::{
        local_player::{publish_local_player_frame, resolve_camera_pose},
        runtime::world::ClientWorld,
    },
    client_interaction::movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    client_presentation::environment::WorldClock,
    client_presentation::{
        camera::{CameraSettingsAuthority, FlyCamera},
        local_player::{CameraPose, LocalPlayerFrameCarrier, LocalViewPose},
    },
};

struct EmptyWorld;
impl sim::CollisionWorld for EmptyWorld {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}
fn camera_app() -> App {
    let mut world = ClientWorld::new(Arc::new(assets::RuntimeAssets::diagnostic()));
    world.stream = Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    }));
    let collisions = PhysicsCollisionRegistries::bind_coherent_assets(
        assets::pinned_block_registry_bytes(),
        include_bytes!("../../../crates/assets/data/block-physics-v2193.bin"),
        Path::new("fixture.preg"),
        Path::new("fixture.mcbea"),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 64.0, 0.0], 0, false);
    physics.advance(
        Duration::from_millis(50),
        sim::MovementInput::default(),
        &EmptyWorld,
    );
    assert!(physics.last_world_identity().is_some());
    let mut app = App::new();
    app.insert_resource(world)
        .insert_resource(collisions)
        .insert_resource(physics)
        .init_resource::<WorldClock>()
        .init_resource::<CameraPose>()
        .init_resource::<LocalViewPose>()
        .init_resource::<CameraSettingsAuthority>()
        .init_resource::<LocalPlayerFrameCarrier>()
        .init_resource::<CameraPublicationAttempt>()
        .add_systems(
            Update,
            (
                begin_camera_publication_attempt,
                resolve_camera_pose,
                publish_local_player_frame,
            )
                .chain(),
        );
    app
}

#[test]
fn missing_or_ambiguous_camera_cannot_relabel_prior_pose_with_new_frame() {
    let mut app = camera_app();
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<LocalPlayerFrameCarrier>()
            .snapshot()
            .is_none()
    );
    let entity = app
        .world_mut()
        .spawn((FlyCamera::default(), Transform::default()))
        .id();
    app.update();
    let proof = app
        .world()
        .resource::<CameraPublicationAttempt>()
        .published()
        .unwrap();
    assert_eq!(proof.entity, entity);
    let generation = proof.frame_generation;
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .unwrap()
            .frame_generation
            > generation
    );
    let second = app
        .world_mut()
        .spawn((FlyCamera::default(), Transform::default()))
        .id();
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<LocalPlayerFrameCarrier>()
            .snapshot()
            .is_none()
    );
    app.world_mut().despawn(second);
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_some()
    );
    app.world_mut().despawn(entity);
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_none()
    );
}
#[test]
fn session_removal_and_real_physics_reset_withhold_receipt_until_valid_observation() {
    let mut app = camera_app();
    app.world_mut()
        .spawn((FlyCamera::default(), Transform::default()));
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_some()
    );
    app.world_mut()
        .resource_mut::<LocalPhysicsController>()
        .reanchor_network_position([0.0, 64.0, 0.0], 2, false);
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<LocalPlayerFrameCarrier>()
            .snapshot()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<LocalPhysicsController>()
        .advance(
            Duration::from_millis(50),
            sim::MovementInput::default(),
            &EmptyWorld,
        );
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_some()
    );
    app.world_mut().resource_mut::<ClientWorld>().stream = None;
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_none()
    );
}

#[derive(Resource, Default)]
struct ReplaceAfterWriter(bool);
fn replace_after_writer(mut flag: ResMut<ReplaceAfterWriter>, mut world: ResMut<ClientWorld>) {
    if std::mem::take(&mut flag.0) {
        world.stream = Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0, 64.0, 0.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        }));
    }
}

#[test]
fn stream_replacement_between_actual_writer_and_publisher_cannot_reuse_prepared_proof() {
    let mut app = camera_app();
    app.init_resource::<ReplaceAfterWriter>().add_systems(
        Update,
        replace_after_writer
            .after(resolve_camera_pose)
            .before(publish_local_player_frame),
    );
    app.world_mut()
        .spawn((FlyCamera::default(), Transform::default()));
    app.update();
    let old = app
        .world()
        .resource::<CameraPublicationAttempt>()
        .published()
        .unwrap();
    app.world_mut().resource_mut::<ReplaceAfterWriter>().0 = true;
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<LocalPlayerFrameCarrier>()
            .snapshot()
            .is_none()
    );
    app.update();
    let new = app
        .world()
        .resource::<CameraPublicationAttempt>()
        .published()
        .unwrap();
    assert_ne!(new.owner.stream, old.owner.stream);
    assert!(new.frame_generation > old.frame_generation);
}

#[derive(Resource, Default)]
struct CorruptAfterWriter(bool);
fn corrupt_after_writer(mut flag: ResMut<CorruptAfterWriter>, mut pose: ResMut<CameraPose>) {
    if std::mem::take(&mut flag.0) {
        let mut changed = *pose.transform();
        changed.translation.x = f32::NAN;
        *pose = CameraPose::new(changed);
    }
}

#[test]
fn nonfinite_pose_changed_after_writer_is_not_published_as_the_written_camera() {
    let mut app = camera_app();
    app.init_resource::<CorruptAfterWriter>().add_systems(
        Update,
        corrupt_after_writer
            .after(resolve_camera_pose)
            .before(publish_local_player_frame),
    );
    app.world_mut()
        .spawn((FlyCamera::default(), Transform::default()));
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_some()
    );
    app.world_mut().resource_mut::<CorruptAfterWriter>().0 = true;
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<LocalPlayerFrameCarrier>()
            .snapshot()
            .is_none()
    );
    app.update();
    assert!(
        app.world()
            .resource::<CameraPublicationAttempt>()
            .published()
            .is_some()
    );
}

#[test]
fn sub_chunk_origin_uses_published_third_person_camera_without_a_new_physics_tick() {
    use crate::runtime::world::sub_chunk_request_center;
    let mut app = camera_app();
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .set_eye_translation(Vec3::new(-0.1, 90.0, -16.1));
    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .cycle_perspective();
    app.world_mut()
        .spawn((FlyCamera::default(), Transform::default()));
    app.update();
    let camera = app
        .world()
        .resource::<CameraPublicationAttempt>()
        .published()
        .unwrap();
    let tick = camera.tick;
    let position = camera.transform.translation;
    let expected = [
        (position.x.floor() as i32).div_euclid(16),
        0,
        (position.z.floor() as i32).div_euclid(16),
    ];
    let owner = app.world().resource::<ClientWorld>();
    let stream = owner.stream.as_ref().unwrap();
    let publication = app.world().resource::<CameraPublicationAttempt>();
    let session = app.world().resource::<WorldClock>().session_generation();
    assert_eq!(
        sub_chunk_request_center(stream, Some(publication), session, [3_000.0; 3]),
        Some(expected)
    );
    assert_eq!(
        sub_chunk_request_center(stream, Some(publication), session + 1, [-0.1, 900.0, -16.1]),
        Some([-1, 0, -2])
    );
    app.update();
    let publication = app.world().resource::<CameraPublicationAttempt>();
    assert_eq!(publication.published().unwrap().tick, tick);
    assert_eq!(
        sub_chunk_request_center(
            app.world()
                .resource::<ClientWorld>()
                .stream
                .as_ref()
                .unwrap(),
            Some(publication),
            session,
            [3_000.0; 3]
        ),
        Some(expected)
    );
}

#[test]
fn sub_chunk_origin_rejects_stale_camera_owners_and_falls_back_without_camera() {
    use crate::runtime::world::sub_chunk_request_center;
    let mut app = camera_app();
    app.world_mut()
        .spawn((FlyCamera::default(), Transform::default()));
    app.update();
    let session = app.world().resource::<WorldClock>().session_generation();
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            1,
            protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                dimension: 1,
                position: [400.0, 80.0, 400.0],
                ..Default::default()
            }),
        )
        .unwrap();
    let world = app.world().resource::<ClientWorld>();
    let stream = world.stream.as_ref().unwrap();
    let publication = app.world().resource::<CameraPublicationAttempt>();
    let actor = [-0.1, f32::NAN, -16.1];
    assert_eq!(
        sub_chunk_request_center(stream, Some(publication), session, actor),
        Some([-1, 0, -2])
    );
    assert_eq!(
        sub_chunk_request_center(stream, None, session, actor),
        Some([-1, 0, -2])
    );
    assert_eq!(
        sub_chunk_request_center(stream, None, session, [f32::NAN; 3]),
        None
    );
    let replacement = camera_app();
    assert_eq!(
        sub_chunk_request_center(
            replacement
                .world()
                .resource::<ClientWorld>()
                .stream
                .as_ref()
                .unwrap(),
            Some(publication),
            session,
            actor
        ),
        Some([-1, 0, -2])
    );
    let absent = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_runtime_id: 0,
        local_player_unique_id: 0,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    assert_eq!(
        sub_chunk_request_center(&absent, Some(publication), session, actor),
        None
    );
}
