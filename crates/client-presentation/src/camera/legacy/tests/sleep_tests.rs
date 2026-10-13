use super::*;
use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, WorldEvent,
};
use semantic_input::PerspectiveMode;
use std::sync::Arc;

/// Publishes the camera core with the same world observation used to sample hand motion.
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
                    let rotation = if sleeping
                        && bed_metadata.is_some()
                        && perspective == PerspectiveMode::FirstPerson
                    {
                        crate::local_player::sleeping_view_rotation(0.0)
                    } else {
                        Quat::IDENTITY
                    };
                    assert!(rendered.rotation.abs_diff_eq(rotation, 1e-5));
                    assert_eq!(*app.world().resource::<LocalViewPose>(), view);
                    assert_eq!(app.world().resource::<TestPhysics>().state, physics);
                }
            }
        }
    }
}
