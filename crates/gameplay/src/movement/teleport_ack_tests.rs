//! HandledTeleport coverage for MovePlayer teleports and provisional routes.
//!
//! MovePlayer acknowledgements are unconditional. Correction-snap and respawn
//! routes use the opt-in construction flag without mutating the environment.
//! The expiry budget remains provisional.

use std::time::Duration;

use super::integration_tests::{VersionedFloor, evidence_context, forward_physics_input};
use super::teleport_ack::{
    TELEPORT_ACK_ADMITTED_TICK_BUDGET, enabled_for_env_value, expired_marker,
};
use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionMode,
    PhysicsCorrectionOutcome, PhysicsMovementSample, PhysicsSampleContext, ProcessedMovementState,
    ServerTeleportKind, flush_player_auth_inputs, reconcile_candidate_physics_correction,
    reconcile_committed_correction,
};
use protocol::{PlayerInputFlags, PlayerInputMode, player_auth_input_trace_sample};
use sim::{CollisionIdSpace, CollisionRegistryIdentity, WorldCollisionIdentity};

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

fn completed_sample(tick: u64, position: [f32; 3]) -> PhysicsMovementSample {
    PhysicsMovementSample {
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
        input: Default::default(),
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

fn physics_ticker(teleport_ack: bool) -> MovementTicker {
    let mut ticker = MovementTicker::default();
    ticker.testing_set_teleport_ack(teleport_ack);
    ticker
}

/// Standard authorized session fixture with the settle window lifted so
/// transport assertions are orthogonal to settling (dedicated suppression
/// coverage arms the window explicitly below).
fn armed_session_ticker(teleport_ack: bool) -> MovementTicker {
    let mut ticker = physics_ticker(teleport_ack);
    ticker.reset(7, 100, [0.0, 70.0, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
}

fn flush_capturing(
    ticker: &mut MovementTicker,
    fail_first_with: Option<&'static str>,
) -> Vec<protocol::Packet> {
    let mut packets = Vec::new();
    let result =
        flush_player_auth_inputs(ticker, 8, Some(evidence_context()), |_identity, packet| {
            match fail_first_with {
                Some(error) if packets.is_empty() => Err(error),
                _ => {
                    packets.push(packet);
                    Ok::<_, &'static str>(())
                }
            }
        });
    if fail_first_with.is_some() {
        assert!(result.is_err(), "fixture requires the send failure");
    } else {
        result.unwrap();
    }
    packets
}

fn carries_handled_teleport(packet: &protocol::Packet) -> bool {
    player_auth_input_trace_sample(packet)
        .expect("PlayerAuthInput projects for the trace table")
        .flag_names
        .contains(&"HandledTeleport")
}

#[test]
fn env_gate_requires_exactly_the_digit_one() {
    use std::ffi::OsString;
    assert!(enabled_for_env_value(Some(&OsString::from("1"))));
    assert!(!enabled_for_env_value(None));
    for disabled in ["", "0", "true", "yes", "01", "1 ", " 1", "one"] {
        assert!(
            !enabled_for_env_value(Some(&OsString::from(disabled))),
            "value {disabled:?} must disable the provisional routes"
        );
    }
}

#[test]
fn expiry_marker_renders_the_exact_bounded_single_line_schema() {
    assert_eq!(
        expired_marker(),
        "TELEPORT_ACK={\"schema\":\"rust-mcbe-movement-teleport-ack-v1\",\"phase\":\"expired\",\"budget_admitted_ticks\":40}"
    );
}

#[test]
fn only_qualifying_server_teleports_arm_the_single_shot_assertion() {
    for kind in [
        ServerTeleportKind::CorrectionSnap,
        ServerTeleportKind::MovePlayer,
        ServerTeleportKind::Respawn,
    ] {
        let mut ticker = armed_session_ticker(true);
        ticker.note_server_teleport(kind);
        assert_eq!(
            ticker.pending_teleport_ack_admitted_ticks(),
            Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET),
            "{kind:?} must arm the assertion"
        );
    }

    // Replay-shaped corrections stay counter-only.
    let mut replayed = armed_session_ticker(true);
    replayed.note_replayed_correction();
    assert_eq!(replayed.pending_teleport_ack_admitted_ticks(), None);
    assert_eq!(replayed.replayed_corrections_observed(), 1);

    // Unmarked local MovePlayers stay counter-only.
    let mut unmarked = armed_session_ticker(true);
    unmarked.note_unmarked_local_move_player();
    assert_eq!(unmarked.pending_teleport_ack_admitted_ticks(), None);
    assert_eq!(unmarked.unmarked_move_players_observed(), 1);
}

#[test]
fn committed_correction_outcomes_dispatch_to_the_matching_observation() {
    // The exact dispatch production world.rs performs for every committed
    // correction that mutated prediction state.
    let mut snapped = armed_session_ticker(true);
    snapped.note_committed_correction_outcome(PhysicsCorrectionOutcome::Snapped { tick: 105 });
    assert_eq!(
        snapped.pending_teleport_ack_admitted_ticks(),
        Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET)
    );
    assert_eq!(snapped.replayed_corrections_observed(), 0);

    let mut replayed = armed_session_ticker(true);
    replayed.note_committed_correction_outcome(PhysicsCorrectionOutcome::Replayed {
        corrected_tick: 101,
        replayed_ticks: 2,
    });
    assert_eq!(replayed.pending_teleport_ack_admitted_ticks(), None);
    assert_eq!(replayed.replayed_corrections_observed(), 1);
}

#[test]
fn a_confirming_correction_mutates_no_acknowledgement_state() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );

    let mut ticker = armed_session_ticker(true);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    // An exactly-agreeing server record classifies Confirmed and must reach
    // the acknowledgement state machine as pure silence.
    let network_position = physics.network_position().unwrap();
    let outcome = reconcile_committed_correction(
        &mut ticker,
        &mut physics,
        network_position,
        101,
        true,
        None,
        &VersionedFloor(1),
    )
    .unwrap();

    assert!(outcome.is_none(), "fixture requires a Confirmed shape");
    assert_eq!(ticker.pending_teleport_ack_admitted_ticks(), None);
    assert_eq!(ticker.replayed_corrections_observed(), 0);
    assert_eq!(ticker.unmarked_move_players_observed(), 0);
}

