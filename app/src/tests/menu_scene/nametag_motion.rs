//! Own labels follow the frozen body pose while remote labels retain actor interpolation.

use super::*;
use crate::ui_runtime::presentation::prepare_ui_runtime;
use client_presentation::environment::{WeatherState, WorldClock, bind_session_generation};
use client_presentation::{
    actor_publication::ActorFramePartialTick,
    camera::{CameraSettingsAuthority, FlyCamera, perspective_pose},
    local_player::{
        LocalAvatarPresentation, LocalAvatarVisibilityCarrier, LocalPlayerFrameCarrier,
        LocalPlayerFrameSample,
    },
};
use launcher::menu::settings_options::SettingId;
use protocol::identity::FifoSequence;
use protocol_ids::ActorFlag;
use semantic_input::PerspectiveMode;
use simulation_clock::{PhysicsTick, identity::SessionGeneration};

const OWN_COLOR: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const REMOTE_COLOR: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

/// Enables distinguishable own and remote labels in a current gameplay session.
fn fixture_world(perspective: PerspectiveMode) -> World {
    let mut world = super::hud_visibility::fixture_world();
    world.resource_scope(|world, mut clock: Mut<WorldClock>| {
        bind_session_generation(&mut clock, &mut world.resource_mut::<WeatherState>(), 1);
    });
    while world.resource::<CameraSettingsAuthority>().perspective() != perspective {
        world
            .resource_mut::<CameraSettingsAuthority>()
            .cycle_perspective();
    }
    let hud: ui::mod_hud::Hud = serde_json::from_value(serde_json::json!({
        "cards": [],
        "actor_style": {"nametags": {
            "show_self": true,
            "own_text_color": OWN_COLOR,
            "replace_own_text_color": true,
            "text_color": REMOTE_COLOR,
            "replace_text_color": true
        }}
    }))
    .unwrap();
    world
        .resource_mut::<UiPresentationResource>()
        .set_mod_hud(Some(&hud))
        .unwrap();
    assert_own_policy(&world);
    world
}

/// Ensures the fixture enables self tags and keeps the two text colors distinguishable.
fn assert_own_policy(world: &World) {
    let presentation = world.resource::<UiPresentationResource>();
    let style = presentation
        .mod_ui()
        .mod_actor_style()
        .and_then(|style| style.nametags.as_ref())
        .expect("self-tag cosmetic policy must remain published");
    assert!(style.show_self && !style.hide && !style.hide_players);
    assert_eq!(style.own_text_color, Some(OWN_COLOR));
    assert!(style.replace_own_text_color);
    assert_eq!(style.text_color, Some(REMOTE_COLOR));
    assert!(style.replace_text_color);
    assert_ne!(
        world.resource::<CameraSettingsAuthority>().perspective(),
        PerspectiveMode::FirstPerson
    );
    let menu = world.resource::<MenuRuntime>();
    assert!(!menu.is_visible());
    assert_ne!(
        menu.settings_snapshot()
            .0
            .value(SettingId::IngamePlayerNames),
        0
    );
    assert_eq!(menu.settings_snapshot().0.value(SettingId::HideHud), 0);
}

/// Feeds completed local simulation positions through the actor authority's normal input.
fn local_feed(position: Vec3) -> client_world::LocalPlayerFeed {
    client_world::LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [1; 16],
        username: "Tester".into(),
        skin: PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        position: position.to_array(),
        velocity: [0.0; 3],
        on_ground: true,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        held_network_ids: [0; 2],
        held_variants: Default::default(),
        main_hand_metadata: 0,
        main_hand_slot: 0,
        main_hand_stack_id: None,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        flying: false,
        gliding: false,
        spinning: false,
        fall_fly_ticks: 0,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

/// Starts a remote interpolation segment through committed movement events.
fn remote_move(position: Vec3, teleported: bool) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
        dimension: 0,
        runtime_id: 2,
        position: position.to_array().map(Some),
        position_origin: protocol::ActorPositionOrigin::Feet,
        pitch: None,
        yaw: None,
        head_yaw: None,
        on_ground: Some(true),
        teleported,
        player_mode: None,
        source_tick: None,
        interpolation: protocol::ActorInterpolation {
            ticks: 6,
            force_completion: false,
        },
    }))
}

