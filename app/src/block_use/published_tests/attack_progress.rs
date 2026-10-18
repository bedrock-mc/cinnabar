//! Every interaction path keeps advancing around frame-time attack admission.

use super::*;
use client_presentation::aim_assist::{
    AimAssistFrame, AimAssistTarget, ServerAimAssist, TargetKind,
};
use gameplay::movement::MovementEffectSource;
use protocol::test_support::transactions::observe_transaction;

/// Routes a frame through the same semantic input owner as the producers.
pub(super) fn buttons(world: &mut World, buttons: Vec<u8>) {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: buttons,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
}

/// Supplies telemetry and idle controls, then registers the actual interaction schedule.
pub(super) fn interaction_app(mut world: World) -> App {
    world.init_resource::<bevy::ecs::schedule::Schedules>();
    #[cfg(feature = "acceptance")]
    {
        world.insert_resource(AcceptanceRun::new(None, None, false, false));
        world.init_resource::<crate::runtime::movement_evidence::MovementEvidenceEmitter>();
    }
    world.init_resource::<crate::melee::MeleeTarget>();
    world.init_resource::<client_interaction::survival_mining::SurvivalMiningRuntime>();
    world.init_resource::<client_state::GameplayScreenControls>();
    world.insert_resource(crate::runtime::visibility::AppMetrics(
        diagnostics::metrics::MetricsCollector::new(),
    ));
    let ray = world
        .resource::<InteractionOriginSnapshot>()
        .outbound_ray()
        .unwrap();
    let eye = ray.origin();
    let mut carrier = client_presentation::local_player::LocalPlayerFrameCarrier::default();
    carrier
        .publish(client_presentation::local_player::LocalPlayerFrameSample {
            session_generation: ray.session_generation(),
            actor_session_id: ray.actor_session_id(),
            fifo_sequence: ray.fifo_sequence(),
            physics_tick: ray.physics_tick(),
            perspective: ray.perspective(),
            world_collision_identity: ray.world_collision_identity().clone(),
            pose: Transform::from_translation(eye),
            eye,
            feet: eye - Vec3::Y * 1.62,
            rotation: Quat::IDENTITY,
        })
        .unwrap();
    world.insert_resource(carrier);
    let mut app = App::new();
    std::mem::swap(app.world_mut(), &mut world);
    crate::game_frame::interaction::configure(&mut app);
    app
}

/// Publishes completed ticks with the fixture's stable network position.
pub(super) fn ticks(world: &mut World, first: u64, end: u64) {
    world
        .resource_mut::<LocalMovementEffectTimeline>()
        .begin_frame();
    for tick in first..end {
        let mut sample = gameplay::test_support::survival_mining::completed(tick);
        sample.position = [4.5, 2.620_01, 8.5];
        world
            .resource_mut::<MovementTicker>()
            .enqueue_completed_physics(sample)
            .unwrap();
        world
            .resource_mut::<LocalMovementEffectTimeline>()
            .commit_successful_tick();
    }
}

#[test]
fn attack_progress_block_press_is_admitted_before_catchup_mining() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_snowball(&mut world);
    ticks(&mut world, 102, 104);
    world
        .resource_mut::<LocalMovementEffectTimeline>()
        .commit_successful_tick();
    buttons(&mut world, vec![1]);
    let mut app = interaction_app(world);
    app.world_mut().run_schedule(Update);
    assert!(
        !app.world().resource::<MeleeRuntime>().press_pending(),
        "the block press must be admitted in this frame"
    );
    let packets = captured.drain();
    let swing = protocol::swing_arm_packet(42, protocol::SwingSource::Mine).kind();
    assert_eq!(packets[0].kind(), swing);
    let inputs: Vec<_> = packets
        .iter()
        .filter_map(protocol::player_auth_input_trace_sample)
        .collect();
    assert_eq!(
        inputs.iter().map(|sample| sample.tick).collect::<Vec<_>>(),
        [101, 102, 103]
    );
    assert!(
        inputs
            .iter()
            .any(|sample| sample.flag_names.contains(&"PerformBlockActions"))
    );
}

