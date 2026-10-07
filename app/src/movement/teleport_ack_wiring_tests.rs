//! HandledTeleport coverage through production world-stream reconciliation.
//!
//! MovePlayer teleports acknowledge without opt-in. Correction-snap and
//! respawn acknowledgements remain gated until their reference is established.

use bevy::prelude::{App, IntoScheduleConfigs, Update};
use protocol::{
    ChangeDimensionEvent, MovePlayerEvent, MovementCorrectionSubject, Packet, PlayerInputMode,
    PlayerMovementCorrectionEvent, RespawnEvent, WorldBootstrap, WorldEvent,
    player_auth_input_trace_sample,
};

use super::integration_tests::{evidence_context, synthetic_preg};
use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCollisionRegistries,
    ProcessedMovementState, ServerTeleportKind, flush_player_auth_inputs,
};
use crate::acceptance::{AcceptanceRun, model_witness::ModelWitnessFileSource};
use crate::camera::CameraSettingsAuthority;
use crate::environment::{WeatherState, WorldClock};
use crate::local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier, LocalViewPose};
use crate::runtime::phase3_evidence::Phase3EvidenceEmitter;
use crate::runtime::world::{
    ClientWorld, WorldStreamFramePoll, advance_dimension_transfer,
    reconcile_world_stream_before_physics,
};
use assets::read_registry_for_protocol;
use chunk_pipeline::WorldStream;
use client_presentation::server_camera::ServerCameraInstructions;
use client_ui::ui_runtime::UiRuntime;
use gameplay::movement::TELEPORT_ACK_ADMITTED_TICK_BUDGET;
use render::ChunkUploadBudget;
use sim::{CollisionIdSpace, CollisionRegistryIdentity, WorldCollisionIdentity};

#[path = "teleport_ack_wiring_tests/correction_presentation.rs"]
mod correction_presentation;
#[path = "teleport_ack_wiring_tests/respawn.rs"]
mod respawn;

fn fixture_world_identity() -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            protocol: 1001,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [1; 32],
        },
        [],
    )
    .unwrap()
}

fn completed_sample(tick: u64, position: [f32; 3]) -> super::PhysicsMovementSample {
    super::PhysicsMovementSample {
        tick,
        position,
        movement: [0.125, -0.078_4, -0.25],
        velocity: [0.125, -0.078_4, -0.25],
        move_vector: [0.0, 1.0],
        raw_move_vector: [0.0, 1.0],
        analogue_move_vector: [0.0, 1.0],
        pitch: 10.0,
        yaw: 20.0,
        head_yaw: 20.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        sneak_button: false,
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: false,
        grounded_after_tick: false,
        horizontal_collision: false,
        vertical_collision: false,
        jump_repeated: false,
        processed: ProcessedMovementState::default(),
        world_identity: fixture_world_identity(),
    }
}