/// Keeps both actor timelines moving independently of the local render-pose carrier.
fn prepare_actor_motion(world: &mut World) {
    let mut client = world.resource_mut::<ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    // The shared fixture contains a server-spawned actor, so its server flags must explicitly
    // admit names before prediction creates the primary flags word.
    stream
        .submit(
            4,
            WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: Arc::from([
                    protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::Flags.wire_value(),
                        value: protocol::ActorMetadataValue::Flags(
                            1 << ActorFlag::ShowName.wire_value(),
                        ),
                    },
                    protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::AlwaysShowNameTag.wire_value(),
                        value: protocol::ActorMetadataValue::Byte(1),
                    },
                ]),
                properties: Arc::from([]),
                tick: 0,
            })),
        )
        .unwrap();
    stream.poll([0.0, 64.0, 0.0], 0);
    stream.sync_local_player_pose(&local_feed(Vec3::new(30.0, 64.0, 4.0)));
    stream
        .submit(5, remote_move(Vec3::new(2.0, 64.0, 2.0), true))
        .unwrap();
    stream.advance_actor_interpolation_frame(1);
    stream.sync_local_player_pose(&local_feed(Vec3::new(34.0, 65.0, 5.0)));
    stream
        .submit(6, remote_move(Vec3::new(8.0, 66.0, 4.0), false))
        .unwrap();
    stream.advance_actor_interpolation_frame(1);
    for runtime_id in [1, 2] {
        let actor = stream.authority().actor(runtime_id).unwrap();
        assert_ne!(actor.previous_pose.position, actor.position);
    }
    assert!(matches!(
        stream.authority().actor(1).unwrap().metadata.get(&0),
        Some(protocol::ActorMetadataValue::Flags(flags))
            if flags & (1 << ActorFlag::ShowName.wire_value()) != 0
    ));
    assert!(
        stream
            .authority()
            .actor_name_tag(1)
            .is_some_and(|name| !name.is_empty())
    );
}

/// Checks the valid frozen body identity before attempting to publish its own tag.
fn assert_own_subject(world: &World) {
    let visibility = world
        .resource::<LocalAvatarVisibilityCarrier>()
        .snapshot()
        .expect("valid body pose must be published");
    assert!(visibility.visible());
    assert_eq!(
        visibility.session_generation().get(),
        world.resource::<WorldClock>().session_generation()
    );
    assert_eq!(
        visibility.runtime_id(),
        world
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .local_player_runtime_id()
    );
}

/// Publishes the render body's pose with an explicit session and subject identity.
fn publish_visibility(
    world: &mut World,
    perspective: PerspectiveMode,
    feet: Vec3,
    session: u64,
    runtime_id: u64,
) {
    world.resource_scope(|world, mut avatar: Mut<LocalAvatarPresentation>| {
        avatar.begin_session(SessionGeneration::new(session), runtime_id);
        avatar.publish_view_visibility(
            perspective,
            feet + Vec3::Y * protocol::PLAYER_NETWORK_OFFSET,
            feet,
            Quat::IDENTITY,
            &mut world.resource_mut::<LocalAvatarVisibilityCarrier>(),
        );
    });
}

/// Positions the camera at the body while retaining a deliberately different player frame.
fn prepare_local_frame(world: &mut World, perspective: PerspectiveMode, feet: Vec3, tick: u64) {
    prepare_local_frame_with_feet(
        world,
        perspective,
        feet,
        feet + Vec3::new(9.0, 3.0, -7.0),
        tick,
    );
}

/// Samples the render camera and player-frame viewer from independent feet positions.
fn prepare_local_frame_with_feet(
    world: &mut World,
    perspective: PerspectiveMode,
    feet: Vec3,
    stale_feet: Vec3,
    tick: u64,
) {
    let eye = feet + Vec3::Y * protocol::PLAYER_NETWORK_OFFSET;
    let rotation = Quat::from_rotation_y(0.4);
    let pose = perspective_pose(eye, rotation, perspective);
    let camera = world
        .query_filtered::<Entity, With<FlyCamera>>()
        .single(world)
        .unwrap();
    world.entity_mut(camera).insert(pose);
    let stale_eye = stale_feet + Vec3::Y * protocol::PLAYER_NETWORK_OFFSET;
    world
        .resource_mut::<LocalPlayerFrameCarrier>()
        .publish(LocalPlayerFrameSample {
            session_generation: SessionGeneration::new(1),
            actor_session_id: 1,
            fifo_sequence: FifoSequence::new(tick),
            physics_tick: PhysicsTick::new(tick),
            perspective,
            world_collision_identity: sim::CollisionQuery::synthetic(()).identity,
            pose: perspective_pose(stale_eye, rotation, perspective),
            eye: stale_eye,
            feet: stale_feet,
            rotation,
        })
        .unwrap();
}