#[test]
fn attack_progress_deferred_block_press_waits_to_break_and_never_attacks_twice() {
    for actor_behind in [false, true] {
        let (mut world, mut captured) = fixture();
        if actor_behind {
            actor_use_tests::spawn(&mut world, 5.5);
        }
        assert!(
            world
                .resource::<MovementTicker>()
                .unsent_sample_at(101)
                .is_some()
        );
        item_use_tests::hold_snowball(&mut world);
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        ticks(&mut world, 102, 103);
        buttons(&mut world, vec![1]);
        let mut app = interaction_app(world);
        app.world_mut().run_schedule(Update);
        assert!(app.world().resource::<MeleeRuntime>().press_pending());
        assert_eq!(
            app.world()
                .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                .len(),
            0,
            "mining must wait with the melee press behind older movement"
        );
        assert!(
            captured
                .drain()
                .iter()
                .filter_map(protocol::player_auth_input_trace_sample)
                .all(|input| !input.flag_names.contains(&"PerformBlockActions"))
        );

        app.world_mut()
            .resource_mut::<LocalMovementEffectTimeline>()
            .begin_frame();
        buttons(app.world_mut(), vec![]);
        app.world_mut().run_schedule(Update);
        assert!(!app.world().resource::<MeleeRuntime>().press_pending());
        assert_eq!(
            app.world()
                .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                .len(),
            1,
            "a released click must still break once after movement drains"
        );
        let packets = captured.drain();
        assert!(
            packets
                .iter()
                .filter_map(observe_transaction)
                .all(|tx| tx.actor_use().is_none())
        );
        let swing = protocol::swing_arm_packet(42, protocol::SwingSource::Mine).kind();
        assert_eq!(
            packets
                .iter()
                .filter(|packet| packet.kind() == swing)
                .count(),
            2
        );

        ticks(app.world_mut(), 103, 104);
        buttons(app.world_mut(), vec![]);
        app.world_mut().run_schedule(Update);
        let packets = captured.drain();
        assert!(
            packets
                .iter()
                .filter_map(observe_transaction)
                .all(|tx| tx.actor_use().is_none())
        );
        let inputs: Vec<_> = packets
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
            .collect();
        assert!(inputs.iter().any(|input| input.tick == 103));
        assert!(
            inputs
                .iter()
                .all(|input| !input.flag_names.contains(&"MissedSwing"))
        );
        assert_eq!(
            app.world()
                .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                .len(),
            1
        );
    }
}

#[test]
fn attack_progress_cancelled_block_press_discards_its_retained_mining_click() {
    for expired in [true, false] {
        let (mut world, mut captured) = fixture();
        item_use_tests::hold_snowball(&mut world);
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        ticks(&mut world, 102, 103);
        buttons(&mut world, vec![1]);
        let mut app = interaction_app(world);
        for waiting in [true, false] {
            if !waiting {
                buttons(app.world_mut(), vec![]);
                if expired {
                    app.world_mut()
                        .resource_mut::<Time<Real>>()
                        .advance_by(Duration::from_millis(
                            gameplay::interaction_authority::MAX_PENDING_INTERACTION_MILLIS + 1,
                        ));
                } else {
                    app.world_mut().resource_mut::<MeleeRuntime>().cancel();
                }
            }
            app.world_mut()
                .run_system_cached(crate::melee::prepare_melee)
                .unwrap();
            app.world_mut()
                .run_system_cached(crate::melee::produce_attacks)
                .unwrap();
            assert_eq!(
                app.world().resource::<MeleeRuntime>().press_pending(),
                waiting
            );
            assert_eq!(
                app.world()
                    .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                    .len(),
                0,
                "a cancelled click must never predict a break"
            );
        }
        app.world_mut().run_schedule(Update);
        ticks(app.world_mut(), 103, 104);
        buttons(app.world_mut(), vec![]);
        app.world_mut().run_schedule(Update);
        let packets = captured.drain();
        let inputs: Vec<_> = packets
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
            .collect();
        assert!(inputs.iter().any(|input| input.tick == 103));
        assert!(
            inputs
                .iter()
                .all(|input| !input.flag_names.contains(&"PerformBlockActions"))
        );
        assert_eq!(
            app.world()
                .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                .len(),
            0
        );
    }
}

