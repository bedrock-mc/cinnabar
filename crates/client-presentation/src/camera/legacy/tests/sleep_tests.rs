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
const BED: [i32; 3] = [2, 0, 5];

mod source_tests;

/// Loads a synthetic bed palette without installing any actor sleep-rotation sample.
fn bed_collisions(app: &mut App, bed: [i32; 3], direction: usize) -> TestCollisions {
    load_bed(app, bed);
    let mut registry = sim::CollisionRegistry::default();
    registry.register(1, []).unwrap();
    TestCollisions {
        registry,
        bed_rotation: Some([90.0, 0.0, 270.0, 180.0][direction]),
    }
}

#[derive(Resource, Default)]
struct TestCollisions {
    registry: sim::CollisionRegistry,
    bed_rotation: Option<f32>,
}

impl CollisionLookup for TestCollisions {
    /// Borrows the synthetic registry shared with the completed physics observation.
    fn registry(&self, _: assets::NetworkIdMode) -> &sim::CollisionRegistry {
        &self.registry
    }

    /// Returns the compiled heading of the fixture's loaded bed palette.
    fn bed_rotation_degrees(&self, _: assets::NetworkIdMode, runtime: u32) -> Option<f32> {
        (runtime == 1).then_some(self.bed_rotation).flatten()
    }

    /// The fixture has no block identifier queries.
    fn block_identifier(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
        None
    }
}

/// Resolves the shared presentation before any camera or actor consumer runs.
fn resolve_presentation(
    mut world: ResMut<TestWorld>,
    collisions: Res<TestCollisions>,
    settings: Res<CameraSettingsAuthority>,
    mut view: ResMut<LocalViewPose>,
    physics: Res<TestPhysics>,
) {
    let position = physics.state.position;
    view.resolve_presentation(
        Some(&mut world.0),
        &*collisions,
        settings.perspective(),
        Some([position.x as f32, position.y as f32, position.z as f32]),
    );
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
fn sleeping_pose_is_shared_before_camera_publication_and_reanchors_only_on_transitions() {
    let bed = BED;
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
            for (direction, bed_rotation) in [90.0, 0.0, 270.0, 180.0].into_iter().enumerate() {
                let mut app = camera_app();
                let mut user = ui::UserSettings::default();
                user.video.java_animations = java;
                user.video.motion_blur = ui::MotionBlurQuality::Medium;
                user.gameplay.default_perspective = perspective;
                app.world_mut()
                    .resource_mut::<CameraSettingsAuthority>()
                    .replace(1, &user)
                    .unwrap();
                let collisions = bed_collisions(&mut app, bed, direction);
                app.world_mut().resource_mut::<TestPhysics>().world_identity = Some(
                    sim::WorldCollisionIdentity::new(collisions.registry.identity(), []).unwrap(),
                );
                app.insert_resource(collisions)
                    .init_resource::<CameraPose>()
                    .init_resource::<CameraPublicationAttempt>()
                    .init_resource::<LocalPlayerFrameCarrier>();
                tick(&mut app, 1);
                app.world_mut().resource_mut::<TestPhysics>().state.position = sim::Vec3::new(
                    f64::from(awake_feet.x),
                    f64::from(awake_feet.y),
                    f64::from(awake_feet.z),
                );
                let physics = app.world().resource::<TestPhysics>().state.clone();
                let camera = app
                    .world_mut()
                    .spawn((
                        FlyCamera::default(),
                        Camera3d::default(),
                        Transform::IDENTITY,
                    ))
                    .id();
                app.add_systems(
                    PostUpdate,
                    crate::camera::motion_blur::apply_camera_motion_blur,
                );
                let mut anchor = app
                    .world()
                    .resource::<LocalViewPose>()
                    .camera_reanchor_epoch();
                let mut blur_epoch = None;
                for (sequence, sleeping) in [(4, false), (5, true), (6, true), (7, false)] {
                    sleep_state(&mut app, sequence, sleeping, Some(bed));
                    {
                        let mut view = app.world_mut().resource_mut::<LocalViewPose>();
                        view.set_subject_position(awake_eye, awake_feet);
                        view.set_rotation(look);
                    }
                    app.world_mut()
                        .run_system_cached(resolve_presentation)
                        .unwrap();
                    let view = *app.world().resource::<LocalViewPose>();
                    let eye = if sleeping { sleeping_eye } else { awake_eye };
                    assert!(view.eye_translation().abs_diff_eq(eye, 1e-5));
                    assert_eq!(
                        view.feet_translation(),
                        if sleeping { feet } else { awake_feet }
                    );
                    assert!(view.rotation().abs_diff_eq(look, 1e-5));
                    assert!(view.look_rotation().abs_diff_eq(look, 1e-5));
                    let transition = sequence == 5 || sequence == 7;
                    assert_eq!(view.camera_reanchor_epoch(), anchor + u64::from(transition));
                    anchor = view.camera_reanchor_epoch();
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
                    // These getters also supply both actor query publication passes.
                    assert!(view.camera_rotation().abs_diff_eq(rotation, 1e-5));
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
                    let mut feed = crate::actor_feed::build_local_player_feed(
                        app.world().resource::<TestPhysics>(),
                        look,
                        false,
                        false,
                        [1; 16],
                        || {
                            protocol::PlayerSkin::Unavailable(
                                protocol::PlayerSkinUnavailable::InvalidDimensions,
                            )
                        },
                        client_world::LocalItemUse::Unpredicted,
                    )
                    .unwrap();
                    view.apply_actor_presentation(&mut feed);
                    // Seed the initial retained pose; later snaps must come from the shared source.
                    feed.teleported |= sequence == 4;
                    {
                        let mut world = app.world_mut().resource_mut::<TestWorld>();
                        world.0.sync_local_player_pose(&feed);
                        let actor = world.0.authority().actor(2).unwrap();
                        for alpha in [0.0, 0.5, 1.0] {
                            let attachment = crate::presentation::actors::actor_emitter_transform(
                                actor,
                                None,
                                &[],
                                None,
                                alpha,
                            )
                            .unwrap();
                            assert_eq!(
                                attachment.position,
                                if sleeping { feet } else { awake_feet }.to_array()
                            );
                        }
                    }
                    app.update();
                    let blur = app
                        .world()
                        .get::<render_scene::CameraMotionBlur>(camera)
                        .unwrap();
                    if transition || sequence == 4 {
                        assert_eq!(blur.exposure_seconds, 0.0);
                        if let Some(previous) = blur_epoch {
                            assert_ne!(blur.reset_epoch, previous);
                        }
                    } else {
                        assert!(blur.exposure_seconds > 0.0);
                        assert_eq!(Some(blur.reset_epoch), blur_epoch);
                    }
                    blur_epoch = Some(blur.reset_epoch);
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
fn sleeping_camera_keeps_its_published_eye_height_in_both_animation_modes() {
    let bed = BED;
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
                // Keep the same lowered eye: neither animation mode moves the camera off it.
                for (sequence, sleeping) in [(2, false), (3, true), (4, false)] {
                    sleep_state(&mut app, sequence, sleeping, bed_metadata);
                    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                        Transform::from_translation(eye);
                    app.world_mut().run_system_cached(sample).unwrap();
                    app.world_mut().run_system_cached(present).unwrap();

                    let rendered = app.world().get::<Transform>(camera).unwrap();
                    assert!(rendered.translation.abs_diff_eq(eye, 1e-5));
                    assert_eq!(rendered.rotation, Quat::IDENTITY);
                    assert_eq!(*app.world().resource::<LocalViewPose>(), view);
                    assert_eq!(app.world().resource::<TestPhysics>().state, physics);
                }
            }
        }
    }
}

/// Commits a resident synthetic bed through the real terrain event and worker lanes.
fn load_bed(app: &mut App, bed: [i32; 3]) {
    let stream = &mut app.world_mut().resource_mut::<TestWorld>().0;
    let mut payload = vec![1, 2];
    payload.extend(std::iter::repeat_n(
        0xff,
        protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count
            - 1,
    ));
    payload.push(0);
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload,
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position: bed,
                layer: 0,
                network_id: 1,
            }]),
        )
        .unwrap();
    test_time::eventually("resident bed direction", || {
        stream.poll([2.5, 2.0, 5.5], 0);
        stream.committed_sequence() >= 3
    });
}

