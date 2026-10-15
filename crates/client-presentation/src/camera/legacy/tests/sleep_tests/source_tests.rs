use super::*;

/// Starts with an awake input pose and a loaded north-facing bed, without an actor tick.
fn pose_app() -> App {
    let mut app = camera_app();
    let collisions = bed_collisions(&mut app, BED, 2);
    app.insert_resource(collisions);
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .set_subject_position(Vec3::Y * protocol::STANDING_PLAYER_EYE_HEIGHT, Vec3::ZERO);
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .set_rotation(Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 0.0));
    app
}

/// Builds the same retained actor feed that presentation adjusts from the shared view.
fn actor_feed(app: &App) -> client_world::LocalPlayerFeed {
    let view = app.world().resource::<LocalViewPose>();
    let mut feed = crate::actor_feed::build_local_player_feed(
        app.world().resource::<TestPhysics>(),
        view.rotation(),
        false,
        false,
        [1; 16],
        || protocol::PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        client_world::LocalItemUse::Unpredicted,
    )
    .unwrap();
    view.apply_actor_presentation(&mut feed);
    feed
}

#[test]
fn loaded_bed_heading_is_ready_on_the_first_metadata_frame_without_actor_ticks() {
    let mut app = pose_app();
    sleep_state(&mut app, 4, true, Some(BED));
    assert_eq!(
        app.world()
            .resource::<TestWorld>()
            .0
            .authority()
            .actor(2)
            .unwrap()
            .status
            .sleep_rotation,
        None
    );
    app.world_mut()
        .run_system_cached(resolve_presentation)
        .unwrap();
    let view = app.world().resource::<LocalViewPose>();
    assert_eq!(
        view.camera_rotation(),
        crate::local_player::sleeping_view_rotation(270.0)
    );
    assert_eq!(
        app.world()
            .resource::<TestWorld>()
            .0
            .authority()
            .actor(2)
            .unwrap()
            .status
            .sleep_rotation,
        Some(270.0)
    );
}

#[test]
fn actor_camera_heading_reads_the_sleeping_presentation_without_replacing_input_look() {
    let mut app = pose_app();
    let input = app.world().resource::<LocalViewPose>().look_rotation();
    sleep_state(&mut app, 4, true, Some(BED));
    app.world_mut()
        .run_system_cached(resolve_presentation)
        .unwrap();
    let view = app.world().resource::<LocalViewPose>();
    assert_ne!(view.camera_rotation(), input);
    assert_eq!(
        view.camera_rotation(),
        crate::local_player::sleeping_view_rotation(270.0)
    );
    assert_eq!(view.look_rotation(), input);
    assert_eq!(view.rotation(), input);
}

#[test]
fn metadata_only_sleep_and_wake_reanchor_once_while_stable_sleep_keeps_history() {
    let mut app = pose_app();
    let start = app
        .world()
        .resource::<LocalViewPose>()
        .camera_reanchor_epoch();
    sleep_state(&mut app, 4, true, Some(BED));
    app.world_mut()
        .run_system_cached(resolve_presentation)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<LocalViewPose>()
            .camera_reanchor_epoch(),
        start + 1
    );
    app.world_mut()
        .run_system_cached(resolve_presentation)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<LocalViewPose>()
            .camera_reanchor_epoch(),
        start + 1
    );
    sleep_state(&mut app, 5, false, Some(BED));
    app.world_mut()
        .run_system_cached(resolve_presentation)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<LocalViewPose>()
            .camera_reanchor_epoch(),
        start + 2
    );
}

