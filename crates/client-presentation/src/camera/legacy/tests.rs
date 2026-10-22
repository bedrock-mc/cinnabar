//! Scheduled camera contracts with original actors and resolved movement observations.
use super::*;
use crate::actor_publication::ActorFramePartialTick;
use crate::camera::{CameraSettingsAuthority, FirstPersonHandMotion, FlyCamera, ServerCameraView};
use crate::camera::{bob::WalkBobState, hurt::CameraHurtState, java::JavaCameraState};
use crate::local_player::LocalViewPose;
use crate::observations::{CameraMovementTick, PhysicsObservation, WorldObservation};
use crate::presentation::combat::tests::{event, player_stream};
use bevy::prelude::*;
use protocol::ActorStatusKind;

const WALK_BLOCKS_PER_TICK: f32 = 0.2;

mod sleep_tests;

#[derive(Resource)]
struct TestWorld(chunk_pipeline::WorldStream);
#[derive(Resource)]
struct TestPhysics {
    state: sim::PlayerState,
    ticks: Vec<CameraMovementTick>,
    alpha: f32,
    world_identity: Option<sim::WorldCollisionIdentity>,
    sneaking: bool,
}
impl PhysicsObservation for TestPhysics {
    /// Returns the deterministic completed state.
    fn state(&self) -> Option<&sim::PlayerState> {
        Some(&self.state)
    }
    /// Visits each recorded tick without inventing intermediate motion.
    fn visit_camera_ticks(&self, after: Option<u64>, visit: &mut dyn FnMut(CameraMovementTick)) {
        for tick in &self.ticks {
            if after.is_none_or(|after| tick.tick > after) {
                visit(*tick);
            }
        }
    }
    /// Reports the fixture's sneak flag; it never sprints.
    fn latest_sneak_sprint(&self) -> Option<(bool, bool)> {
        Some((self.sneaking, false))
    }
    /// The fixture uses ordinary walking.
    fn mode(&self) -> sim::MovementMode {
        sim::MovementMode::Walking
    }
    /// No glide state is present.
    fn fall_fly_ticks(&self) -> u32 {
        0
    }
    /// Returns the completed collision observation when a publication test supplies it.
    fn last_world_identity(&self) -> Option<&sim::WorldCollisionIdentity> {
        self.world_identity.as_ref()
    }
    /// Translation is simulation-owned in this fixture.
    fn is_active(&self) -> bool {
        true
    }
    /// Uses the fixture's physics fraction independently of the actor clock.
    fn tick_alpha(&self) -> f32 {
        self.alpha
    }
}

/// Runs the real presentation adapter with deterministic borrowed observations.
#[allow(clippy::too_many_arguments)]
fn sample(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    world: Res<TestWorld>,
    physics: Res<TestPhysics>,
    bob: ResMut<WalkBobState>,
    hurt: ResMut<CameraHurtState>,
    java: ResMut<JavaCameraState>,
    legacy: ResMut<LegacyCameraMotion>,
    hand: ResMut<FirstPersonHandMotion>,
) {
    crate::camera::presentation::advance_presentation_state(
        time,
        settings,
        view,
        Some(WorldObservation {
            stream: Some(&world.0),
        }),
        Some(&*physics),
        None,
        bob,
        hurt,
        java,
        legacy,
        hand,
    );
}