/// Checks the rendered hand angles after sampling the real camera presentation owner.
fn assert_sway(app: &mut App, pitch: f32, yaw: f32) {
    app.world_mut().run_system_cached(sample).unwrap();
    let hand = app.world().resource::<FirstPersonHandMotion>();
    assert!(
        (hand.sway_pitch_radians.to_degrees() - pitch).abs() < 1e-4
            && (hand.sway_yaw_radians.to_degrees() - yaw).abs() < 1e-4,
        "sway ({}, {}) degrees, expected ({pitch}, {yaw})",
        hand.sway_pitch_radians.to_degrees(),
        hand.sway_yaw_radians.to_degrees(),
    );
}

#[test]
fn sleep_entry_after_freelook_reanchors_the_first_person_hand() {
    for java in [false, true] {
        for turn in [360.0_f32, -720.0, 405.0] {
            let mut app = camera_app();
            let mut user = ui::UserSettings::default();
            user.video.java_animations = java;
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .replace(1, &user)
                .unwrap();
            let collisions = bed_collisions(&mut app, BED, 0);
            app.insert_resource(collisions);
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .freelook = true;
            {
                let mut view = app.world_mut().resource_mut::<LocalViewPose>();
                view.set_freelook(true);
                view.turn_look_rotation(
                    Quat::from_rotation_y(-turn.to_radians()),
                    -turn.to_radians(),
                );
            }
            for index in 1..=32 {
                tick(&mut app, index);
                app.world_mut()
                    .run_system_cached(resolve_presentation)
                    .unwrap();
                app.world_mut().run_system_cached(sample).unwrap();
            }
            assert_sway(&mut app, 0.0, 0.0);
            // Sleep arrives while freelook still selects the third-person input heading.
            sleep_state(&mut app, 4, true, Some(BED));
            app.world_mut()
                .run_system_cached(resolve_presentation)
                .unwrap();
            assert_sway(&mut app, 0.0, 0.0);
            let physics = app.world().resource::<TestPhysics>().state.clone();
            let actor = app.world().resource::<LocalViewPose>().rotation();
            // The bed UI releases freelook on the following frame, without a physics tick.
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .freelook = false;
            app.world_mut()
                .resource_mut::<LocalViewPose>()
                .set_freelook(false);
            app.world_mut()
                .run_system_cached(resolve_presentation)
                .unwrap();
            for alpha in [0.0, 0.5, 1.0] {
                app.world_mut().resource_mut::<TestPhysics>().alpha = alpha;
                assert_sway(&mut app, 0.0, 0.0);
            }
            let anchor = app
                .world()
                .resource::<LocalViewPose>()
                .camera_reanchor_epoch();
            for _ in 0..3 {
                app.world_mut()
                    .run_system_cached(resolve_presentation)
                    .unwrap();
                assert_sway(&mut app, 0.0, 0.0);
                assert_eq!(
                    app.world()
                        .resource::<LocalViewPose>()
                        .camera_reanchor_epoch(),
                    anchor
                );
            }
            assert_eq!(app.world().resource::<LocalViewPose>().rotation(), actor);
            assert_eq!(app.world().resource::<TestPhysics>().state, physics);
            // Input still owns the actor heading while the sleeping camera stays on the bed.
            app.world_mut()
                .resource_mut::<LocalViewPose>()
                .turn_look_rotation(
                    Quat::from_rotation_y(-30.0_f32.to_radians()),
                    -30.0_f32.to_radians(),
                );
            app.world_mut()
                .run_system_cached(resolve_presentation)
                .unwrap();
            assert_sway(&mut app, 0.0, 0.0);
            assert_eq!(
                app.world()
                    .resource::<LocalViewPose>()
                    .camera_reanchor_epoch(),
                anchor
            );
            sleep_state(&mut app, 5, false, Some(BED));
            app.world_mut()
                .run_system_cached(resolve_presentation)
                .unwrap();
            assert_sway(&mut app, 0.0, 0.0);
            let view = app.world().resource::<LocalViewPose>();
            assert!(view.camera_rotation().abs_diff_eq(view.rotation(), 1e-5));
            assert_eq!(app.world().resource::<TestPhysics>().state, physics);
            // A real turn immediately after waking must retain its ordinary hand lag.
            app.world_mut()
                .resource_mut::<LocalViewPose>()
                .turn_look_rotation(
                    Quat::from_rotation_y(-60.0_f32.to_radians()),
                    -30.0_f32.to_radians(),
                );
            app.world_mut()
                .run_system_cached(resolve_presentation)
                .unwrap();
            assert_sway(&mut app, 0.0, 3.0);
        }
    }
}