/// Selects emitted text by its cosmetic color without relying on draw-pass ordering.
fn glyph_records(world: &mut World, color: [f32; 4]) -> Vec<render_model::NametagRecord> {
    assert_own_policy(world);
    world
        .resource_mut::<UiPresentationResource>()
        .nametag_scene()
        .records
        .into_iter()
        .filter(|record| record.text != 0 && record.color[..3] == color[..3])
        .collect()
}

/// Observes emitted glyph anchors without depending on glyph counts or draw-pass ordering.
fn glyph_anchors(world: &mut World, color: [f32; 4]) -> Vec<Vec3> {
    glyph_records(world, color)
        .into_iter()
        .map(|record| Vec3::from_array(record.anchor))
        .collect()
}

/// Checks every visible glyph's anchor against the selected body or actor pose.
fn assert_anchors(anchors: &[Vec3], expected: Vec3, context: &str) {
    assert!(!anchors.is_empty(), "{context}: expected visible text");
    assert!(
        anchors
            .iter()
            .all(|anchor| anchor.abs_diff_eq(expected, 1e-5)),
        "{context}: anchors {anchors:?}, expected {expected:?}"
    );
}

#[test]
fn own_nametag_tracks_render_feet_through_fractional_motion_in_both_third_person_views() {
    for perspective in [
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
    ] {
        let mut world = fixture_world(perspective);
        prepare_actor_motion(&mut world);
        let mut height_offset = None;
        let mut prior_remote = None;
        let mut remote_changed = false;
        // Walk, jump up/down, reverse direction, then accept a position correction.
        for (index, (feet, alpha)) in [
            (Vec3::new(-8.0, 64.0, 3.0), 0.1),
            (Vec3::new(-7.75, 64.0, 3.0), 0.75),
            (Vec3::new(-7.5, 64.7, 3.0), 0.25),
            (Vec3::new(-7.25, 65.5, 3.0), 0.9),
            (Vec3::new(-7.0, 64.8, 3.0), 0.4),
            (Vec3::new(-7.5, 64.0, 3.0), 0.6),
            (Vec3::new(-12.0, 63.25, -5.0), 0.15),
            (Vec3::new(-12.25, 63.25, -5.0), 0.8),
        ]
        .into_iter()
        .enumerate()
        {
            prepare_local_frame(&mut world, perspective, feet, index as u64 + 1);
            publish_visibility(&mut world, perspective, feet, 1, 1);
            assert_own_subject(&world);
            assert_eq!(
                world
                    .resource::<LocalAvatarVisibilityCarrier>()
                    .snapshot()
                    .unwrap()
                    .feet(),
                feet
            );
            assert_ne!(
                world
                    .resource::<LocalPlayerFrameCarrier>()
                    .snapshot()
                    .unwrap()
                    .feet(),
                feet
            );
            world.resource_mut::<ActorFramePartialTick>().0 = alpha;
            world.run_system_cached(prepare_ui_runtime).unwrap();
            let own = glyph_anchors(&mut world, OWN_COLOR);
            assert!(!own.is_empty(), "{perspective:?}, sample {index}");
            let offset = *height_offset.get_or_insert(own[0] - feet);
            assert!(offset.y > 0.0 && offset.x.abs() < 1e-5 && offset.z.abs() < 1e-5);
            assert_anchors(
                &own,
                feet + offset,
                &format!("{perspective:?}, own sample {index}"),
            );
            let client = world.resource::<ClientWorld>();
            let authority = client.stream.as_ref().unwrap().authority();
            let local_actor = authority.actor(1).unwrap();
            assert!(!feet.abs_diff_eq(Vec3::from_array(local_actor.previous_pose.position), 1.0));
            assert!(!feet.abs_diff_eq(Vec3::from_array(local_actor.position), 1.0));
            let remote_feet = Vec3::from_array(
                authority
                    .actor(2)
                    .unwrap()
                    .interpolated_position(alpha)
                    .unwrap(),
            );
            remote_changed |=
                prior_remote.is_some_and(|prior: Vec3| !prior.abs_diff_eq(remote_feet, 1e-5));
            prior_remote = Some(remote_feet);
            assert_anchors(
                &glyph_anchors(&mut world, REMOTE_COLOR),
                remote_feet + offset,
                &format!("{perspective:?}, remote sample {index}"),
            );
        }
        assert!(
            remote_changed,
            "fractional ticks must exercise remote motion"
        );
    }
}