/// Applies camera effects with the same world observation used to sample hand motion.
#[allow(clippy::too_many_arguments)]
fn present(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    world: Res<TestWorld>,
    server: ResMut<ServerCameraView>,
    cameras: Query<(&mut Transform, Option<&mut Projection>), With<FlyCamera>>,
) {
    crate::camera::presentation::apply_camera_presentation(
        time,
        settings,
        None,
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

/// Shares the late effect with the hand while retaining each owner's clock fraction.
#[allow(clippy::too_many_arguments)]
fn apply(
    settings: Res<CameraSettingsAuthority>,
    hand: ResMut<FirstPersonHandMotion>,
    legacy: Res<LegacyCameraMotion>,
    server: Res<ServerCameraView>,
    world: Res<TestWorld>,
    physics: Res<TestPhysics>,
    actor_fraction: Res<ActorFramePartialTick>,
    cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    crate::camera::actor_effects::apply_actor_damage_camera_rotation(
        settings,
        hand,
        Some(legacy),
        server,
        Some(WorldObservation {
            stream: Some(&world.0),
        }),
        actor_fraction.0,
        physics.tick_alpha(),
        cameras,
    );
}

/// Creates the camera resources without assets, input devices or a visible window.
fn camera_app() -> App {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<CameraSettingsAuthority>()
        .init_resource::<LocalViewPose>()
        .init_resource::<WalkBobState>()
        .init_resource::<CameraHurtState>()
        .init_resource::<JavaCameraState>()
        .init_resource::<LegacyCameraMotion>()
        .init_resource::<FirstPersonHandMotion>()
        .init_resource::<ServerCameraView>()
        .insert_resource(ActorFramePartialTick(0.5))
        .insert_resource(TestWorld(player_stream()))
        .insert_resource(TestPhysics {
            state: sim::PlayerState::new(sim::Vec3::ZERO),
            ticks: Vec::new(),
            alpha: 0.5,
            world_identity: None,
            sneaking: false,
        });
    app
}

/// Adds resolved motion and keeps the completed feet position aligned with its network position.
fn tick(app: &mut App, index: u64) {
    let mut physics = app.world_mut().resource_mut::<TestPhysics>();
    let position = [index as f32 * WALK_BLOCKS_PER_TICK, 64.0, 0.0];
    physics.state.tick = index;
    physics.state.position = sim::Vec3::new(
        f64::from(position[0]),
        f64::from(position[1] - protocol::PLAYER_NETWORK_OFFSET),
        f64::from(position[2]),
    );
    physics.state.on_ground = true;
    physics.ticks.push(CameraMovementTick {
        tick: index,
        position,
        movement: [WALK_BLOCKS_PER_TICK, 0.0, 0.0],
        velocity: [WALK_BLOCKS_PER_TICK, -0.0784, 0.0],
        on_ground: true,
        riding: false,
        walks: true,
    });
}

#[test]
fn camera_adapter_consumes_every_tick_independently_of_frame_cadence() {
    let mut batched = camera_app();
    let mut frames = camera_app();
    for index in 0..4 {
        tick(&mut batched, index);
        tick(&mut frames, index);
        frames.world_mut().run_system_cached(sample).unwrap();
    }
    batched.world_mut().run_system_cached(sample).unwrap();
    assert_eq!(
        batched.world().resource::<LegacyCameraMotion>().state,
        frames.world().resource::<LegacyCameraMotion>().state
    );
}

#[test]
fn native_frame_bob_receives_the_real_dead_gate() {
    let mut app = camera_app();
    event(
        &mut app.world_mut().resource_mut::<TestWorld>().0,
        2,
        ActorStatusKind::Death,
    );
    tick(&mut app, 1);
    let mut bob = WalkBobState::default();
    bob.advance(Vec3::ZERO, true, true, 0.05);
    bob.advance(Vec3::new(0.2, 0.0, 0.0), true, true, 0.05);
    let previous = bob.bob();
    app.insert_resource(bob)
        .insert_resource(LocalViewPose::new(Vec3::new(0.4, 0.0, 0.0), Quat::IDENTITY));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(0.05));
    app.world_mut().run_system_cached(sample).unwrap();
    assert!(app.world().resource::<WalkBobState>().bob() < previous);
}

/// A crouched first-person camera stays on the gameplay eye that picks blocks, in both animation modes.
#[test]
fn crouched_camera_stays_on_the_interaction_eye() {
    for java in [false, true] {
        let mut app = camera_app();
        let mut user = ui::UserSettings::default();
        user.video.java_animations = java;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(1, &user)
            .unwrap();
        app.world_mut().resource_mut::<TestPhysics>().sneaking = true;
        let feet = Vec3::new(0.0, 64.0, 0.0);
        let eye = feet + Vec3::Y * (protocol::PLAYER_NETWORK_OFFSET - 0.35);
        app.world_mut()
            .resource_mut::<LocalViewPose>()
            .set_subject_position(eye, feet);
        let camera = app
            .world_mut()
            .spawn((FlyCamera::default(), Transform::from_translation(eye)))
            .id();
        for index in 0..4 {
            tick(&mut app, index);
            app.world_mut().run_system_cached(sample).unwrap();
            *app.world_mut().get_mut::<Transform>(camera).unwrap() =
                Transform::from_translation(eye);
            app.world_mut().run_system_cached(present).unwrap();
            let rendered = app.world().get::<Transform>(camera).unwrap().translation;
            assert!(
                rendered.abs_diff_eq(
                    app.world().resource::<LocalViewPose>().eye_translation(),
                    1e-5
                ),
                "java {java}, tick {index}: {rendered} != {eye}"
            );
        }
    }
}

#[test]
fn java_poses_keep_one_legacy_effect_for_world_and_hand_without_changing_aim() {
    for java in [false, true] {
        let mut app = camera_app();
        let mut user = ui::UserSettings::default();
        user.video.java_animations = java;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(1, &user)
            .unwrap();
        for index in 0..4 {
            tick(&mut app, index);
        }
        app.world_mut().run_system_cached(sample).unwrap();
        event(
            &mut app.world_mut().resource_mut::<TestWorld>().0,
            2,
            ActorStatusKind::Hurt,
        );
        app.world_mut()
            .resource_mut::<TestWorld>()
            .0
            .advance_actor_interpolation_frame(5);
        let base = Transform::from_xyz(3.0, 6.0, -8.0).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.7,
            -0.4,
            0.0,
        ));
        let camera = app.world_mut().spawn((FlyCamera::default(), base)).id();
        let aim = *app.world().resource::<LocalViewPose>();
        let physics = app.world().resource::<TestPhysics>().state.clone();
        app.world_mut().run_system_cached(apply).unwrap();
        let hand = app.world().resource::<FirstPersonHandMotion>();
        let shared = hand.legacy_effect.unwrap();
        let rendered = app
            .world()
            .get::<Transform>(camera)
            .unwrap()
            .to_matrix()
            .inverse();
        assert!(rendered.abs_diff_eq(shared * base.to_matrix().inverse(), 3e-5));
        assert_eq!(hand.bob, crate::camera::ViewEffect::NONE);
        assert_eq!(hand.hurt, Mat4::IDENTITY);
        assert_eq!(*app.world().resource::<LocalViewPose>(), aim);
        assert_eq!(app.world().resource::<TestPhysics>().state, physics);
        let endpoints = app.world().resource::<LegacyCameraMotion>().state.endpoints;
        let expected = render_model::legacy_combat::legacy_hurt_matrix(5, 0.0, None, 0.5, 1.0)
            * endpoints.bob_matrix(0.5);
        assert!(shared.abs_diff_eq(expected, 3e-6));
        user.video.view_bobbing = ui::ViewBobbing::Off;
        user.video.damage_bob = 0.0;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(2, &user)
            .unwrap();
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = base;
        app.world_mut().run_system_cached(apply).unwrap();
        assert_eq!(
            app.world()
                .resource::<FirstPersonHandMotion>()
                .legacy_effect,
            Some(Mat4::IDENTITY)
        );
    }
}