#[test]
fn surface_spawn_resolution_never_marks_and_never_clears_an_armed_assertion() {
    // A client-derived surface-spawn resolve is not a server teleport and is
    // not one of the listed clearing boundaries: an assertion armed by a real
    // earlier server teleport survives it.
    let mut survivor = armed_session_ticker(true);
    survivor.note_server_teleport(ServerTeleportKind::MovePlayer);
    survivor.reanchor_surface_spawn(105, [8.0, 71.620_01, 9.0]);
    assert_eq!(
        survivor.pending_teleport_ack_admitted_ticks(),
        Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET),
        "a client-derived anchor must neither mark nor clear the assertion"
    );

    // ...and resolving one never arms anything by itself.
    let mut untouched = armed_session_ticker(true);
    untouched.reanchor_surface_spawn(105, [8.0, 71.620_01, 9.0]);
    assert_eq!(untouched.pending_teleport_ack_admitted_ticks(), None);
}

#[test]
fn every_queue_clearing_boundary_clears_the_armed_assertion() {
    let kind = ServerTeleportKind::Respawn;

    // StartGame session reset.
    let mut reset = armed_session_ticker(true);
    reset.note_server_teleport(kind);
    reset.reset(8, 40, [0.0; 3]);
    assert_eq!(reset.pending_teleport_ack_admitted_ticks(), None);

    // Deactivation.
    let mut deactivated = armed_session_ticker(true);
    deactivated.note_server_teleport(kind);
    deactivated.deactivate();
    assert_eq!(deactivated.pending_teleport_ack_admitted_ticks(), None);

    // Physics authority fault.
    let mut faulted = armed_session_ticker(true);
    faulted.note_server_teleport(kind);
    faulted.record_physics_fault(super::PhysicsAuthorityFault::Unauthorized);
    assert_eq!(faulted.pending_teleport_ack_admitted_ticks(), None);

    // FreeCamera source transition.
    let mut free_camera = armed_session_ticker(true);
    free_camera.note_server_teleport(kind);
    free_camera.set_source(MovementSource::FreeCamera);
    assert_eq!(free_camera.pending_teleport_ack_admitted_ticks(), None);

    // Dimension change handling: the exact silent clear the world-stream
    // reconciliation now performs on CommittedControlEvent::ChangeDimension.
    let mut dimension = armed_session_ticker(true);
    dimension.note_server_teleport(kind);
    dimension.clear_pending_teleport_ack();
    assert_eq!(dimension.pending_teleport_ack_admitted_ticks(), None);
}