#[test]
fn attack_progress_earlier_predicted_break_reaches_its_tick_while_a_new_press_waits() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_snowball(&mut world);
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let block = stream
            .collision_store()
            .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
            .unwrap()
            .runtime_id(0, 4, 2, 6)
            .unwrap();
        stream.predict_block([4, 2, 5], 0, block);
    }
    buttons(&mut world, vec![]);
    produce_air_use(&mut world);
    captured.drain();
    let mut app = interaction_app(world);
    buttons(app.world_mut(), vec![1]);
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        1
    );
    captured.drain();
    buttons(app.world_mut(), vec![]);
    app.world_mut().run_schedule(Update);
    ticks(app.world_mut(), 102, 104);
    app.world_mut()
        .resource_mut::<LocalMovementEffectTimeline>()
        .begin_frame();
    app.world_mut()
        .resource_mut::<LocalMovementEffectTimeline>()
        .commit_successful_tick();
    buttons(app.world_mut(), vec![1]);
    app.world_mut().run_schedule(Update);
    assert!(app.world().resource::<MeleeRuntime>().press_pending());
    let packets = captured.drain();
    let carrier = packets
        .iter()
        .filter_map(protocol::player_auth_input_trace_sample)
        .find(|sample| sample.tick == 102)
        .unwrap();
    assert!(
        carrier.flag_names.contains(&"PerformBlockActions"),
        "an earlier predicted break must travel even while the next press waits"
    );
}

#[test]
fn attack_progress_active_crossbow_completes_while_melee_waits() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_air_item(&mut world, "minecraft:crossbow", 1, [].into());
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    produce_air_use(&mut world);
    let (started, duration) = world
        .resource::<client_interaction::item_use::ItemUseRuntime>()
        .active_timing()
        .unwrap();
    captured.drain();
    ticks(&mut world, started, started + u64::from(duration));
    buttons(&mut world, vec![1, 2]);
    world
        .resource_mut::<MeleeRuntime>()
        .observe_input(true, true);
    produce_air_use(&mut world);
    assert!(world.resource::<MeleeRuntime>().press_pending());
    let runtime = world.resource::<client_interaction::item_use::ItemUseRuntime>();
    assert!(
        !runtime.is_using(),
        "accepted charge completion must advance while melee waits"
    );
    assert_eq!(runtime.movement_modifier(), None);
    assert!(
        captured
            .drain()
            .iter()
            .filter_map(observe_transaction)
            .all(|tx| tx.item_use().is_none())
    );
}

#[test]
fn attack_progress_release_keeps_the_next_movement_input_advancing() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_air_item(&mut world, "minecraft:crossbow", 1, [].into());
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    let mut app = interaction_app(world);
    app.world_mut().run_schedule(Update);
    assert!(
        app.world()
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .is_using()
    );
    captured.drain();
    buttons(app.world_mut(), vec![1]);
    ticks(app.world_mut(), 102, 103);
    app.world_mut().run_schedule(Update);
    assert!(
        captured
            .drain()
            .iter()
            .filter_map(observe_transaction)
            .any(|tx| tx.release().is_some())
    );
    ticks(app.world_mut(), 103, 104);
    buttons(app.world_mut(), vec![1]);
    app.world_mut().run_schedule(Update);
    assert!(
        captured
            .drain()
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
            .any(|sample| sample.tick == 103)
    );
}