/// An authorized session fixture with the settle window lifted before the
/// reconciliation under test re-engages it through its snap anchor.
fn authorized_ticker(enabled: bool) -> MovementTicker {
    let mut ticker = MovementTicker::default();
    ticker.testing_set_teleport_ack(enabled);
    ticker.reset(7, 100, [0.0, 70.0, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
}

fn fixture_registries() -> PhysicsCollisionRegistries {
    let breg = include_bytes!("../../../crates/assets/data/block-registry-v2193.bin");
    let records = read_registry_for_protocol(breg, 2193).expect("checked-in protocol-2193 BREG");
    let preg = synthetic_preg(breg, &records);
    PhysicsCollisionRegistries::from_assets(
        breg,
        &records,
        &preg,
        crate::asset_startup::active_content_registry_protocol(),
    )
    .expect("BREG-bound PREG facts are valid")
}

fn fixture_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 70.0, 0.0],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

/// Minimal production-shaped app running exactly the real world-stream
/// reconciliation system over a real connected client world.
fn wiring_app(ticker: MovementTicker, physics: LocalPhysicsController) -> App {
    let mut app = App::new();
    app.add_message::<client_presentation::audio_ingress::SequencedAudioEvent>()
        .insert_resource(ClientWorld {
            stream: Some(fixture_stream()),
            ..ClientWorld::default()
        })
        .init_resource::<WorldClock>()
        .init_resource::<WeatherState>()
        .insert_resource(ticker)
        .insert_resource(physics)
        .init_resource::<crate::movement::LocalMovementEffectTimeline>()
        .init_resource::<crate::movement::LocalMovementSpeedAuthority>()
        .insert_resource(fixture_registries())
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .init_resource::<bevy::prelude::Time<bevy::time::Real>>()
        .insert_resource(AcceptanceRun::new(Some(900), None, false, false))
        .init_resource::<ChunkUploadBudget>()
        .insert_resource(ModelWitnessFileSource::new(None))
        .init_resource::<CameraSettingsAuthority>()
        .init_resource::<LocalViewPose>()
        .init_resource::<LocalPlayerFrameCarrier>()
        .init_resource::<InteractionOriginSnapshot>()
        .init_resource::<Phase3EvidenceEmitter>()
        .init_resource::<WorldStreamFramePoll>()
        .init_resource::<ServerCameraInstructions>()
        .add_systems(Update, reconcile_world_stream_before_physics);
    app
}

fn submit(app: &mut App, sequence: u64, event: WorldEvent) {
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .expect("fixture stream stays connected")
        .submit(sequence, event)
        .expect("fixture event commits");
}

fn flush_capturing(ticker: &mut MovementTicker) -> Vec<Packet> {
    let mut packets = Vec::new();
    flush_player_auth_inputs(ticker, 8, Some(evidence_context()), |_identity, packet| {
        packets.push(packet);
        Ok::<_, &'static str>(())
    })
    .unwrap();
    packets
}

fn carries_handled_teleport(packet: &Packet) -> bool {
    player_auth_input_trace_sample(packet)
        .expect("PlayerAuthInput projects for the trace table")
        .flag_names
        .contains(&"HandledTeleport")
}

/// Transmission is unconditional, so the armed assertion rides the first
/// transmitted packet after reconciliation and later packets stay unflagged.
/// Enqueues two contiguous admissions from the post-reconciliation tick and
/// returns both transmitted packets.
fn transmit_two_after_reconciliation(ticker: &mut MovementTicker) -> Vec<Packet> {
    let tick = ticker.next_tick();
    ticker
        .enqueue_completed_physics(completed_sample(tick, [1.0, 70.0, 0.0]))
        .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(tick + 1, [1.0, 70.0, 0.0]))
        .unwrap();
    flush_capturing(ticker)
}

#[test]
fn committed_respawn_through_production_reconciliation_projects_the_opt_in_flag() {
    let mut app = wiring_app(authorized_ticker(true), LocalPhysicsController::default());
    submit(
        &mut app,
        1,
        WorldEvent::Respawn(RespawnEvent {
            position: [8.5, 71.620_01, -4.25],
            state: 1,
            runtime_entity_id: 1,
        }),
    );
    app.update();

    assert_eq!(
        app.world()
            .resource::<MovementTicker>()
            .pending_teleport_ack_admitted_ticks(),
        Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET),
        "the production respawn reconciliation arm must mark the assertion"
    );

    let mut ticker = app
        .world_mut()
        .remove_resource::<MovementTicker>()
        .expect("ticker resource");
    let packets = transmit_two_after_reconciliation(&mut ticker);
    assert_eq!(packets.len(), 2);
    assert!(
        carries_handled_teleport(&packets[0]),
        "the next transmitted packet must carry HandledTeleport"
    );
    assert!(!carries_handled_teleport(&packets[1]));
    assert_eq!(ticker.pending_teleport_ack_admitted_ticks(), None);
}

#[test]
fn default_off_respawn_reconciliation_stays_inert_and_unflagged() {
    let mut app = wiring_app(authorized_ticker(false), LocalPhysicsController::default());
    submit(
        &mut app,
        1,
        WorldEvent::Respawn(RespawnEvent {
            position: [8.5, 71.620_01, -4.25],
            state: 1,
            runtime_entity_id: 1,
        }),
    );
    app.update();

    let mut ticker = app
        .world_mut()
        .remove_resource::<MovementTicker>()
        .expect("ticker resource");
    assert_eq!(
        ticker.pending_teleport_ack_admitted_ticks(),
        None,
        "default-off must never arm through the production respawn path"
    );
    let packets = transmit_two_after_reconciliation(&mut ticker);
    assert_eq!(packets.len(), 2);
    assert!(
        !packets.iter().any(carries_handled_teleport),
        "default-off must never project the flag onto transmitted bytes"
    );
}