#[test]
fn move_player_without_opt_in_flags_exactly_the_first_transmitted_sample() {
    let mut ticker = armed_session_ticker(false);
    ticker.note_server_teleport(ServerTeleportKind::MovePlayer);
    for tick in 101..104 {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [1.0, 2.0, 3.0]))
            .unwrap();
    }

    let packets = flush_capturing(&mut ticker, None);

    assert_eq!(packets.len(), 3);
    assert!(carries_handled_teleport(&packets[0]));
    assert!(!carries_handled_teleport(&packets[1]));
    assert!(!carries_handled_teleport(&packets[2]));
    assert_eq!(ticker.pending_teleport_ack_admitted_ticks(), None);
}

#[test]
fn move_player_without_opt_in_restores_and_resends_the_flagged_sample() {
    let mut ticker = armed_session_ticker(false);
    ticker.note_server_teleport(ServerTeleportKind::MovePlayer);
    ticker
        .enqueue_completed_physics(completed_sample(101, [1.0, 2.0, 3.0]))
        .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(102, [1.5, 2.0, 3.0]))
        .unwrap();

    let failed = flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |_identity, _packet| Err("full"),
    );
    assert!(matches!(
        failed,
        Err(super::MovementSendError::Transport("full"))
    ));
    let restored = ticker.pending_snapshots();
    assert_eq!(restored.len(), 2);
    assert_ne!(
        restored[0].flags.bits() & PlayerInputFlags::HANDLED_TELEPORT.bits(),
        0,
        "the restored retry sample must keep its projected flag bit"
    );
    assert_eq!(
        ticker.pending_teleport_ack_admitted_ticks(),
        Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET - 2),
        "the two admissions charged the budget; the failed write must not consume the assertion"
    );

    let packets = flush_capturing(&mut ticker, None);
    assert_eq!(packets.len(), 2);
    assert!(carries_handled_teleport(&packets[0]));
    assert!(!carries_handled_teleport(&packets[1]));
    assert_eq!(ticker.pending_teleport_ack_admitted_ticks(), None);
}