#[test]
fn attack_progress_middle_click_still_sends_its_pick_request() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_snowball(&mut world);
    buttons(&mut world, vec![]);
    world.init_resource::<client_state::GameplayScreenControls>();
    world
        .resource_mut::<client_state::GameplayScreenControls>()
        .pick_block_pressed = true;
    let mut app = interaction_app(world);
    app.world_mut().run_schedule(Update);
    let pick = protocol::block_pick_request_packet([4, 2, 6], false).kind();
    assert!(captured.drain().iter().any(|packet| packet.kind() == pick));
}

#[test]
fn attack_progress_instant_break_publishes_both_swings_and_the_next_input() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_snowball(&mut world);
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    buttons(&mut world, vec![]);
    produce_air_use(&mut world);
    captured.drain();
    buttons(&mut world, vec![1]);
    let mut app = interaction_app(world);
    app.world_mut().run_schedule(Update);
    let packets = captured.drain();
    let swing = protocol::swing_arm_packet(42, protocol::SwingSource::Mine).kind();
    assert_eq!(
        packets
            .iter()
            .filter(|packet| packet.kind() == swing)
            .count(),
        2
    );
    assert_eq!(
        app.world()
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        1
    );
    ticks(app.world_mut(), 102, 103);
    buttons(app.world_mut(), vec![1]);
    app.world_mut().run_schedule(Update);
    assert!(
        captured
            .drain()
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
            .any(|sample| sample.tick == 102)
    );
}

#[test]
fn attack_progress_assisted_target_point_and_facing_share_the_published_frame() {
    assisted_attack_with_published_aim(false);
}

#[test]
fn attack_progress_piercing_direction_shares_the_published_facing() {
    assisted_attack_with_published_aim(true);
}