#[test]
fn sleep_and_wake_clear_motion_blur_once_without_moving_physics() {
    let mut app = pose_app();
    let position = sim::Vec3::new(2.5, 2.0, 5.5);
    app.world_mut().resource_mut::<TestPhysics>().state.position = position;
    let feet = Vec3::new(position.x as f32, position.y as f32, position.z as f32);
    let eye = feet + Vec3::Y * protocol::STANDING_PLAYER_EYE_HEIGHT;
    {
        let mut view = app.world_mut().resource_mut::<LocalViewPose>();
        view.set_subject_position(eye, feet);
        view.set_rotation(Quat::IDENTITY);
    }
    app.add_systems(Update, crate::camera::motion_blur::apply_camera_motion_blur);
    let camera = app
        .world_mut()
        .spawn((FlyCamera::default(), Camera3d::default()))
        .id();
    let mut settings = ui::UserSettings::default();
    settings.gameplay.default_perspective = PerspectiveMode::ThirdPersonBack;
    settings.video.motion_blur = ui::MotionBlurQuality::High;
    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .replace(1, &settings)
        .unwrap();
    let mut epoch = 0;
    for (index, (sleeping, transition)) in [
        (false, false),
        (false, false),
        (true, true),
        (true, false),
        (false, true),
        (false, false),
    ]
    .into_iter()
    .enumerate()
    {
        sleep_state(&mut app, index as u64 + 4, sleeping, Some(BED));
        app.world_mut()
            .run_system_cached(resolve_presentation)
            .unwrap();
        let view = *app.world().resource::<LocalViewPose>();
        if transition {
            epoch += 1;
        }
        assert_eq!(view.camera_reanchor_epoch(), epoch);
        assert_eq!(view.rotation(), Quat::IDENTITY);
        let expected = if sleeping {
            crate::local_player::sleeping_subject_position(BED).0
        } else {
            eye
        };
        assert_eq!(view.eye_translation(), expected);
        app.update();
        let blur = app
            .world()
            .get::<render_scene::CameraMotionBlur>(camera)
            .unwrap();
        if transition {
            assert_eq!(blur.exposure_seconds, 0.0);
        } else if epoch > 0 {
            assert!(blur.exposure_seconds > 0.0);
        }
        assert_eq!(
            app.world().resource::<TestPhysics>().state.position,
            position
        );
    }
}

#[test]
fn retained_actor_attachment_origin_snaps_to_the_bed_and_back_without_actor_ticks() {
    let mut app = pose_app();
    let mut initial = actor_feed(&app);
    initial.teleported = true;
    app.world_mut()
        .resource_mut::<TestWorld>()
        .0
        .sync_local_player_pose(&initial);
    let revision = app
        .world()
        .resource::<TestWorld>()
        .0
        .authority()
        .actor(2)
        .unwrap()
        .spawn_revision;
    let (_, feet) = crate::local_player::sleeping_subject_position(BED);
    for (sequence, sleeping) in [(4, true), (5, true), (6, false)] {
        sleep_state(&mut app, sequence, sleeping, Some(BED));
        app.world_mut()
            .run_system_cached(resolve_presentation)
            .unwrap();
        {
            let world = app.world().resource::<TestWorld>();
            let actor = world.0.authority().actor(2).unwrap();
            let expected = if sleeping {
                feet.to_array()
            } else {
                initial.position
            };
            assert_eq!(
                actor.position, expected,
                "camera actor-pose readers see the source immediately"
            );
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
                    attachment.position, expected,
                    "attachments do not wait for actor preparation"
                );
            }
        }
        let feed = actor_feed(&app);
        let mut world = app.world_mut().resource_mut::<TestWorld>();
        world.0.sync_local_player_pose(&feed);
        let actor = world.0.authority().actor(2).unwrap();
        assert_eq!(
            actor.spawn_revision, revision,
            "bound effects keep the same actor identity"
        );
        for alpha in [0.0, 0.5, 1.0] {
            let attachment =
                crate::presentation::actors::actor_emitter_transform(actor, None, &[], None, alpha)
                    .unwrap();
            assert_eq!(
                attachment.position,
                if sleeping {
                    feet.to_array()
                } else {
                    initial.position
                }
            );
        }
    }
}

#[test]
fn actor_positions_preserve_awake_predictions_and_server_fallbacks_with_an_independent_view() {
    let mut app = pose_app();
    let camera_feet = Vec3::new(90.0, 100.0, 110.0);
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .set_subject_position(camera_feet + Vec3::Y, camera_feet);
    let predicted_eye = Some([10.0, 21.0, 30.0]);
    let predicted_feet = Some([10.0, 20.0, 30.0]);
    let (bed_eye, bed_feet) = crate::local_player::sleeping_subject_position(BED);
    let mut avatar = crate::local_player::LocalAvatarPresentation::default();
    avatar.begin_session(SESSION, 2);
    let mut visibility = crate::local_player::LocalAvatarVisibilityCarrier::default();
    for (sequence, sleeping) in [(4, false), (5, true), (6, false)] {
        sleep_state(&mut app, sequence, sleeping, Some(BED));
        app.world_mut()
            .run_system_cached(resolve_presentation)
            .unwrap();
        let view = app.world().resource::<LocalViewPose>();
        for eye in [predicted_eye, None] {
            for feet in [predicted_feet, None] {
                let positions = view.actor_positions(eye, feet);
                assert_eq!(
                    positions,
                    if sleeping {
                        (Some(bed_eye.to_array()), Some(bed_feet.to_array()))
                    } else {
                        (eye, feet)
                    }
                );
                let server_eye = app
                    .world()
                    .resource::<TestWorld>()
                    .0
                    .resolved_server_position()
                    .position;
                let actor_eye = crate::actor_clock::authoritative_local_actor_eye(
                    positions.0,
                    Some(server_eye),
                );
                let actor_feet = positions.1.map(Vec3::from_array).unwrap_or_else(|| {
                    Vec3::from_array(server_eye) - Vec3::Y * protocol::PLAYER_NETWORK_OFFSET
                });
                crate::actor_clock::publish_local_actor_visibility(
                    &avatar,
                    PerspectiveMode::ThirdPersonBack,
                    None,
                    actor_eye,
                    Some(actor_feet),
                    view.rotation(),
                    &mut visibility,
                );
                let body = visibility.snapshot().unwrap();
                assert!(body.visible());
                assert_eq!(
                    body.eye(),
                    if sleeping {
                        bed_eye
                    } else {
                        Vec3::from_array(eye.unwrap_or(server_eye))
                    }
                );
                assert_eq!(
                    body.feet(),
                    if sleeping {
                        bed_feet
                    } else {
                        feet.map(Vec3::from_array).unwrap_or_else(|| {
                            Vec3::from_array(server_eye) - Vec3::Y * protocol::PLAYER_NETWORK_OFFSET
                        })
                    }
                );
            }
        }
        if !sleeping {
            assert_eq!(view.feet_translation(), camera_feet);
            assert_eq!(actor_feed(&app).position, [0.0; 3]);
        }
    }
}

