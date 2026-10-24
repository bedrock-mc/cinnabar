//! Respawn controls retire the producer's charge before movement or item admission resumes.

use super::*;
use client_interaction::item_use::ItemUseRuntime;

/// Routes the physical Use button through the ordinary semantic input owner.
pub(super) fn use_button(world: &mut World, held: bool) {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: if held { vec![2] } else { Vec::new() },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
}

/// Runs the two item-use phases around the production movement FIFO drain.
pub(super) fn item_phases(world: &mut World) {
    world
        .run_system_cached(crate::item_use::prepare_item_release)
        .unwrap();
    flush_and_acknowledge(world);
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
}

/// Publishes one wet fixed tick, then resolves frame-time item actions in wire order.
fn wet_tick(world: &mut World, held: bool) -> gameplay::movement::PhysicsMovementSample {
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    use_button(world, held);
    let sample = tick(world);
    item_phases(world);
    sample
}

/// Builds a retained selected trident and an admitted charge of the requested length.
pub(super) fn charging_world(charge_ticks: u64) -> (World, client_session::CapturedPackets) {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_air_item(&mut world, "minecraft:trident", 1, riptide_data());
    let position = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 100, false);
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &Water,
    );
    world.insert_resource(physics);
    world.init_resource::<Messages<client_audio::audio_ingress::SequencedAudioEvent>>();
    world.init_resource::<client_presentation::environment::WorldClock>();
    world.init_resource::<client_presentation::environment::WeatherState>();
    world.init_resource::<client_interaction::movement::LocalMovementSpeedAuthority>();
    world.init_resource::<client_interaction::movement::LocomotionState>();
    world.init_resource::<client_presentation::camera::look::LookSmoother>();
    world.init_resource::<crate::runtime::world::WorldStreamFramePoll>();
    world.init_resource::<render_scene::ChunkUploadBudget>();
    world.init_resource::<client_presentation::camera::CameraSettingsAuthority>();
    world.init_resource::<client_presentation::local_player::LocalPlayerFrameCarrier>();
    world.init_resource::<client_presentation::server_camera::ServerCameraInstructions>();
    world.init_resource::<crate::runtime::movement_evidence::MovementEvidenceEmitter>();
    world.insert_resource(acceptance::model_witness::ModelWitnessFileSource::new(None));
    world.insert_resource(client_presentation::camera::AutoFly::new(false));
    #[cfg(feature = "acceptance")]
    world.insert_resource(AcceptanceRun::new(None, None, false, false));
    #[cfg(not(feature = "acceptance"))]
    world.init_resource::<AcceptanceRun>();
    item_phases(&mut world);
    assert!(world.resource::<ItemUseRuntime>().is_using());
    for _ in 0..charge_ticks {
        wet_tick(&mut world, true);
    }
    captured.drain();
    (world, captured)
}

/// Commits a contiguous respawn event through the real world reconciliation owner.
fn respawn_phase(world: &mut World, sequence: u64, state: u8) {
    let position = [4.5, 2.620_01, 8.5];
    world
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            sequence,
            protocol::WorldEvent::Respawn(protocol::RespawnEvent {
                position,
                state,
                runtime_entity_id: 0,
            }),
        )
        .unwrap();
    world
        .run_system_cached(crate::runtime::world::reconcile_world_stream_before_physics)
        .unwrap();
    world
        .run_system_cached(crate::runtime::world::advance_dimension_transfer)
        .unwrap();
}

/// Holds and releases after respawn, then verifies only a fresh charge can launch.
fn charge_across_respawn(search: bool, fresh_ticks: u64) {
    let (mut world, mut captured) = charging_world(gameplay::item_use::RIPTIDE_CHARGE_TICKS);
    let selection = client_interaction::block_use::verified_use_selection(
        world.resource::<client_state::PlayerRuntime>(),
        world.resource::<UiRuntimeResource>(),
    )
    .unwrap();
    if search {
        respawn_phase(&mut world, 3, 0);
        for _ in 0..2 {
            use_button(&mut world, true);
            world
                .resource_mut::<Time<Real>>()
                .advance_by(simulation_clock::TICK_DURATION);
            world
                .run_system_cached(crate::movement::advance_local_physics)
                .unwrap();
            item_phases(&mut world);
            assert!(!world.resource::<MovementTicker>().has_unsent_inputs());
        }
    }
    respawn_phase(&mut world, if search { 4 } else { 3 }, 1);
    assert_eq!(
        client_interaction::block_use::verified_use_selection(
            world.resource::<client_state::PlayerRuntime>(),
            world.resource::<UiRuntimeResource>(),
        )
        .unwrap(),
        selection,
        "respawn retains the selected stack in this fixture"
    );
    world.resource_mut::<LocalPhysicsController>().advance(
        Duration::ZERO,
        sim::MovementInput::default(),
        &Water,
    );
    for _ in 0..2 {
        wet_tick(&mut world, true);
    }
    use_button(&mut world, false);
    item_phases(&mut world);
    wet_tick(&mut world, false);
    let after_release = wet_tick(&mut world, false);
    assert!(
        !after_release.processed.spin_started,
        "the pre-respawn charge generated a new launch (search={search})"
    );
    assert!(!world.resource::<ItemUseRuntime>().is_using());
    assert_eq!(world.resource::<ItemUseRuntime>().movement_modifier(), None);
    assert!(!world.resource::<MovementTicker>().has_held_release());
    assert!(
        captured
            .drain()
            .iter()
            .filter_map(protocol::player_auth_input_trace_sample)
            .all(|sample| !sample.flag_names.contains(&"StartSpinAttack"))
    );
    use_button(&mut world, true);
    item_phases(&mut world);
    assert!(
        world.resource::<ItemUseRuntime>().is_using(),
        "a new press remains usable"
    );
    for _ in 0..fresh_ticks {
        wet_tick(&mut world, true);
    }
    use_button(&mut world, false);
    item_phases(&mut world);
    wet_tick(&mut world, false);
    let launch = wet_tick(&mut world, false);
    assert_eq!(
        launch.processed.spin_started,
        fresh_ticks >= gameplay::item_use::RIPTIDE_CHARGE_TICKS
    );
}