/// Checks actor and item-directed attacks against the same changing assisted frame.
fn assisted_attack_with_published_aim(piercing: bool) {
    let (mut world, mut captured) = attack_hit_position_tests::unpublished_attack_fixture(7.5);
    if piercing {
        item_use_tests::hold_air_item(&mut world, "fixture:piercing", 1, [].into());
        world
            .resource_mut::<crate::runtime::world::ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .set_item_attack_timings(std::sync::Arc::new(
                [(
                    Box::<str>::from("fixture:piercing"),
                    protocol::ItemAttackTiming {
                        piercing_weapon: true,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            ));
        assert!(
            crate::melee::selected_attack_timing(
                world.resource::<client_state::PlayerRuntime>(),
                world.resource::<crate::runtime::world::ClientWorld>()
            )
            .unwrap()
            .piercing_weapon
        );
    }
    attack_frame_tests::actor_boxes(&mut world, 100, &[([5.0, 2.0, 7.0], [6.0, 3.0, 8.0])]);
    let context = client_presentation::camera::ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 90.0,
        actors: &|_| None,
    };
    let mut camera = client_presentation::camera::ServerCameraView::default();
    camera.apply(
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
    camera.apply(
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
    world.insert_resource(camera);
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &protocol::CameraEvent::AimAssist(protocol::CameraAimAssistSettings {
            preset_id: "".into(),
            view_angle: [60.0; 2],
            distance: 12.0,
            target_mode: protocol::CameraAimAssistTargetMode::Angle,
            action: protocol::CameraAimAssistAction::Set,
            show_debug_render: false,
        }),
    );
    let mut frame = AimAssistFrame::default();
    {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_ref().unwrap();
        let registries = world.resource::<PhysicsCollisionRegistries>();
        let blocks = sim::PaletteWorld::new(
            stream.collision_store(),
            registries.registry(stream.network_id_mode()),
            0,
            Default::default(),
        );
        frame.evaluate(
            &state,
            stream.authority(),
            &blocks,
            1,
            Vec3::new(4.5, 2.5, 8.5),
            Vec3::NEG_Z,
            None,
            None,
            |_| None,
            |_| &[],
        );
    }
    let (minimum, maximum) = attack_hit_position_tests::actor_pick_bounds(&world);
    let point = Vec3::new(4.6, 2.4, 7.5);
    frame.target = Some(AimAssistTarget {
        kind: TargetKind::Actor(attack_hit_position_tests::actor_pick_lifetime(&world)),
        minimum,
        maximum,
        point,
        direction: Vec3::NEG_Z,
        score: 0.0,
    });
    let expected = client_interaction::camera::aim_assist::action_rotation(
        &frame,
        world.resource::<client_presentation::camera::ServerCameraView>(),
    )
    .unwrap();
    world
        .resource_mut::<InteractionOriginSnapshot>()
        .assist_direction(frame.interaction_direction().unwrap());
    let direction = frame.interaction_direction().unwrap();
    world.insert_resource(frame);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    let lifetime = world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_lifetime(100)
        .unwrap();
    world.resource_mut::<AimAssistFrame>().target = Some(AimAssistTarget {
        kind: TargetKind::Actor(lifetime),
        minimum: Vec3::new(5.0, 2.0, 7.0),
        maximum: Vec3::new(6.0, 3.0, 8.0),
        point: Vec3::new(5.5, 2.5, 7.5),
        direction: Vec3::NEG_Z,
        score: 0.0,
    });
    let current = client_interaction::camera::aim_assist::action_rotation(
        world.resource::<AimAssistFrame>(),
        world.resource::<client_presentation::camera::ServerCameraView>(),
    )
    .unwrap();
    assert!(expected.dot(current).abs() < 0.99);
    let current_direction = world
        .resource::<AimAssistFrame>()
        .interaction_direction()
        .unwrap();
    world
        .resource_mut::<InteractionOriginSnapshot>()
        .assist_direction(current_direction);
    ticks(&mut world, 102, 103);
    attack_hit_position_tests::attack(&mut world);
    let packets = captured.drain();
    let tx = packets.iter().find_map(observe_transaction).unwrap();
    if piercing {
        let item = tx.item_use().unwrap();
        assert_eq!(
            item.action,
            protocol::test_support::transactions::ItemUseAction::UseAsAttack
        );
        let emitted_direction =
            Vec3::from_array(item.click_position) - Vec3::from_array(item.from_position);
        assert!(
            (emitted_direction - direction).length() < 0.00001,
            "piercing packets must use the published direction"
        );
    } else {
        assert_eq!(
            (
                tx.actor_use().unwrap().runtime_id,
                tx.actor_use().unwrap().hit_position
            ),
            (99, point.to_array())
        );
    }
    assert!(
        world
            .resource::<LocalViewPose>()
            .rotation()
            .dot(expected)
            .abs()
            > 0.99999,
        "the admitted attack must face its published target"
    );
    produce_air_use(&mut world);
    let inputs = captured.drain();
    let input = inputs
        .iter()
        .find_map(protocol::player_auth_input_trace_sample)
        .unwrap();
    let (yaw, pitch, _) = expected.to_euler(EulerRot::YXZ);
    assert!((input.yaw - gameplay::movement::wire_yaw(180.0 - yaw.to_degrees())).abs() < 0.001);
    assert!((input.pitch + pitch.to_degrees()).abs() < 0.001);
}

#[test]
fn attack_progress_held_sweep_preserves_the_post_break_delay() {
    for actor_in_front in [false, true] {
        let (mut world, mut captured) = fixture();
        item_use_tests::hold_snowball(&mut world);
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        let block = world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .collision_store()
            .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
            .unwrap()
            .runtime_id(0, 4, 2, 6)
            .unwrap();
        buttons(&mut world, vec![]);
        produce_air_use(&mut world);
        captured.drain();
        let mut app = interaction_app(world);
        buttons(app.world_mut(), vec![1]);
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world()
                .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                .len(),
            1
        );
        ticks(app.world_mut(), 102, 103);
        buttons(app.world_mut(), vec![1]);
        app.world_mut().run_schedule(Update);
        if actor_in_front {
            actor_use_tests::spawn(app.world_mut(), 7.5);
            // Give published picking its preceding frame before the next mining tick.
            app.world_mut()
                .resource_mut::<LocalMovementEffectTimeline>()
                .begin_frame();
            buttons(app.world_mut(), vec![1]);
            app.world_mut().run_schedule(Update);
        }
        ticks(app.world_mut(), 103, 104);
        buttons(app.world_mut(), vec![1]);
        app.world_mut().run_schedule(Update);
        assert!(
            app.world().resource::<MeleeRuntime>().actor_in_front(),
            "both air misses and actor picks suppress the mining target"
        );
        if actor_in_front {
            actor_use_tests::spawn(app.world_mut(), -30.0);
        }
        {
            let mut world = app
                .world_mut()
                .resource_mut::<crate::runtime::world::ClientWorld>();
            world
                .stream
                .as_mut()
                .unwrap()
                .predict_block([4, 2, 6], 0, block);
        }
        // Publish the changed pick without advancing mining time or releasing Attack.
        app.world_mut()
            .resource_mut::<LocalMovementEffectTimeline>()
            .begin_frame();
        buttons(app.world_mut(), vec![1]);
        app.world_mut().run_schedule(Update);
        let last_delayed = 103 + u64::from(gameplay::survival_mining::DESTROY_DELAY_TICKS);
        for tick in 104..last_delayed {
            ticks(app.world_mut(), tick, tick + 1);
            buttons(app.world_mut(), vec![1]);
            assert!(
                app.world()
                    .resource::<client_state::SemanticInputSnapshot>()
                    .phase(semantic_input::Action::Attack)
                    .held
            );
            app.world_mut().run_schedule(Update);
            assert_eq!(
                app.world()
                    .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                    .len(),
                1,
                "held input lost its delay after aiming at actor={actor_in_front}, tick={tick}"
            );
        }
        ticks(app.world_mut(), last_delayed, last_delayed + 1);
        buttons(app.world_mut(), vec![1]);
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world()
                .resource::<Messages<client_audio::audio::LocalBlockCue>>()
                .len(),
            2,
            "held mining must resume when its original delay finishes"
        );
    }
}

#[test]
fn attack_progress_pending_press_does_not_hold_an_existing_destroy_after_release() {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_snowball(&mut world);
    world.init_resource::<crate::melee::MeleeTarget>();
    // A held input can maintain mining without another press proposal.
    buttons(&mut world, vec![1]);
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    assert_eq!(
        world
            .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
            .destroying_target()
            .map(|(position, _)| position),
        Some([4, 2, 6])
    );
    produce_air_use(&mut world);
    captured.drain();
    world
        .resource_mut::<MeleeRuntime>()
        .observe_input(true, false);
    buttons(&mut world, vec![]);
    ticks(&mut world, 102, 103);
    // No proposal is available for the queued press, while current mining facts remain valid.
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    assert!(world.resource::<MeleeRuntime>().press_pending());
    assert_eq!(
        world
            .resource::<client_interaction::survival_mining::SurvivalMiningRuntime>()
            .destroying_target(),
        None,
        "the queued press must not turn released input into held mining"
    );
    produce_air_use(&mut world);
    let packets = captured.drain();
    let actions: Vec<_> = packets
        .iter()
        .flat_map(protocol::test_support::packets::input_block_actions)
        .collect();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].0,
        protocol::BlockActionKind::AbortDestroy.wire_value()
    );
    assert_eq!(actions[0].1, [4, 2, 6]);
}