#[test]
fn waking_attachments_restore_physics_or_server_feet_instead_of_the_free_camera() {
    for predicted_feet in [Some([10.0, 20.0, 30.0]), None] {
        let mut app = pose_app();
        let camera_feet = Vec3::new(90.0, 100.0, 110.0);
        app.world_mut()
            .resource_mut::<LocalViewPose>()
            .set_subject_position(camera_feet + Vec3::Y, camera_feet);
        sleep_state(&mut app, 4, true, Some(BED));
        app.world_mut()
            .run_system_cached(resolve_presentation)
            .unwrap();
        sleep_state(&mut app, 5, false, Some(BED));
        let TestWorld(mut stream) = app.world_mut().remove_resource::<TestWorld>().unwrap();
        let collisions = app.world().resource::<TestCollisions>();
        let mut view = *app.world().resource::<LocalViewPose>();
        let expected = predicted_feet.unwrap_or_else(|| {
            let mut feet = stream.resolved_server_position().position;
            feet[1] -= protocol::PLAYER_NETWORK_OFFSET;
            feet
        });
        view.resolve_presentation(
            Some(&mut stream),
            collisions,
            PerspectiveMode::ThirdPersonBack,
            predicted_feet,
        );
        let actor = stream.authority().actor(2).unwrap();
        assert_eq!(actor.position, expected);
        for alpha in [0.0, 0.5, 1.0] {
            let attachment =
                crate::presentation::actors::actor_emitter_transform(actor, None, &[], None, alpha)
                    .unwrap();
            assert_eq!(attachment.position, expected);
        }
        view.set_subject_position(Vec3::splat(200.0), Vec3::splat(199.0));
        view.resolve_presentation(
            Some(&mut stream),
            collisions,
            PerspectiveMode::ThirdPersonBack,
            predicted_feet,
        );
        assert_eq!(stream.authority().actor(2).unwrap().position, expected);
    }
}

#[test]
fn changed_and_missing_bed_facts_refresh_without_actor_ticks() {
    let mut app = pose_app();
    sleep_state(&mut app, 4, true, Some(BED));
    for heading in [Some(270.0), None, Some(90.0)] {
        app.world_mut()
            .resource_mut::<TestCollisions>()
            .bed_rotation = heading;
        app.world_mut()
            .run_system_cached(resolve_presentation)
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<TestWorld>()
                .0
                .authority()
                .actor(2)
                .unwrap()
                .status
                .sleep_rotation,
            heading
        );
        assert_eq!(
            app.world().resource::<LocalViewPose>().camera_rotation(),
            crate::local_player::sleeping_view_rotation(heading.unwrap_or(0.0))
        );
    }
    // Move the sleep target outside resident terrain; an old registry fact cannot keep its heading.
    sleep_state(&mut app, 5, true, Some([BED[0] + 1024, BED[1], BED[2]]));
    app.world_mut()
        .run_system_cached(resolve_presentation)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<TestWorld>()
            .0
            .authority()
            .actor(2)
            .unwrap()
            .status
            .sleep_rotation,
        None
    );
    sleep_state(&mut app, 6, true, Some(BED));
    app.world_mut()
        .run_system_cached(resolve_presentation)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<TestWorld>()
            .0
            .authority()
            .actor(2)
            .unwrap()
            .status
            .sleep_rotation,
        Some(90.0)
    );
}