#[test]
fn committed_teleport_snap_correction_dispatches_through_production_reconciliation() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    // Corrections only land on retained ticks, so tick 101 must be simulated.
    physics.advance(
        std::time::Duration::from_millis(100),
        super::integration_tests::forward_physics_input(),
        &super::integration_tests::VersionedFloor(1),
    );
    let mut app = wiring_app(authorized_ticker(true), physics);
    submit(
        &mut app,
        1,
        WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
            // Far beyond the provisional sixteen-block teleport bound.
            position: [40.0, 2.620_01, 0.0],
            delta: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            subject: MovementCorrectionSubject::Player,
            on_ground: true,
            tick: 101,
        }),
    );
    app.update();

    let ticker = app.world().resource::<MovementTicker>();
    assert_eq!(
        ticker.pending_teleport_ack_admitted_ticks(),
        Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET),
        "the production correction arm must dispatch the snap outcome"
    );
    assert_eq!(
        ticker.replayed_corrections_observed(),
        0,
        "the same dispatch must stay counter-only for non-snap shapes"
    );
}

#[test]
fn committed_move_player_teleported_split_dispatches_through_production() {
    // MovePlayer mode Teleport arms without the provisional opt-in.
    let mut marked = wiring_app(authorized_ticker(false), LocalPhysicsController::default());
    submit(
        &mut marked,
        1,
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 1,
            position: [30.5, 71.620_01, 0.5],
            teleported: true,
            source_tick: 90,
            ..MovePlayerEvent::default()
        }),
    );
    marked.update();
    assert_eq!(
        marked
            .world()
            .resource::<MovementTicker>()
            .pending_teleport_ack_admitted_ticks(),
        Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET),
        "the production MovePlayer arm must mark an explicit teleport"
    );

    let mut ticker = marked
        .world_mut()
        .remove_resource::<MovementTicker>()
        .expect("ticker resource");
    let packets = transmit_two_after_reconciliation(&mut ticker);
    assert_eq!(packets.len(), 2);
    assert!(carries_handled_teleport(&packets[0]));
    assert!(!carries_handled_teleport(&packets[1]));
    assert_eq!(ticker.pending_teleport_ack_admitted_ticks(), None);

    // Counting leg: an unmarked local MovePlayer counts without arming.
    let mut unmarked = wiring_app(authorized_ticker(true), LocalPhysicsController::default());
    submit(
        &mut unmarked,
        1,
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 1,
            position: [0.75, 70.0, 0.5],
            teleported: false,
            source_tick: 91,
            ..MovePlayerEvent::default()
        }),
    );
    unmarked.update();
    let ticker = unmarked.world().resource::<MovementTicker>();
    assert_eq!(
        ticker.pending_teleport_ack_admitted_ticks(),
        None,
        "an unmarked local MovePlayer must not arm"
    );
    assert_eq!(
        ticker.unmarked_move_players_observed(),
        1,
        "the production MovePlayer split must keep counting unmarked moves"
    );
}

#[test]
fn change_dimension_clears_an_armed_assertion_through_production_reconciliation() {
    let mut armed = authorized_ticker(true);
    // Arming itself has dedicated witnesses above; this witness pins only the
    // production clearing boundary on CommittedControlEvent::ChangeDimension.
    armed.note_server_teleport(ServerTeleportKind::CorrectionSnap);
    let mut app = wiring_app(armed, LocalPhysicsController::default());
    submit(
        &mut app,
        1,
        WorldEvent::ChangeDimension(ChangeDimensionEvent {
            dimension: 1,
            position: [240.75, 82.0, -17.25],
            ..Default::default()
        }),
    );
    app.update();

    let ticker = app.world().resource::<MovementTicker>();
    assert_eq!(
        ticker.pending_teleport_ack_admitted_ticks(),
        None,
        "the production dimension boundary must clear the armed assertion"
    );
    assert_eq!(
        ticker.completed_tick(),
        100,
        "a dimension switch must preserve the input tick clock"
    );
    let physics = app.world().resource::<LocalPhysicsController>();
    assert_eq!(physics.state().unwrap().tick, 100);
    assert_eq!(physics.network_position(), Some([240.75, 82.0, -17.25]));
    let world = app.world().resource::<ClientWorld>();
    assert!(world.dimension_transfer.active());
}