#[test]
fn middle_click_requests_the_nearer_mob_and_respects_block_occlusion() {
    for (actor_z, expected) in [
        (7.5, protocol::actor_pick_request_packet(99, false)),
        (5.5, protocol::block_pick_request_packet([4, 2, 6], false)),
    ] {
        let (mut world, mut captured) = fixture();
        actor_use_tests::spawn(&mut world, actor_z);
        world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        buttons(&mut world, vec![]);
        world.init_resource::<client_state::GameplayScreenControls>();
        world
            .resource_mut::<client_state::GameplayScreenControls>()
            .pick_block_pressed = true;
        world
            .run_system_cached(crate::pick_block::produce_pick_block)
            .unwrap();
        let actor_pick = protocol::actor_pick_request_packet(99, false).kind();
        let block_pick = protocol::block_pick_request_packet([4, 2, 6], false).kind();
        let picks = captured
            .drain()
            .iter()
            .filter(|packet| packet.kind() == actor_pick || packet.kind() == block_pick)
            .map(|packet| protocol::encode(packet).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            picks,
            [protocol::encode(&expected).unwrap()],
            "actor z={actor_z}"
        );
    }
}

#[test]
fn browser_canvas_pick_binding_publishes_one_request_per_press() {
    use crate::browser_input::{BrowserOptions, CanvasCapture, InputSource, sample_input};
    use launcher::menu::settings_options::BindingId;
    use semantic_input::{ButtonEdges, DeviceFrame, KeyboardMouseFrame, PhysicalControl};

    for (actor_z, expected) in [
        (7.5, protocol::actor_pick_request_packet(99, false)),
        (5.5, protocol::block_pick_request_packet([4, 2, 6], false)),
    ] {
        for control in [
            PhysicalControl::MouseButton(3),
            PhysicalControl::KeyboardUsage(0x19),
        ] {
            let (mut world, mut captured) = fixture();
            actor_use_tests::spawn(&mut world, actor_z);
            world
                .resource_mut::<client_state::PlayerRuntime>()
                .facts
                .publish_player_game_mode(PlayerGameMode::Creative);
            buttons(&mut world, vec![]);
            world.init_resource::<client_state::GameplayScreenControls>();
            world.init_resource::<crate::semantic_controls::PendingDeviceFrame>();
            let mut options = BrowserOptions::default();
            assert!(
                std::sync::Arc::make_mut(&mut options.0)
                    .remap(BindingId::PickItem.row(false).unwrap(), control)
            );
            world.insert_resource(options);
            world.init_resource::<CanvasCapture>();
            for (pressed, held, focus_lost, ui_focused, count) in [
                (true, true, false, false, 1),
                (false, true, false, false, 0),
                (false, false, false, false, 0),
                (true, false, false, false, 1),
                (true, true, true, false, 0),
                (true, true, false, true, 0),
            ] {
                let mut keyboard = KeyboardMouseFrame::default();
                match control {
                    PhysicalControl::MouseButton(button) => {
                        keyboard.mouse_buttons = held.then_some(button).into_iter().collect();
                        keyboard.mouse_edges = ButtonEdges {
                            pressed: pressed.then_some(button).into_iter().collect(),
                            released: (!held).then_some(button).into_iter().collect(),
                        };
                    }
                    PhysicalControl::KeyboardUsage(key) => {
                        keyboard.keys = held.then_some(key).into_iter().collect();
                        keyboard.key_edges.pressed = pressed.then_some(key).into_iter().collect();
                    }
                    _ => unreachable!(),
                }
                world.insert_non_send(InputSource(Box::new(move || DeviceFrame {
                    keyboard_mouse: Some(keyboard.clone()),
                    window_focus_lost: focus_lost,
                    ..Default::default()
                })));
                world
                    .resource_mut::<client_state::ScreenFocus>()
                    .absorbs_input = ui_focused;
                world.run_system_cached(sample_input).unwrap();
                world
                    .run_system_cached(crate::pick_block::produce_pick_block)
                    .unwrap();
                let picks: Vec<_> = captured
                    .drain()
                    .into_iter()
                    .filter(|packet| packet.kind() == expected.kind())
                    .map(|packet| protocol::encode(&packet).unwrap())
                    .collect();
                assert_eq!(
                    picks,
                    vec![protocol::encode(&expected).unwrap(); count],
                    "control={control:?}, pressed={pressed}, held={held}, focus_lost={focus_lost}, ui={ui_focused}"
                );
            }
        }
    }
}
