//! Server pose, Riptide spin and item-use flags enter local state as vanilla's
//! flag component writes do: they start or end the state without actions, and
//! the client's own rules then continue or end it.

use super::*;

/// An airborne controller and outbox, high enough for a full spin or glide.
fn airborne() -> (LocalPhysicsController, MovementTicker) {
    let origin = [0.0, 65.620_01, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    (physics, ticker)
}

/// A context whose chest slot holds an unbroken elytra.
fn elytra(ready: bool) -> PhysicsSampleContext {
    PhysicsSampleContext {
        mode_intent: super::super::ModeIntent {
            elytra_ready: ready,
            ..Default::default()
        },
        ..PhysicsSampleContext::default()
    }
}

/// Runs one tick with `input`, carrying adopted server flags into its input as
/// the movement frame does, and queues the sample.
fn tick_with(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    input: MovementInput,
    context: PhysicsSampleContext,
) -> PhysicsMovementSample {
    let server = physics.take_server_control_flags();
    let context = PhysicsSampleContext {
        input: super::super::TickInput {
            server_mode: server.and_then(|flags| flags.mode),
            server_spinning: server.and_then(|flags| flags.spinning),
            ..context.input
        },
        ..context
    };
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        input,
        context,
        &VersionedFloor(1),
    );
    let sample = frame
        .samples
        .into_iter()
        .next()
        .expect("one completed tick");
    ticker.enqueue_completed_physics(sample.clone()).unwrap();
    sample
}

fn tick(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    context: PhysicsSampleContext,
) -> PhysicsMovementSample {
    tick_with(physics, ticker, MovementInput::default(), context)
}

/// How many queued inputs carry `flag`.
fn count(ticker: &MovementTicker, flag: PlayerInputFlags) -> usize {
    ticker
        .pending_snapshots()
        .iter()
        .filter(|snapshot| snapshot.flags.bits() & flag.bits() != 0)
        .count()
}

/// A server glide flag starts the glide without a StartGliding action, and the
/// glide's own rules keep an equipped player gliding or end it without an elytra.
#[test]
fn a_live_server_glide_starts_the_pose_and_its_own_rules_continue_it() {
    for (ready, expected) in [
        (true, sim::MovementMode::Gliding),
        (false, sim::MovementMode::Walking),
    ] {
        let (mut physics, mut ticker) = airborne();
        tick(&mut physics, &mut ticker, elytra(ready));
        let start = flags(|flags| flags.gliding = Some(true));
        assert_eq!(physics.apply_server_movement_flags(0, start), None);
        assert_eq!(physics.mode(), sim::MovementMode::Gliding);
        let sample = tick(&mut physics, &mut ticker, elytra(ready));
        assert_eq!(sample.processed.mode, expected, "elytra {ready}");
        assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
        assert_eq!(
            count(&ticker, PlayerInputFlags::STOP_GLIDING),
            usize::from(!ready),
            "only the client's own stop is an action"
        );
    }
}

/// A server pose clear ends a local glide without a stop action.
#[test]
fn a_live_server_pose_clear_reports_no_stop_action() {
    let (mut physics, mut ticker) = airborne();
    for jumping in [false, true] {
        let input = MovementInput {
            jumping,
            ..MovementInput::default()
        };
        tick_with(&mut physics, &mut ticker, input, elytra(true));
    }
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    let clear = flags(|flags| flags.gliding = Some(false));
    assert_eq!(physics.apply_server_movement_flags(0, clear), None);
    let sample = tick(&mut physics, &mut ticker, elytra(true));
    assert_eq!(sample.processed.mode, sim::MovementMode::Walking);
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 1);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_GLIDING), 0);
}

/// A stamped glide start applies from the tick after its stamp through replay;
/// an echo of the local pose at the stamp changes nothing.
#[test]
fn a_retained_server_glide_start_replays_from_its_stamp() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    let walking = flags(|flags| flags.gliding = Some(false));
    assert_eq!(physics.apply_server_movement_flags(101, walking), None);
    let start = flags(|flags| flags.gliding = Some(true));
    assert_eq!(physics.apply_server_movement_flags(101, start), Some(101));
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    let echo = flags(|flags| flags.gliding = Some(true));
    assert_eq!(physics.apply_server_movement_flags(102, echo), None);
}

