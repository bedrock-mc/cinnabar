use super::*;
use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, WorldEvent,
};
use semantic_input::PerspectiveMode;
use simulation_clock::identity::SessionGeneration;
use std::sync::Arc;

use crate::local_player::{CameraPose, CameraPublicationContext, LocalPlayerFrameCarrier};
use crate::local_player_camera_receipt::{
    CameraPublicationAttempt, begin_camera_publication_attempt, published_default_camera,
};
use crate::observations::{CollisionLookup, SessionObservation};

const SESSION: SessionGeneration = SessionGeneration::new(1);

/// Applies camera effects with the same world observation used to sample hand motion.
#[allow(clippy::too_many_arguments)]
fn present(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    hand: Res<FirstPersonHandMotion>,
    view: Res<LocalViewPose>,
    world: Res<TestWorld>,
    server: ResMut<ServerCameraView>,
    cameras: Query<(&mut Transform, Option<&mut Projection>), With<FlyCamera>>,
) {
    crate::camera::presentation::apply_camera_presentation(
        time,
        settings,
        None,
        hand,
        None,
        view,
        Some(WorldObservation {
            stream: Some(&world.0),
        }),
        None,
        server,
        cameras,
    );
}

#[derive(Resource, Default)]
struct TestCollisions(sim::CollisionRegistry);

impl CollisionLookup for TestCollisions {
    /// Borrows the synthetic registry shared with the completed physics observation.
    fn registry(&self, _: assets::NetworkIdMode) -> &sim::CollisionRegistry {
        &self.0
    }

    /// The fixture has no block state queries.
    fn block_canonical_state(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
        None
    }

    /// The fixture has no block identifier queries.
    fn block_identifier(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
        None
    }
}

/// Runs the camera owner with completed physics and collision observations.
#[allow(clippy::too_many_arguments)]
fn resolve(
    world: Res<TestWorld>,
    collisions: Res<TestCollisions>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    published: ResMut<CameraPose>,
    physics: Res<TestPhysics>,
    receipt: ResMut<CameraPublicationAttempt>,
    camera: Single<(Entity, &mut Transform), With<FlyCamera>>,
) {
    crate::local_player::resolve_camera_pose(
        WorldObservation {
            stream: Some(&world.0),
        },
        &*collisions,
        settings,
        view,
        published,
        CameraPublicationContext {
            clock: SessionObservation(SESSION),
            physics: &*physics,
            receipt,
        },
        camera,
    );
}

/// Publishes the local frame from the real camera owner's prepared receipt.
fn publish(
    world: Res<TestWorld>,
    physics: Res<TestPhysics>,
    receipt: ResMut<CameraPublicationAttempt>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    camera: Res<CameraPose>,
    carrier: ResMut<LocalPlayerFrameCarrier>,
) {
    crate::local_player::publish_local_player_frame(
        WorldObservation {
            stream: Some(&world.0),
        },
        CameraPublicationContext {
            clock: SessionObservation(SESSION),
            physics: &*physics,
            receipt,
        },
        settings,
        view,
        camera,
        carrier,
    );
}

/// Reads the camera through the same publication gate used by named audio.
fn audio_camera(
    world: Res<TestWorld>,
    receipt: Res<CameraPublicationAttempt>,
    frame: Res<LocalPlayerFrameCarrier>,
    camera: Res<CameraPose>,
    physics: Res<TestPhysics>,
    cameras: Query<&Transform, With<FlyCamera>>,
) -> Option<[f32; 3]> {
    published_default_camera(
        WorldObservation {
            stream: Some(&world.0),
        },
        SESSION,
        &receipt,
        &frame,
        &camera,
        &*physics,
        &cameras,
    )
}