#[test]
fn the_provisional_budget_expires_on_admission_forty_one_not_forty() {
    // The armed assertion charges one admitted tick at a time and expires on
    // the admission after its budget is spent. Transmission is now
    // unconditional (every tick), so the first transmitted packet normally
    // rides the assertion long before expiry; the budget guards the case where
    // transmission is deferred (transport backpressure) and no packet ever
    // rides. Exercise the exact boundary through the admission observer, which
    // production calls on every admitted tick.
    let mut ticker = physics_ticker(false);
    ticker.reset(7, 10, [0.0, 70.0, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.note_server_teleport(ServerTeleportKind::MovePlayer);

    for spent in 1..=TELEPORT_ACK_ADMITTED_TICK_BUDGET {
        ticker.observe_admitted_tick_for_teleport_ack();
        assert_eq!(
            ticker.pending_teleport_ack_admitted_ticks(),
            Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET - spent),
            "admission {spent} must charge the budget without expiring",
        );
    }

    ticker.observe_admitted_tick_for_teleport_ack();
    assert_eq!(
        ticker.pending_teleport_ack_admitted_ticks(),
        None,
        "admission 41 expires the spent assertion"
    );
    assert_eq!(ticker.teleport_acks_expired(), 1);
}

#[test]
fn replay_rewrite_preserves_the_flag_bit_on_a_flagged_pending_sample() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 3);

    let mut ticker = armed_session_ticker(true);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    // Confirm tick 101 so the later correction has a retained confirmation.
    let mut confirmed = None;
    flush_player_auth_inputs(
        &mut ticker,
        1,
        Some(evidence_context()),
        |identity, _packet| {
            confirmed = Some(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert!(ticker.acknowledge_physics_send(confirmed.unwrap()));

    // Distant snap arms the assertion; the next transmitted sample carries
    // the flag and stays staged (unacknowledged). Budget-limited to one so
    // exactly one packet leaves.
    ticker.note_server_teleport(ServerTeleportKind::CorrectionSnap);
    let mut flagged_packets = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        1,
        Some(evidence_context()),
        |_identity, packet| {
            flagged_packets.push(packet);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert_eq!(flagged_packets.len(), 1);
    assert!(carries_handled_teleport(&flagged_packets[0]));

    // A small ordinary replay rewrites the staged samples' positions and
    // collision/jump flags without stripping the unrelated flag bit.
    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.25, 2.620_01, 0.0],
        101,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(matches!(outcome, PhysicsCorrectionOutcome::Replayed { .. }));

    let still_pending = &ticker.pending_sends;
    let flagged_tick = still_pending
        .iter()
        .find(|pending| pending.identity.tick == 102)
        .expect("tick 102 remains staged across the replay");
    assert_ne!(
        flagged_tick.sample.snapshot.flags.bits() & PlayerInputFlags::HANDLED_TELEPORT.bits(),
        0,
        "the replay rewrite must preserve the projected HandledTeleport bit"
    );
    let clean_snapshot = ticker
        .pending_snapshots()
        .into_iter()
        .find(|snapshot| snapshot.tick == 103)
        .expect("tick 103 remains queued across the replay");
    assert_eq!(
        clean_snapshot.flags.bits() & PlayerInputFlags::HANDLED_TELEPORT.bits(),
        0
    );
}

#[test]
fn the_assertion_rides_the_first_transmitted_packet() {
    let mut ticker = physics_ticker(true);
    ticker.reset(7, 10, [0.0, 70.0, 0.0]);
    ticker.set_source(MovementSource::Physics);

    ticker.note_server_teleport(ServerTeleportKind::Respawn);

    // Transmission is unconditional, so the assertion rides the very first
    // transmitted packet after arming; subsequent packets stay unflagged.
    ticker
        .enqueue_completed_physics(completed_sample(11, [1.0, 70.0, 0.0]))
        .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(12, [1.0, 70.0, 0.0]))
        .unwrap();
    let packets = flush_capturing(&mut ticker, None);
    assert_eq!(packets.len(), 2, "every admitted tick transmits");
    assert!(carries_handled_teleport(&packets[0]));
    assert!(!carries_handled_teleport(&packets[1]));
    assert_eq!(ticker.pending_teleport_ack_admitted_ticks(), None);
}

#[test]
fn disabled_provisional_routes_stay_inert_and_byte_identical() {
    let kinds = [
        ServerTeleportKind::CorrectionSnap,
        ServerTeleportKind::Respawn,
    ];

    // Unproven routes stay silent unless explicitly enabled.
    let mut disabled = armed_session_ticker(false);
    for kind in kinds {
        disabled.note_server_teleport(kind);
    }
    disabled.note_replayed_correction();
    disabled.note_unmarked_local_move_player();
    assert_eq!(disabled.pending_teleport_ack_admitted_ticks(), None);
    assert_eq!(disabled.teleport_acks_expired(), 0);
    assert_eq!(disabled.replayed_corrections_observed(), 0);
    assert_eq!(disabled.unmarked_move_players_observed(), 0);
    for tick in 101..104 {
        disabled
            .enqueue_completed_physics(completed_sample(tick, [1.0, 2.0, 3.0]))
            .unwrap();
    }
    let disabled_packets = flush_capturing(&mut disabled, None);
    assert_eq!(disabled_packets.len(), 3);
    assert!(
        !disabled_packets.iter().any(carries_handled_teleport),
        "disabled provisional routes must never project the flag"
    );

    // An enabled session that observed no qualifying teleport produces
    // byte-identical packets to the disabled run over identical samples:
    // the feature only ever differs through its own state machine.
    let mut quiet_enabled = armed_session_ticker(true);
    for tick in 101..104 {
        quiet_enabled
            .enqueue_completed_physics(completed_sample(tick, [1.0, 2.0, 3.0]))
            .unwrap();
    }
    let quiet_packets = flush_capturing(&mut quiet_enabled, None);
    assert_eq!(quiet_packets, disabled_packets);
}

#[test]
fn review_expiry_removes_acknowledgement_from_a_restored_retry() {
    let mut ticker = armed_session_ticker(true);
    ticker.note_server_teleport(ServerTeleportKind::MovePlayer);
    ticker
        .enqueue_completed_physics(completed_sample(101, [1.0, 2.0, 3.0]))
        .unwrap();
    assert!(
        flush_player_auth_inputs(
            &mut ticker,
            1,
            Some(evidence_context()),
            |_id, _packet| Err("full")
        )
        .is_err()
    );
    assert!(
        ticker.peek_pending().unwrap().snapshot.flags.bits()
            & PlayerInputFlags::HANDLED_TELEPORT.bits()
            != 0
    );
    for _ in 0..=TELEPORT_ACK_ADMITTED_TICK_BUDGET {
        ticker.observe_admitted_tick_for_teleport_ack();
    }
    assert!(ticker.pending_teleport_ack_admitted_ticks().is_none());
    let mut retried = false;
    flush_player_auth_inputs(&mut ticker, 1, Some(evidence_context()), |_id, packet| {
        retried = true;
        assert!(!carries_handled_teleport(&packet));
        Ok::<_, ()>(())
    })
    .unwrap();
    assert!(retried);
}