#[test]
fn dimension_destination_controls_preserve_the_client_input_tick() {
    for source in [MovementSource::Physics, MovementSource::FreeCamera] {
        for wire_tick in [0, 10_000] {
            let mut ticker = authorized_ticker(false);
            ticker.set_source(source);
            let mut app = wiring_app(ticker, LocalPhysicsController::default());
            submit(
                &mut app,
                1,
                WorldEvent::ChangeDimension(ChangeDimensionEvent {
                    dimension: 1,
                    position: [240.75, 82.0, -17.25],
                    ..Default::default()
                }),
            );
            app.update();
            let destination = [242.5, 83.0, -15.5];
            submit(
                &mut app,
                2,
                WorldEvent::MovePlayer(MovePlayerEvent {
                    runtime_id: 1,
                    position: destination,
                    teleported: true,
                    mode: protocol::MovePlayerMode::Teleport,
                    source_tick: wire_tick,
                    ..MovePlayerEvent::default()
                }),
            );
            app.update();
            assert_eq!(
                app.world().resource::<MovementTicker>().completed_tick(),
                100
            );
            let physics = app.world().resource::<LocalPhysicsController>();
            if matches!(source, MovementSource::Physics) {
                assert_eq!(physics.state().unwrap().tick, 100);
                assert_eq!(physics.network_position(), Some(destination));
            } else {
                assert!(!physics.is_active());
            }
            let respawn = [244.5, 84.0, -14.5];
            submit(
                &mut app,
                3,
                WorldEvent::Respawn(RespawnEvent {
                    position: respawn,
                    state: 1,
                    runtime_entity_id: 1,
                }),
            );
            app.update();
            let ticker = app.world().resource::<MovementTicker>();
            assert_eq!(ticker.completed_tick(), 100);
            assert_eq!(ticker.next_tick(), 101);
            let physics = app.world().resource::<LocalPhysicsController>();
            if matches!(source, MovementSource::Physics) {
                assert_eq!(physics.state().unwrap().tick, 100);
                assert_eq!(physics.network_position(), Some(respawn));
            } else {
                assert!(!physics.is_active());
            }
            assert!(
                app.world()
                    .resource::<ClientWorld>()
                    .dimension_transfer
                    .active()
            );
        }
    }
}

#[test]
fn dimension_transfer_starts_loading_and_tracks_later_server_teleports() {
    let mut app = wiring_app(authorized_ticker(false), LocalPhysicsController::default());
    let (network, mut packets) = crate::runtime::network::NetworkHandle::stub_capturing_packets();
    app.insert_resource(network);
    app.add_systems(
        Update,
        advance_dimension_transfer.after(reconcile_world_stream_before_physics),
    );
    submit(
        &mut app,
        1,
        WorldEvent::ChangeDimension(ChangeDimensionEvent {
            dimension: 1,
            position: [0.0, 4000.0, 0.0],
            loading_screen_id: Some(42),
            ..Default::default()
        }),
    );
    submit(
        &mut app,
        2,
        WorldEvent::DimensionChangeAck { runtime_id: 0 },
    );
    let destination = [240.5, 82.0, -17.25];
    submit(
        &mut app,
        3,
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 1,
            position: destination,
            teleported: true,
            ..Default::default()
        }),
    );
    app.update();
    let outgoing = packets.drain();
    assert_eq!(outgoing.len(), 1);
    let session = protocol::BedrockSession { shield_item_id: 0 };
    assert_eq!(
        protocol::encode(&outgoing[0], &session).unwrap(),
        protocol::encode(
            &protocol::loading_screen_packet(protocol::LoadingScreenPhase::Start, Some(42)),
            &session
        )
        .unwrap()
    );
    let world = app.world().resource::<ClientWorld>();
    assert!(world.dimension_transfer.active());
    assert!(world.dimension_transfer.waiting_for_switch());
    assert_eq!(
        world
            .stream
            .as_ref()
            .unwrap()
            .resolved_server_position()
            .position,
        destination
    );
    assert!(
        app.world()
            .resource::<MovementTicker>()
            .can_advance_physics_frame()
    );
    app.update();
    assert!(
        packets.drain().is_empty(),
        "the destination terrain is still absent"
    );
}