/// A server spin flag starts a spin without a start action; the spin's own
/// lifetime ends it on its twentieth tick with one stop action.
#[test]
fn a_live_server_spin_lasts_the_vanilla_lifetime() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let start = flags(|flags| flags.spinning = Some(true));
    assert_eq!(physics.apply_server_movement_flags(0, start), None);
    assert_eq!(
        physics.state().unwrap().spin_attack_ticks,
        Some(0),
        "the spin starts its own lifetime"
    );
    for index in 0..20 {
        let sample = tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
        assert_eq!(sample.processed.spinning, index < 19, "tick {index}");
    }
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 1);
}

/// A server clear cancels a local Riptide spin without a stop action.
#[test]
fn a_live_server_spin_clear_cancels_the_local_spin() {
    let (mut physics, mut ticker) = airborne();
    physics.request_riptide(3, [0.0; 2]);
    for _ in 0..3 {
        assert!(
            tick(&mut physics, &mut ticker, PhysicsSampleContext::default())
                .processed
                .spinning
        );
    }
    let clear = flags(|flags| flags.spinning = Some(false));
    assert_eq!(physics.apply_server_movement_flags(0, clear), None);
    assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
    let sample = tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert!(!sample.processed.spinning);
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 1);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 0);
}

/// A stamped spin start enters the state after its stamp and replays from there.
#[test]
fn a_retained_server_spin_start_replays_from_its_stamp() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let start = flags(|flags| flags.spinning = Some(true));
    assert_eq!(physics.apply_server_movement_flags(101, start), Some(101));
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.state().unwrap().spin_attack_ticks, Some(2));
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 0);
}

/// Live `UsingItem` writes always reach the item-use owner; a stamped one only
/// when it differs from the local use during its tick.
#[test]
fn server_item_use_follows_the_local_use_at_its_stamp() {
    let (mut physics, mut ticker) = airborne();
    for using in [false, true, true] {
        let input = MovementInput {
            item_use_movement_modifier: using.then_some(0.35),
            ..MovementInput::default()
        };
        tick_with(
            &mut physics,
            &mut ticker,
            input,
            PhysicsSampleContext::default(),
        );
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    for (tick, value, adopted) in [
        (101, false, None),
        (102, true, None),
        (101, true, Some(true)),
        (102, false, Some(false)),
        (0, true, Some(true)),
    ] {
        physics.apply_server_movement_flags(tick, using(value));
        assert_eq!(
            physics.take_server_using_item(),
            adopted,
            "stamp {tick}, using {value}"
        );
    }
}

/// Stamped spin and glide writes stay in their frame when a later replay
/// starts from an earlier tick.
#[test]
fn retained_server_writes_survive_an_earlier_rewind() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    let start = flags(|flags| {
        flags.gliding = Some(true);
        flags.spinning = Some(true);
    });
    assert_eq!(physics.apply_server_movement_flags(102, start), Some(102));
    reconcile_timeline_rewind(&mut ticker, &mut physics, 102, &VersionedFloor(1)).unwrap();
    let expected = physics.state().cloned();
    assert_eq!(expected.as_ref().unwrap().spin_attack_ticks, Some(2));
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    assert_eq!(
        physics.state().unwrap().spin_attack_ticks,
        Some(2),
        "the spin written before tick 103 replays again"
    );
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
}

/// A later write with the same stamp replaces an earlier one, so a clear
/// withdraws a server-started glide, spin and item use.
#[test]
fn a_later_write_with_the_same_stamp_withdraws_a_server_start() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    let set = |value| {
        flags(|flags| {
            flags.gliding = Some(value);
            flags.spinning = Some(value);
            flags.using_item = Some(value);
        })
    };
    assert_eq!(
        physics.apply_server_movement_flags(101, set(true)),
        Some(101)
    );
    assert_eq!(
        physics.apply_server_movement_flags(101, set(false)),
        Some(101)
    );
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.mode(), sim::MovementMode::Walking);
    assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
    assert_eq!(physics.take_server_using_item(), Some(false));
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 0);
}

/// Respawn discards a server item-use write the old life left pending.
#[test]
fn respawn_discards_a_pending_server_item_use() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(0, using);
    ticker.discard_respawn_actions(&mut physics);
    assert_eq!(physics.take_server_using_item(), None);
}