#[test]
fn invalid_or_hidden_body_carrier_suppresses_only_the_own_nametag() {
    for perspective in [
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
    ] {
        let mut world = fixture_world(perspective);
        prepare_actor_motion(&mut world);
        let feet = Vec3::new(-8.0, 64.0, 3.0);
        prepare_local_frame(&mut world, perspective, feet, 1);
        world.resource_mut::<ActorFramePartialTick>().0 = 0.35;
        for invalid in [
            "missing",
            "hidden",
            "other runtime",
            "other session",
            "nonfinite",
        ] {
            publish_visibility(&mut world, perspective, feet, 1, 1);
            assert_own_subject(&world);
            world.run_system_cached(prepare_ui_runtime).unwrap();
            assert!(
                !glyph_anchors(&mut world, OWN_COLOR).is_empty(),
                "{perspective:?}, restore before {invalid}"
            );
            let remote = glyph_anchors(&mut world, REMOTE_COLOR);
            assert!(!remote.is_empty());
            match invalid {
                "missing" => world.resource_mut::<LocalAvatarVisibilityCarrier>().clear(),
                "hidden" => {
                    publish_visibility(&mut world, PerspectiveMode::FirstPerson, feet, 1, 1)
                }
                "other runtime" => publish_visibility(&mut world, perspective, feet, 1, 9),
                "other session" => publish_visibility(&mut world, perspective, feet, 2, 1),
                "nonfinite" => publish_visibility(
                    &mut world,
                    perspective,
                    Vec3::new(f32::NAN, 64.0, 3.0),
                    1,
                    1,
                ),
                _ => unreachable!(),
            }
            world.run_system_cached(prepare_ui_runtime).unwrap();
            assert!(
                glyph_anchors(&mut world, OWN_COLOR).is_empty(),
                "{perspective:?}, {invalid} body carrier"
            );
            assert_eq!(
                glyph_anchors(&mut world, REMOTE_COLOR),
                remote,
                "{perspective:?}, {invalid} must preserve remote tags"
            );
        }
    }
}

#[test]
fn own_score_remains_visible_after_body_correction_with_stale_player_frame() {
    for perspective in [
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
    ] {
        let mut world = fixture_world(perspective);
        prepare_actor_motion(&mut world);
        let feet = Vec3::new(180.0, 64.0, -120.0);
        let stale_feet = Vec3::new(0.0, 64.0, 0.0);
        assert!(feet.distance(stale_feet) > 64.0);
        prepare_local_frame_with_feet(&mut world, perspective, feet, stale_feet, 1);
        publish_visibility(&mut world, perspective, feet, 1, 1);
        assert_own_subject(&world);
        world.resource_mut::<ActorFramePartialTick>().0 = 0.35;
        world.run_system_cached(prepare_ui_runtime).unwrap();
        let own = glyph_records(&mut world, OWN_COLOR);
        assert!(
            !own.is_empty(),
            "{perspective:?}: corrected body must retain its own label"
        );
        let offset = Vec3::from_array(own[0].anchor) - feet;
        assert!(offset.y > 0.0 && offset.x.abs() < 1e-5 && offset.z.abs() < 1e-5);
        assert!(own.iter().all(|record| record.line_lift == 0.0));
        let remote = glyph_anchors(&mut world, REMOTE_COLOR);
        assert!(
            !remote.is_empty(),
            "remote range must keep using the player-frame viewer"
        );

        // The server's synced score text is the only additional line in this plain-name fixture.
        {
            let mut client = world.resource_mut::<ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            stream
                .submit(
                    7,
                    WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                        dimension: 0,
                        runtime_id: 1,
                        metadata: Arc::from([protocol::ActorMetadata {
                            key: 84,
                            value: protocol::ActorMetadataValue::String("7 wins".into()),
                        }]),
                        properties: Arc::from([]),
                        tick: 0,
                    })),
                )
                .unwrap();
            stream.poll(stale_feet.to_array(), 0);
            stream.advance_actor_interpolation_frame(0);
            assert!(matches!(
                stream.authority().actor(1).unwrap().metadata.get(&84),
                Some(protocol::ActorMetadataValue::String(score)) if score.as_ref() == "7 wins"
            ));
        }
        world.run_system_cached(prepare_ui_runtime).unwrap();
        let scored = glyph_records(&mut world, OWN_COLOR);
        assert_anchors(
            &scored
                .iter()
                .map(|record| Vec3::from_array(record.anchor))
                .collect::<Vec<_>>(),
            feet + offset,
            &format!("{perspective:?}, corrected own score"),
        );
        assert!(
            scored.iter().all(|record| record.line_lift > 0.0),
            "{perspective:?}: score range must use the current camera"
        );
        assert!(
            scored
                .iter()
                .any(|record| record.rect[1] != scored[0].rect[1]),
            "{perspective:?}: name and synced score must occupy separate text rows"
        );
        assert_eq!(glyph_anchors(&mut world, REMOTE_COLOR), remote);
    }
}