#[test]
fn riptide_unreleased_charge_does_not_cross_direct_ready_respawn() {
    for fresh_ticks in [
        gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1,
        gameplay::item_use::RIPTIDE_CHARGE_TICKS,
    ] {
        charge_across_respawn(false, fresh_ticks);
    }
}

#[test]
fn riptide_unreleased_charge_does_not_cross_search_and_ready_respawn() {
    for fresh_ticks in [
        gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1,
        gameplay::item_use::RIPTIDE_CHARGE_TICKS,
    ] {
        charge_across_respawn(true, fresh_ticks);
    }
}

#[test]
fn riptide_new_presses_cannot_start_a_charge_during_respawn_search() {
    let (mut world, mut captured) = charging_world(gameplay::item_use::RIPTIDE_CHARGE_TICKS);
    respawn_phase(&mut world, 3, 0);
    use_button(&mut world, false);
    item_phases(&mut world);
    use_button(&mut world, true);
    item_phases(&mut world);
    assert!(!world.resource::<ItemUseRuntime>().is_using());
    assert_eq!(world.resource::<ItemUseRuntime>().movement_modifier(), None);
    assert!(
        captured.drain().is_empty(),
        "search withholds item actions too"
    );
}

#[test]
fn riptide_respawn_retires_the_modifier_before_physics_and_blocks_full_completion() {
    for state in [0, 1] {
        let (mut world, mut captured) = charging_world(gameplay::item_use::RIPTIDE_CHARGE_TICKS);
        if state == 1 {
            let network = world.resource::<NetworkHandle<PackApplication>>();
            while network
                .send_inventory_packets(vec![protocol::respawn_ready_packet(42)])
                .is_ok()
            {}
        }
        respawn_phase(&mut world, 3, state);
        assert!(
            world
                .resource::<crate::runtime::world::ClientWorld>()
                .respawn
                .input_held()
        );
        assert_eq!(world.resource::<ItemUseRuntime>().active_timing(), None);
        assert_eq!(world.resource::<ItemUseRuntime>().movement_modifier(), None);
        captured.drain();
        use_button(&mut world, false);
        item_phases(&mut world);
        use_button(&mut world, true);
        item_phases(&mut world);
        assert!(!world.resource::<ItemUseRuntime>().is_using());
        assert!(captured.drain().is_empty());
    }
}

/// Resolves an old-life use or release whose facing input has not been simulated.
fn reserved_input_after_respawn(release: bool) {
    let (mut world, mut captured) = charging_world(0);
    if release {
        world
            .resource_mut::<LocalViewPose>()
            .set_rotation(Quat::from_rotation_y(90_f32.to_radians()));
        use_button(&mut world, false);
        item_phases(&mut world);
        assert!(world.resource::<MovementTicker>().has_held_release());
    }
    respawn_phase(&mut world, 3, 1);
    world.resource_mut::<LocalPhysicsController>().advance(
        Duration::ZERO,
        sim::MovementInput::default(),
        &Water,
    );
    captured.drain();
    let sample = wet_tick(&mut world, true);
    let packets = captured.drain();
    let inputs: Vec<_> = packets
        .iter()
        .filter_map(protocol::player_auth_input_trace_sample)
        .collect();
    assert_eq!(inputs.len(), 1);
    if release {
        assert_eq!(
            (inputs[0].pitch, inputs[0].yaw),
            (sample.pitch, sample.yaw),
            "a discarded release cannot rotate the new life's input"
        );
    }
    assert!(
        !inputs[0].flag_names.contains(&"StartUsingItem"),
        "a reserved old-life use flag crossed ready-to-spawn"
    );
}

#[test]
fn riptide_respawn_discards_reserved_use_input() {
    reserved_input_after_respawn(false);
}

#[test]
fn riptide_respawn_discards_reserved_release_facing() {
    reserved_input_after_respawn(true);
}