/// A clear and a set before the next tick keep the spin's counter, as
/// vanilla's counter outlives its flag until a tick processes the clear.
#[test]
fn a_clear_and_set_before_a_tick_keep_the_spin_counter() {
    let (mut physics, mut ticker) = airborne();
    physics.request_riptide(3, [0.0; 2]);
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let counter = physics.state().unwrap().spin_attack_ticks;
    assert!(counter.is_some_and(|ticks| ticks > 0));
    let write = |value| flags(|flags| flags.spinning = Some(value));
    physics.apply_server_movement_flags(0, write(false));
    physics.apply_server_movement_flags(0, write(true));
    assert_eq!(physics.state().unwrap().spin_attack_ticks, counter);
}

/// A stamped spin cancel also applies to the live state at once, so it holds
/// even when no replay follows.
#[test]
fn a_stamped_spin_cancel_applies_live() {
    let (mut physics, mut ticker) = airborne();
    physics.request_riptide(3, [0.0; 2]);
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let clear = flags(|flags| flags.spinning = Some(false));
    assert_eq!(physics.apply_server_movement_flags(102, clear), Some(102));
    assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
}

/// A live glide and spin write survives a later replay through the tick it
/// preceded, whatever the stamp it was delivered with.
#[test]
fn live_server_writes_survive_a_replay_through_their_tick() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, elytra(true));
    let start = flags(|flags| {
        flags.gliding = Some(true);
        flags.spinning = Some(true);
    });
    assert_eq!(physics.apply_server_movement_flags(101, start), None);
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    let expected = physics.state().unwrap().spin_attack_ticks;
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    assert_eq!(physics.state().unwrap().spin_attack_ticks, expected);
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
}

/// A stamped spin clear keeps its action baseline when no replay follows, so
/// the next input reports no stop action.
#[test]
fn a_stamped_spin_clear_without_replay_reports_no_stop_action() {
    let (mut physics, mut ticker) = airborne();
    physics.request_riptide(3, [0.0; 2]);
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let clear = flags(|flags| flags.spinning = Some(false));
    assert_eq!(physics.apply_server_movement_flags(102, clear), Some(102));
    let sample = tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert!(!sample.processed.spinning);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 0);
}

/// A glide clear and set before a tick keep the glide counter, so a jump
/// after eleven gliding ticks still ends the glide.
#[test]
fn a_glide_clear_and_set_before_a_tick_keep_the_glide_counter() {
    let (mut physics, mut ticker) = airborne();
    let jump = |jumping| MovementInput {
        jumping,
        ..MovementInput::default()
    };
    tick_with(&mut physics, &mut ticker, jump(false), elytra(true));
    for _ in 0..12 {
        tick_with(&mut physics, &mut ticker, jump(true), elytra(true));
    }
    tick_with(&mut physics, &mut ticker, jump(false), elytra(true));
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    let write = |value| flags(|flags| flags.gliding = Some(value));
    physics.apply_server_movement_flags(0, write(false));
    physics.apply_server_movement_flags(0, write(true));
    let sample = tick_with(&mut physics, &mut ticker, jump(true), elytra(true));
    assert_eq!(sample.processed.mode, sim::MovementMode::Walking);
}

/// A replay that rebuilds the live spin also replaces the baseline a tickless
/// frame already took, so a server clear after a set reports no stop action.
#[test]
fn a_replay_rebuilds_baselines_a_tickless_frame_took() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let write = |value| flags(|flags| flags.spinning = Some(value));
    physics.apply_server_movement_flags(0, write(true));
    let server = physics.take_server_control_flags();
    let tickless = PhysicsSampleContext {
        input: super::super::TickInput {
            server_spinning: server.and_then(|flags| flags.spinning),
            ..Default::default()
        },
        ..PhysicsSampleContext::default()
    };
    let frame = physics.advance_with_context(
        Duration::ZERO,
        MovementInput::default(),
        tickless,
        &VersionedFloor(1),
    );
    assert!(frame.samples.is_empty());
    physics.apply_server_movement_flags(0, write(false));
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    let sample = tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert!(!sample.processed.spinning);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 0);
}