/// Commits one metadata update without advancing the actor's rendered pose.
fn set_tag_metadata(
    world: &mut World,
    runtime_id: u64,
    key: protocol_ids::ActorDataKey,
    value: protocol::ActorMetadataValue,
    feet: Vec3,
) {
    let mut client = world.resource_mut::<ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    stream
        .submit(
            stream.committed_sequence() + 1,
            WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id,
                metadata: Arc::from([protocol::ActorMetadata {
                    key: key.wire_value(),
                    value,
                }]),
                properties: Arc::from([]),
                tick: 0,
            })),
        )
        .unwrap();
    stream.poll(feet.to_array(), 0);
}

#[test]
fn empty_own_name_tag_does_not_fall_back_to_the_username() {
    let perspective = PerspectiveMode::ThirdPersonBack;
    let mut world = fixture_world(perspective);
    prepare_actor_motion(&mut world);
    let feet = Vec3::new(0.0, 64.0, 0.0);
    prepare_local_frame(&mut world, perspective, feet, 1);
    publish_visibility(&mut world, perspective, feet, 1, 1);
    world.run_system_cached(prepare_ui_runtime).unwrap();
    assert!(!glyph_anchors(&mut world, OWN_COLOR).is_empty());
    set_tag_metadata(
        &mut world,
        1,
        protocol_ids::ActorDataKey::Name,
        protocol::ActorMetadataValue::String("".into()),
        feet,
    );
    world.run_system_cached(prepare_ui_runtime).unwrap();
    assert!(glyph_anchors(&mut world, OWN_COLOR).is_empty());
}

#[test]
fn nametag_publication_uses_the_shared_eye_ray_pick_in_third_person() {
    let perspective = PerspectiveMode::ThirdPersonBack;
    let mut world = fixture_world(perspective);
    prepare_actor_motion(&mut world);
    let feet = Vec3::new(0.0, 64.0, 0.0);
    prepare_local_frame(&mut world, perspective, feet, 1);
    publish_visibility(&mut world, perspective, feet, 1, 1);
    set_tag_metadata(
        &mut world,
        2,
        protocol_ids::ActorDataKey::AlwaysShowNameTag,
        protocol::ActorMetadataValue::Byte(0),
        feet,
    );
    world.insert_resource(crate::block_selection::CrosshairActor(Some(2)));
    world.run_system_cached(prepare_ui_runtime).unwrap();
    assert!(!glyph_anchors(&mut world, REMOTE_COLOR).is_empty());
    world
        .resource_mut::<crate::block_selection::CrosshairActor>()
        .0 = None;
    world.run_system_cached(prepare_ui_runtime).unwrap();
    assert!(glyph_anchors(&mut world, REMOTE_COLOR).is_empty());
}

#[test]
fn own_tag_setting_does_not_override_server_visibility() {
    let perspective = PerspectiveMode::ThirdPersonBack;
    let mut world = fixture_world(perspective);
    prepare_actor_motion(&mut world);
    let feet = Vec3::new(0.0, 64.0, 0.0);
    prepare_local_frame(&mut world, perspective, feet, 1);
    publish_visibility(&mut world, perspective, feet, 1, 1);
    world.run_system_cached(prepare_ui_runtime).unwrap();
    assert!(!glyph_anchors(&mut world, OWN_COLOR).is_empty());
    set_tag_metadata(
        &mut world,
        1,
        protocol_ids::ActorDataKey::AlwaysShowNameTag,
        protocol::ActorMetadataValue::Byte(0),
        feet,
    );
    world.run_system_cached(prepare_ui_runtime).unwrap();
    assert!(glyph_anchors(&mut world, OWN_COLOR).is_empty());
}