#[test]
fn sleeping_rotation_is_published_before_camera_effects_and_keeps_audio_eligible() {
    let bed = [2, 70, -5];
    let (sleeping_eye, feet) = crate::local_player::sleeping_subject_position(bed);
    let awake_feet = Vec3::new(feet.x, 0.0, feet.z);
    let awake_eye = awake_feet + Vec3::Y * protocol::STANDING_PLAYER_EYE_HEIGHT;
    let look = Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 0.0);
    for java in [false, true] {
        for perspective in [
            PerspectiveMode::FirstPerson,
            PerspectiveMode::ThirdPersonBack,
            PerspectiveMode::ThirdPersonFront,
        ] {
            for bed_rotation in [0.0, 90.0, 180.0, 270.0] {
                let mut app = camera_app();
                let mut user = ui::UserSettings::default();
                user.video.java_animations = java;
                user.gameplay.default_perspective = perspective;
                app.world_mut()
                    .resource_mut::<CameraSettingsAuthority>()
                    .replace(1, &user)
                    .unwrap();
                let collisions = TestCollisions::default();
                app.world_mut().resource_mut::<TestPhysics>().world_identity =
                    Some(sim::WorldCollisionIdentity::new(collisions.0.identity(), []).unwrap());
                app.insert_resource(collisions)
                    .init_resource::<CameraPose>()
                    .init_resource::<CameraPublicationAttempt>()
                    .init_resource::<LocalPlayerFrameCarrier>();
                tick(&mut app, 1);
                let physics = app.world().resource::<TestPhysics>().state.clone();
                let camera = app
                    .world_mut()
                    .spawn((FlyCamera::default(), Transform::IDENTITY))
                    .id();
                for (sequence, sleeping) in [(2, false), (3, true), (4, true), (5, false)] {
                    sleep_state(&mut app, sequence, sleeping, Some(bed));
                    app.world_mut()
                        .resource_mut::<TestWorld>()
                        .0
                        .set_actor_bed_rotations(&[(2, bed_rotation)]);
                    let eye = if sleeping { sleeping_eye } else { awake_eye };
                    let subject_feet = if sleeping { feet } else { awake_feet };
                    {
                        let mut view = app.world_mut().resource_mut::<LocalViewPose>();
                        view.set_subject_position(eye, subject_feet);
                        view.set_rotation(look);
                    }
                    let view = *app.world().resource::<LocalViewPose>();
                    app.world_mut()
                        .run_system_cached(begin_camera_publication_attempt)
                        .unwrap();
                    app.world_mut().run_system_cached(resolve).unwrap();
                    let captured = *app.world().resource::<CameraPose>().transform();
                    let rotation = if sleeping && perspective == PerspectiveMode::FirstPerson {
                        crate::local_player::sleeping_view_rotation(bed_rotation)
                    } else {
                        look
                    };
                    let expected = crate::camera::perspective_pose(eye, rotation, perspective);
                    assert!(captured.translation.abs_diff_eq(expected.translation, 1e-5));
                    assert!(captured.rotation.abs_diff_eq(expected.rotation, 1e-5));
                    app.world_mut().run_system_cached(sample).unwrap();
                    app.world_mut().run_system_cached(present).unwrap();
                    app.world_mut().run_system_cached(publish).unwrap();

                    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), captured);
                    assert_eq!(
                        *app.world()
                            .resource::<LocalPlayerFrameCarrier>()
                            .snapshot()
                            .unwrap()
                            .pose(),
                        captured
                    );
                    let eligible = app.world_mut().run_system_cached(audio_camera).unwrap();
                    assert_eq!(
                        eligible,
                        (perspective == PerspectiveMode::FirstPerson)
                            .then_some(captured.translation.to_array())
                    );
                    assert_eq!(*app.world().resource::<LocalViewPose>(), view);
                    assert_eq!(app.world().resource::<TestPhysics>().state, physics);
                }
            }
        }
    }
}

/// Updates the local actor's sleeping flag and optional bed through the world event lane.
fn sleep_state(app: &mut App, sequence: u64, sleeping: bool, bed: Option<[i32; 3]>) {
    let mut metadata = vec![ActorMetadata {
        key: 26,
        value: ActorMetadataValue::Byte(if sleeping { 1 << 1 } else { 0 }),
    }];
    if let Some(bed) = bed {
        metadata.push(ActorMetadata {
            key: 28,
            value: ActorMetadataValue::BlockPosition(bed),
        });
    }
    app.world_mut()
        .resource_mut::<TestWorld>()
        .0
        .submit(
            sequence,
            WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 2,
                metadata: Arc::from(metadata),
                properties: Arc::from([]),
                tick: sequence,
            })),
        )
        .unwrap();
}

#[test]
fn sleeping_camera_keeps_its_published_eye_height_and_restores_java_adjustment_on_waking() {
    let bed = [2, 70, -5];
    let (eye, feet) = crate::local_player::sleeping_subject_position(bed);
    for java in [false, true] {
        for perspective in [
            PerspectiveMode::FirstPerson,
            PerspectiveMode::ThirdPersonBack,
            PerspectiveMode::ThirdPersonFront,
        ] {
            for bed_metadata in [Some(bed), None] {
                let mut app = camera_app();
                let mut user = ui::UserSettings::default();
                user.video.java_animations = java;
                user.gameplay.default_perspective = perspective;
                app.world_mut()
                    .resource_mut::<CameraSettingsAuthority>()
                    .replace(1, &user)
                    .unwrap();
                tick(&mut app, 1);
                app.world_mut()
                    .resource_mut::<LocalViewPose>()
                    .set_subject_position(eye, feet);
                let view = *app.world().resource::<LocalViewPose>();
                let physics = app.world().resource::<TestPhysics>().state.clone();
                let camera = app
                    .world_mut()
                    .spawn((FlyCamera::default(), Transform::from_translation(eye)))
                    .id();
                // Keep the same lowered eye to check that only awake Java poses are corrected.
                for (sequence, sleeping) in [(2, false), (3, true), (4, false)] {
                    sleep_state(&mut app, sequence, sleeping, bed_metadata);
                    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                        Transform::from_translation(eye);
                    app.world_mut().run_system_cached(sample).unwrap();
                    app.world_mut().run_system_cached(present).unwrap();

                    let adjustment = if java && !sleeping {
                        protocol::STANDING_PLAYER_EYE_HEIGHT - (eye.y - feet.y)
                    } else {
                        0.0
                    };
                    assert_eq!(
                        app.world()
                            .resource::<FirstPersonHandMotion>()
                            .eye_height_adjustment,
                        adjustment
                    );
                    let rendered = app.world().get::<Transform>(camera).unwrap();
                    assert!(
                        rendered
                            .translation
                            .abs_diff_eq(eye + Vec3::Y * adjustment, 1e-5)
                    );
                    assert_eq!(rendered.rotation, Quat::IDENTITY);
                    assert_eq!(*app.world().resource::<LocalViewPose>(), view);
                    assert_eq!(app.world().resource::<TestPhysics>().state, physics);
                }
            }
        }
    }
}