#[test]
fn awake_presentation_preserves_normal_turn_sway_across_perspectives() {
    for java in [false, true] {
        let mut app = camera_app();
        let mut user = ui::UserSettings::default();
        user.video.java_animations = java;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(1, &user)
            .unwrap();
        app.insert_resource(TestCollisions::default());
        tick(&mut app, 1);
        app.world_mut()
            .run_system_cached(resolve_presentation)
            .unwrap();
        assert_sway(&mut app, 0.0, 0.0);
        let anchor = app
            .world()
            .resource::<LocalViewPose>()
            .camera_reanchor_epoch();
        app.world_mut()
            .resource_mut::<LocalViewPose>()
            .turn_look_rotation(
                Quat::from_euler(
                    EulerRot::YXZ,
                    -30.0_f32.to_radians(),
                    -20.0_f32.to_radians(),
                    0.0,
                ),
                -30.0_f32.to_radians(),
            );
        for _ in 0..3 {
            app.world_mut()
                .run_system_cached(resolve_presentation)
                .unwrap();
            assert_sway(&mut app, 2.0, 3.0);
            assert_eq!(
                app.world()
                    .resource::<LocalViewPose>()
                    .camera_reanchor_epoch(),
                anchor
            );
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .cycle_perspective();
        }
        tick(&mut app, 2);
        for (alpha, gain) in [(0.0, 1.0), (0.5, 0.75), (1.0, 0.5)] {
            app.world_mut().resource_mut::<TestPhysics>().alpha = alpha;
            app.world_mut()
                .run_system_cached(resolve_presentation)
                .unwrap();
            assert_sway(&mut app, 2.0 * gain, 3.0 * gain);
        }
    }
}