/// Minimal retains the Full hand effect while the world receives only hurt/death motion.
#[test]
fn minimal_bobbing_keeps_the_hand_moving_without_bobbing_the_camera() {
    for java in [false, true] {
        for hurt in [false, true] {
            let mut app = camera_app();
            let mut user = ui::UserSettings::default();
            user.video.java_animations = java;
            for index in 0..4 {
                tick(&mut app, index);
            }
            if hurt {
                event(
                    &mut app.world_mut().resource_mut::<TestWorld>().0,
                    2,
                    ActorStatusKind::Hurt,
                );
                app.world_mut()
                    .resource_mut::<TestWorld>()
                    .0
                    .advance_actor_interpolation_frame(5);
            }
            let base = Transform::from_xyz(3.0, 6.0, -8.0).with_rotation(Quat::from_euler(
                EulerRot::YXZ,
                0.7,
                -0.4,
                0.0,
            ));
            let camera = app.world_mut().spawn((FlyCamera::default(), base)).id();
            let aim = *app.world().resource::<LocalViewPose>();
            let mut full_hand = None;
            for (generation, mode) in [
                ui::ViewBobbing::Full,
                ui::ViewBobbing::Minimal,
                ui::ViewBobbing::Off,
            ]
            .into_iter()
            .enumerate()
            {
                user.video.view_bobbing = mode;
                app.world_mut()
                    .resource_mut::<CameraSettingsAuthority>()
                    .replace(generation as u64 + 1, &user)
                    .unwrap();
                app.world_mut().run_system_cached(sample).unwrap();
                *app.world_mut().get_mut::<Transform>(camera).unwrap() = base;
                app.world_mut().run_system_cached(apply).unwrap();
                let hand = app.world().resource::<FirstPersonHandMotion>();
                let effect = hand.legacy_effect.unwrap();
                let endpoints = app.world().resource::<LegacyCameraMotion>().state.endpoints;
                let bob = endpoints.bob_matrix(0.5);
                assert_ne!(bob, Mat4::IDENTITY);
                let damage = if hurt {
                    render_model::legacy_combat::legacy_hurt_matrix(5, 0.0, None, 0.5, 1.0)
                } else {
                    Mat4::IDENTITY
                };
                match mode {
                    ui::ViewBobbing::Full => {
                        assert!(effect.abs_diff_eq(damage * bob, 3e-6));
                        full_hand = Some(effect);
                    }
                    ui::ViewBobbing::Minimal => assert_eq!(Some(effect), full_hand),
                    ui::ViewBobbing::Off => assert!(effect.abs_diff_eq(damage, 3e-6)),
                }
                let camera_effect = if mode.camera_enabled() {
                    damage * bob
                } else {
                    damage
                };
                let rendered = app
                    .world()
                    .get::<Transform>(camera)
                    .unwrap()
                    .to_matrix()
                    .inverse();
                assert!(rendered.abs_diff_eq(camera_effect * base.to_matrix().inverse(), 3e-5));
                assert_eq!(*app.world().resource::<LocalViewPose>(), aim);
            }
        }
    }
}

#[test]
fn third_person_and_free_server_camera_preserve_their_base_pose() {
    let mut app = camera_app();
    tick(&mut app, 0);
    tick(&mut app, 1);
    app.world_mut().run_system_cached(sample).unwrap();
    let mut user = ui::UserSettings::default();
    user.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonBack;
    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .replace(1, &user)
        .unwrap();
    let base = Transform::from_xyz(2.0, 5.0, 8.0);
    let camera = app.world_mut().spawn((FlyCamera::default(), base)).id();
    app.world_mut().run_system_cached(apply).unwrap();
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), base);
    assert_eq!(
        app.world()
            .resource::<FirstPersonHandMotion>()
            .legacy_effect,
        None
    );
    user.gameplay.default_perspective = semantic_input::PerspectiveMode::FirstPerson;
    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .replace(2, &user)
        .unwrap();
    let context = crate::camera::ViewContext {
        base,
        subject: base,
        base_fov: 60.0,
        actors: &|_| None,
    };
    let mut server = ServerCameraView::default();
    server.apply(
        1,
        &protocol::CameraEvent::Presets(
            vec![protocol::CameraPreset {
                name: "minecraft:free".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    server.apply(
        2,
        &protocol::CameraEvent::Instruction(Box::new(protocol::CameraInstructionEvent {
            set: Some(protocol::CameraSetInstruction {
                preset_id: 0,
                ease: None,
                position: None,
                rotation_degrees: None,
                facing_position: None,
                view_offset: None,
                entity_offset: None,
                default_preset: None,
                remove_ignore_starting_values: false,
            }),
            ..Default::default()
        })),
        &context,
    );
    assert!(!server.renders_first_person(true));
    app.insert_resource(server);
    app.world_mut().run_system_cached(apply).unwrap();
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), base);
    assert_eq!(
        app.world()
            .resource::<FirstPersonHandMotion>()
            .legacy_effect,
        None
    );
}

#[test]
fn teleport_reanchor_skips_old_ticks_even_when_the_physics_clock_does_not_advance() {
    let mut app = camera_app();
    for index in 0..4 {
        tick(&mut app, index);
    }
    app.world_mut().run_system_cached(sample).unwrap();
    assert!(
        app.world()
            .resource::<LegacyCameraMotion>()
            .state
            .endpoints
            .bob[1]
            > 0.0
    );
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .reanchor_camera();
    app.world_mut().run_system_cached(sample).unwrap();
    let motion = app.world().resource::<LegacyCameraMotion>();
    assert_eq!(motion.state.last_tick(), Some(3));
    assert_eq!(
        motion.state.endpoints,
        render_model::legacy_combat::LegacyCameraEndpoints::default()
    );
}

#[test]
fn reanchored_bob_does_not_rewind_when_the_actor_fraction_wraps() {
    let mut app = camera_app();
    for index in 0..4 {
        tick(&mut app, index);
    }
    app.world_mut().run_system_cached(sample).unwrap();
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .reanchor_camera();
    app.world_mut().resource_mut::<TestPhysics>().alpha = 0.0;
    app.world_mut().run_system_cached(sample).unwrap();
    assert_eq!(
        app.world().resource::<LegacyCameraMotion>().state.endpoints,
        render_model::legacy_combat::LegacyCameraEndpoints::default()
    );
    tick(&mut app, 4);
    app.world_mut().run_system_cached(sample).unwrap();
    app.world_mut().resource_mut::<TestPhysics>().alpha = 0.75;
    let endpoints = app.world().resource::<LegacyCameraMotion>().state.endpoints;
    let rules = render_api::BEDROCK_112;
    assert_eq!(
        endpoints.walked,
        [0.0, rules.walk_gain * WALK_BLOCKS_PER_TICK]
    );
    assert_eq!(endpoints.bob, [0.0, rules.bob_follow * rules.bob_cap]);
    assert_eq!(endpoints.fall_degrees, [0.0; 2]);
    let expected = endpoints.bob_matrix(0.75);
    assert_ne!(expected, Mat4::IDENTITY);
    assert!(!expected.abs_diff_eq(endpoints.bob_matrix(0.0), 1e-5));
    let base = Transform::from_xyz(3.0, 5.0, 7.0).with_rotation(Quat::from_rotation_y(0.7));
    let camera = app.world_mut().spawn((FlyCamera::default(), base)).id();
    // Physics has no new completed tick while the independent actor fraction wraps.
    for actor_fraction in [0.75, 0.0, 0.25] {
        app.world_mut().resource_mut::<ActorFramePartialTick>().0 = actor_fraction;
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = base;
        app.world_mut().run_system_cached(apply).unwrap();
        assert_eq!(
            app.world()
                .resource::<FirstPersonHandMotion>()
                .legacy_effect,
            Some(expected)
        );
        let view = app
            .world()
            .get::<Transform>(camera)
            .unwrap()
            .to_matrix()
            .inverse();
        assert!(view.abs_diff_eq(expected * base.to_matrix().inverse(), 3e-5));
    }
    assert_eq!(app.world().resource::<TestPhysics>().state.tick, 4);
    assert_eq!(
        app.world().resource::<LegacyCameraMotion>().state.endpoints,
        endpoints
    );
}

#[test]
fn shared_hand_sway_uses_java_ticks_with_either_animation_option_and_bob_preference() {
    for java_mode in [false, true] {
        for view_bobbing in ui::ViewBobbing::ALL {
            let mut app = camera_app();
            let mut user = ui::UserSettings::default();
            user.video.java_animations = java_mode;
            user.video.view_bobbing = view_bobbing;
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .replace(1, &user)
                .unwrap();
            tick(&mut app, 1);
            app.world_mut().run_system_cached(sample).unwrap();
            tick(&mut app, 2);
            app.insert_resource(LocalViewPose::new(
                Vec3::ZERO,
                Quat::from_euler(
                    EulerRot::YXZ,
                    20.0_f32.to_radians(),
                    -40.0_f32.to_radians(),
                    0.0,
                ),
            ));
            app.world_mut().run_system_cached(sample).unwrap();
            let motion = app.world().resource::<FirstPersonHandMotion>();
            assert!((motion.sway_pitch_radians.to_degrees() - 3.0).abs() < 1e-5);
            assert!((motion.sway_yaw_radians.to_degrees() + 1.5).abs() < 1e-5);
            let camera = *app.world().resource::<JavaCameraState>();
            app.world_mut().run_system_cached(sample).unwrap();
            assert_eq!(*app.world().resource::<JavaCameraState>(), camera);
        }
    }
}

/// The hand follows complete fast turns at 20, 60 and 240 rendered frames per second.
#[test]
fn shared_hand_sway_retains_turn_direction_at_high_and_low_frame_rates() {
    for java_mode in [false, true] {
        for frames_per_tick in [1, 3, 12] {
            let mut app = camera_app();
            let mut user = ui::UserSettings::default();
            user.video.java_animations = java_mode;
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .replace(1, &user)
                .unwrap();
            tick(&mut app, 1);
            app.world_mut().run_system_cached(sample).unwrap();
            let mut previous = Vec2::ZERO;
            let mut current = Vec2::ZERO;
            let mut from = Vec2::ZERO;
            for (index, to) in [
                Vec2::new(80.0, 240.0),
                Vec2::new(-80.0, 480.0),
                Vec2::new(0.0, 720.0),
                Vec2::new(-80.0, 480.0),
                Vec2::new(80.0, 240.0),
                Vec2::ZERO,
            ]
            .into_iter()
            .enumerate()
            {
                for frame in 1..=frames_per_tick {
                    let look = from.lerp(to, frame as f32 / frames_per_tick as f32);
                    app.world_mut()
                        .resource_mut::<LocalViewPose>()
                        .turn_look_rotation(
                            Quat::from_euler(
                                EulerRot::YXZ,
                                -look.y.to_radians(),
                                -look.x.to_radians(),
                                0.0,
                            ),
                            -(to.y - from.y).to_radians() / frames_per_tick as f32,
                        );
                    if frame == frames_per_tick {
                        tick(&mut app, index as u64 + 2);
                        previous = current;
                        current += (look - current) * 0.5;
                    }
                    let alpha = (frame % frames_per_tick) as f32 / frames_per_tick as f32;
                    app.world_mut().resource_mut::<TestPhysics>().alpha = alpha;
                    app.world_mut().run_system_cached(sample).unwrap();
                    let motion = app.world().resource::<FirstPersonHandMotion>();
                    let expected = (look - previous.lerp(current, alpha)) * 0.1;
                    let expected = Mat4::from_rotation_x(expected.x.to_radians())
                        * Mat4::from_rotation_y(expected.y.to_radians());
                    let actual = Mat4::from_rotation_x(motion.sway_pitch_radians)
                        * Mat4::from_rotation_y(motion.sway_yaw_radians);
                    assert!(
                        actual.abs_diff_eq(expected, 3e-6),
                        "Java {java_mode}, frames per tick {frames_per_tick}, tick {index}, frame {frame}"
                    );
                }
                from = to;
            }
        }
    }
}

/// Returning from freelook adds only the physical return angle to settled hand sway.
#[test]
fn freelook_release_preserves_hand_sway_winding() {
    for java_mode in [false, true] {
        for (turn, return_angle) in [
            (360.0_f32, 0.0_f32),
            (-720.0, 0.0),
            (405.0, -45.0),
            (-405.0, 45.0),
        ] {
            let mut app = camera_app();
            let mut user = ui::UserSettings::default();
            user.video.java_animations = java_mode;
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .replace(1, &user)
                .unwrap();
            let actor_yaw = 135.0_f32;
            app.insert_resource(LocalViewPose::new(
                Vec3::ZERO,
                Quat::from_rotation_y(-actor_yaw.to_radians()),
            ));
            let actor_rotation = app.world().resource::<LocalViewPose>().rotation();
            tick(&mut app, 1);
            app.world_mut().run_system_cached(sample).unwrap();
            {
                let mut view = app.world_mut().resource_mut::<LocalViewPose>();
                view.set_freelook(true);
                view.turn_look_rotation(
                    Quat::from_rotation_y(-(actor_yaw + turn).to_radians()),
                    -turn.to_radians(),
                );
            }
            for index in 2..=34 {
                tick(&mut app, index);
                app.world_mut().run_system_cached(sample).unwrap();
            }
            assert!(
                app.world()
                    .resource::<FirstPersonHandMotion>()
                    .sway_yaw_radians
                    .abs()
                    < 2e-6
            );
            app.world_mut()
                .resource_mut::<LocalViewPose>()
                .set_freelook(false);
            let view = *app.world().resource::<LocalViewPose>();
            assert_eq!(view.rotation(), actor_rotation);
            assert_eq!(view.camera_rotation(), actor_rotation);
            for alpha in [0.0, 0.5, 1.0] {
                app.world_mut().resource_mut::<TestPhysics>().alpha = alpha;
                app.world_mut().run_system_cached(sample).unwrap();
                let actual = app
                    .world()
                    .resource::<FirstPersonHandMotion>()
                    .sway_yaw_radians;
                let expected = (return_angle * 0.1).to_radians();
                assert!(
                    (actual - expected).abs() < 2e-6,
                    "Java {java_mode}, turn {turn}, alpha {alpha}: {actual} != {expected}"
                );
            }
            {
                let mut view = app.world_mut().resource_mut::<LocalViewPose>();
                view.set_freelook(false);
                view.turn_look_rotation(
                    Quat::from_rotation_y(-(actor_yaw + 30.0).to_radians()),
                    -30.0_f32.to_radians(),
                );
            }
            app.world_mut().run_system_cached(sample).unwrap();
            let actual = app
                .world()
                .resource::<FirstPersonHandMotion>()
                .sway_yaw_radians;
            assert!((actual - ((return_angle + 30.0) * 0.1).to_radians()).abs() < 2e-6);
        }
    }
}
