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

/// Runs one tick with `input` and queues the sample.
fn tick_with(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    input: MovementInput,
    context: PhysicsSampleContext,
) -> PhysicsMovementSample {
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
    assert_eq!(physics.apply_server_movement_flags(101, start), None);
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

/// A stamped spin start enters the live state at once and replays from its
/// stamp. Replay copies the spin flag back but not its counter, which vanilla
/// keeps in a separate component, so the live spin keeps its own lifetime.
#[test]
fn a_retained_server_spin_start_replays_from_its_stamp() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let start = flags(|flags| flags.spinning = Some(true));
    assert_eq!(physics.apply_server_movement_flags(101, start), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.state().unwrap().spin_attack_ticks, Some(0));
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 0);
}

/// An unstamped `UsingItem` write always reaches the item-use owner, which
/// compares it with its use; a stamped one only when it differs from the
/// following frame's capture.
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
    // Each stamp compares with the following frame's capture: 102 and 103 use.
    for (tick, value, adopted) in [
        (101, true, None),
        (102, true, None),
        (101, false, Some(false)),
        (102, false, Some(false)),
        (0, true, Some(true)),
        (0, false, Some(false)),
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
    assert_eq!(physics.apply_server_movement_flags(102, start), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 102, &VersionedFloor(1)).unwrap();
    // Replay keeps the live spin counter, which the write started at zero.
    let expected = physics.state().cloned();
    assert_eq!(expected.as_ref().unwrap().spin_attack_ticks, Some(0));
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    assert_eq!(
        physics.state().unwrap().spin_attack_ticks,
        Some(0),
        "the spin written before tick 103 replays again"
    );
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
}

/// A later write with the same stamp compares with the frame's capture, as
/// vanilla's flag correction does, so a clear matching it leaves the start.
#[test]
fn a_same_stamp_clear_compares_with_the_frame_capture() {
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
    assert_eq!(physics.apply_server_movement_flags(101, set(true)), None);
    assert_eq!(physics.apply_server_movement_flags(101, set(false)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
    assert_eq!(physics.take_server_using_item(), Some(true));
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
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
    assert_eq!(physics.apply_server_movement_flags(102, clear), None);
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
    assert_eq!(physics.apply_server_movement_flags(102, clear), None);
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

/// A tickless frame between a server spin set and clear, and a replay after
/// them, leave no stop action: each tick's baseline is the word it starts with.
#[test]
fn a_tickless_frame_and_replay_report_no_server_spin_action() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let write = |value| flags(|flags| flags.spinning = Some(value));
    physics.apply_server_movement_flags(0, write(true));
    let frame = physics.advance_with_context(
        Duration::ZERO,
        MovementInput::default(),
        PhysicsSampleContext::default(),
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

/// A hard anchor such as respawn discards a spin counter a clear saved, so a
/// new life's spin counts from zero.
#[test]
fn a_reanchor_discards_a_saved_spin_counter() {
    let (mut physics, mut ticker) = airborne();
    physics.request_riptide(3, [0.0; 2]);
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let tick = physics.state().unwrap().tick;
    let write = |value| flags(|flags| flags.spinning = Some(value));
    physics.apply_server_movement_flags(0, write(false));
    physics.reanchor_network_position([0.0, 65.620_01, 0.0], tick, false);
    physics.apply_server_movement_flags(0, write(true));
    assert_eq!(physics.state().unwrap().spin_attack_ticks, Some(0));
}

/// After a replay applied a stamped spin start, a later write with the same
/// stamp compares with the frame's capture, as vanilla's does, and leaves it.
#[test]
fn a_replayed_stamped_write_is_not_the_base_for_later_writes() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let write = |value| flags(|flags| flags.spinning = Some(value));
    assert_eq!(physics.apply_server_movement_flags(101, write(true)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.apply_server_movement_flags(101, write(false)), None);
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
}

/// A replay from an earlier tick refreshes the captures of the frames it
/// rebuilt, so a live write no longer serves as their base afterwards.
#[test]
fn an_earlier_replay_refreshes_captured_live_writes() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let write = |value| flags(|flags| flags.spinning = Some(value));
    assert_eq!(physics.apply_server_movement_flags(0, write(true)), None);
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    assert_eq!(physics.apply_server_movement_flags(102, write(false)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 102, &VersionedFloor(1)).unwrap();
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.apply_server_movement_flags(102, write(true)), None);
}

/// One stamped update fixes its comparison bases before recording any field,
/// so an item-use change cannot make its pose compare with the live glide.
#[test]
fn one_update_compares_every_flag_with_the_same_base() {
    let (mut physics, mut ticker) = airborne();
    let jump = |jumping| MovementInput {
        jumping,
        ..MovementInput::default()
    };
    for jumping in [false, false, true] {
        tick_with(&mut physics, &mut ticker, jump(jumping), elytra(true));
    }
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    let update = flags(|flags| {
        flags.using_item = Some(true);
        flags.gliding = Some(true);
    });
    assert_eq!(physics.apply_server_movement_flags(101, update), None);
}

/// A live write captured with a frame stays that frame's base after a stamped
/// correction replays, so a later echo of the live value changes nothing.
#[test]
fn a_captured_live_write_stays_the_base_after_a_correction() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let write = |value| flags(|flags| flags.spinning = Some(value));
    assert_eq!(physics.apply_server_movement_flags(0, write(true)), None);
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    assert_eq!(physics.apply_server_movement_flags(101, write(false)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.apply_server_movement_flags(101, write(true)), None);
}

/// An item-only stamped write replays like other flags, so once applied a
/// later write equal to the frame's capture changes nothing.
#[test]
fn an_item_only_stamped_write_settles_through_replay() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    assert_eq!(physics.apply_server_movement_flags(101, using(true)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
    physics.apply_server_movement_flags(101, using(false));
    assert_eq!(physics.take_server_using_item(), None);
}

/// A write compares with its own frame's capture even while another stamped
/// write awaits replay: vanilla's flag correction has no pending type.
#[test]
fn a_write_compares_with_its_capture_while_another_awaits_replay() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let write = |value| flags(|flags| flags.spinning = Some(value));
    assert_eq!(physics.apply_server_movement_flags(101, write(true)), None);
    assert_eq!(physics.apply_server_movement_flags(102, write(false)), None);
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
}

/// A server pose write keeps flight, which vanilla holds beside pose flags.
#[test]
fn a_server_pose_write_keeps_flight() {
    let (mut physics, mut ticker) = airborne();
    let flying = PhysicsSampleContext {
        mode_intent: super::super::ModeIntent {
            can_fly: true,
            flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
            ..Default::default()
        },
        ..PhysicsSampleContext::default()
    };
    tick(&mut physics, &mut ticker, flying);
    assert_eq!(physics.mode(), sim::MovementMode::Flying);
    let crawl = flags(|flags| flags.crawling = Some(true));
    physics.apply_server_movement_flags(0, crawl);
    assert_eq!(physics.mode(), sim::MovementMode::Flying);
    let sample = tick(&mut physics, &mut ticker, flying);
    assert_eq!(sample.processed.mode, sim::MovementMode::Flying);
}

/// Respawn retires a server spin baseline the old life left for the next
/// input, so the new life reports no stop action.
#[test]
fn respawn_retires_pending_server_spin_baselines() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    physics.apply_server_movement_flags(0, flags(|flags| flags.spinning = Some(true)));
    ticker.discard_respawn_actions(&mut physics);
    let tick_now = physics.state().unwrap().tick;
    physics.reanchor_network_position([0.0, 65.620_01, 0.0], tick_now, false);
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_SPIN_ATTACK), 0);
}

/// A current-stamped item-use write reaches the item-use owner once; the
/// tick that captures it does not hand it over again.
#[test]
fn a_queued_item_use_write_reaches_the_owner_once() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(101, using);
    assert_eq!(physics.take_server_using_item(), Some(true));
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert_eq!(physics.take_server_using_item(), None);
}

/// An unstamped item-use write always reaches the item-use owner, which
/// compares it with the use it actually holds.
#[test]
fn an_unstamped_item_use_write_always_reaches_the_owner() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(false));
    physics.apply_server_movement_flags(0, using);
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// A tick that fails keeps a queued correction for its retry, so a later
/// rewind through that frame still applies it.
#[test]
fn a_failed_tick_keeps_its_queued_correction() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let spin = flags(|flags| flags.spinning = Some(true));
    assert_eq!(physics.apply_server_movement_flags(102, spin), None);
    let failed = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &UnavailableWorld,
    );
    assert!(failed.samples.is_empty());
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let expected = physics.state().unwrap().spin_attack_ticks;
    assert!(expected.is_some());
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.state().unwrap().spin_attack_ticks, expected);
}

/// The first tick after a reanchor also starts from the word server writes
/// left, so a server glide start reports no start action.
#[test]
fn the_first_tick_after_a_reanchor_reports_no_server_start() {
    let (mut physics, mut ticker) = airborne();
    let start = flags(|flags| flags.gliding = Some(true));
    assert_eq!(physics.apply_server_movement_flags(0, start), None);
    let sample = tick(&mut physics, &mut ticker, elytra(true));
    assert_eq!(sample.processed.mode, sim::MovementMode::Gliding);
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
}

/// Replay refreshes later frames' item-use captures with a corrected item-use
/// bit, so a later stamped clear compares with the use the correction began.
#[test]
fn replay_carries_a_corrected_item_use_into_later_captures() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    assert_eq!(physics.apply_server_movement_flags(101, using(true)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
    assert_eq!(physics.apply_server_movement_flags(102, using(false)), None);
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// A stamped flag write schedules no replay of its own, as vanilla's flag
/// correction has no pending type: a spin stamped long ago starts live now.
#[test]
fn a_stamped_flag_write_schedules_no_replay() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..25 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let spin = flags(|flags| flags.spinning = Some(true));
    assert_eq!(physics.apply_server_movement_flags(101, spin), None);
    assert_eq!(physics.state().unwrap().spin_attack_ticks, Some(0));
}

/// Capturing a queued correction does not apply it again, so an unstamped
/// clear after a current-stamped set keeps the spin cleared.
#[test]
fn capturing_a_queued_correction_keeps_a_later_clear() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let write = |value| flags(|flags| flags.spinning = Some(value));
    physics.apply_server_movement_flags(101, write(true));
    physics.apply_server_movement_flags(0, write(false));
    let sample = tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert!(!sample.processed.spinning);
}

/// A current-stamped item-use write keeps its correction for the next frame
/// even when an unstamped write follows, so replay through that frame
/// restores it.
#[test]
fn a_current_stamped_item_use_write_keeps_its_correction() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = |value| flags(|flags| flags.using_item = Some(value));
    physics.apply_server_movement_flags(101, using(true));
    physics.apply_server_movement_flags(0, using(false));
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    physics.take_server_using_item();
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
}

/// Replay hands the item-use bit its corrections leave to the item-use owner,
/// whatever packet arrived last.
#[test]
fn replay_hands_the_final_item_use_to_the_owner() {
    let (mut physics, mut ticker) = airborne();
    for using in [false, false, false, true] {
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
    physics.apply_server_movement_flags(103, using(false));
    physics.apply_server_movement_flags(101, using(true));
    assert_eq!(physics.take_server_using_item(), Some(true));
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// Replay copies its whole flag word back, as vanilla's fixed copy-back mask
/// does, so a live spin a current-stamped write started for the next frame
/// gives way to a replay whose ticks never spin.
#[test]
fn replay_overwrites_a_live_flag_written_after_its_last_tick() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let spin = flags(|flags| flags.spinning = Some(true));
    assert_eq!(physics.apply_server_movement_flags(104, spin), None);
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
}

/// A current-stamped item-use write compares with a local release made
/// after the last capture, so its correction is retained and a replay
/// through the next frame restores it.
#[test]
fn a_stamped_item_use_write_compares_with_a_local_release() {
    let (mut physics, mut ticker) = airborne();
    let using = MovementInput {
        item_use_movement_modifier: Some(0.35),
        ..MovementInput::default()
    };
    tick_with(
        &mut physics,
        &mut ticker,
        using,
        PhysicsSampleContext::default(),
    );
    physics.write_local_using_item(false);
    let write = |value| flags(|flags| flags.using_item = Some(value));
    physics.apply_server_movement_flags(101, write(true));
    physics.apply_server_movement_flags(0, write(false));
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    physics.take_server_using_item();
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
}

/// A write stamped past the current tick, whose frame does not exist yet,
/// joins the current frame's corrections, so a replay through that frame
/// applies it after a later unstamped clear.
#[test]
fn a_future_stamped_write_joins_the_current_frame() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let write = |value| flags(|flags| flags.spinning = Some(value));
    physics.apply_server_movement_flags(105, write(true));
    physics.apply_server_movement_flags(0, write(false));
    physics.queue_server_motion([0.0; 3], 103);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 103, &VersionedFloor(1)).unwrap();
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
}

/// A stale write whose clamped frame has no following frame yet applies live
/// and waits for the next capture instead of being dropped.
#[test]
fn a_stale_write_without_a_following_frame_applies_live() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let write = |value| flags(|flags| flags.spinning = Some(value));
    physics.apply_server_movement_flags(0, write(true));
    physics.apply_server_movement_flags(50, write(false));
    assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
}

/// A spin that starts and expires within a replay stays expired, although
/// the original ticks also ended without a spin.
#[test]
fn replay_keeps_a_spin_that_expired_within_it() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..25 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let spin = flags(|flags| flags.spinning = Some(true));
    assert_eq!(physics.apply_server_movement_flags(101, spin), None);
    assert_eq!(physics.state().unwrap().spin_attack_ticks, Some(0));
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
}

/// A retained glide start that the replayed tick ends at once (no elytra)
/// counts as a replayed change, so the live glide the write began ends too.
#[test]
fn replay_ends_a_glide_its_correction_started_and_ended() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let glide = flags(|flags| flags.gliding = Some(true));
    assert_eq!(physics.apply_server_movement_flags(102, glide), None);
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_ne!(physics.mode(), sim::MovementMode::Gliding);
}

/// A local item-use start or end supersedes a server item-use write still
/// waiting for the item-use owner, as vanilla's start and stop write the
/// actual flag the next tick reads.
#[test]
fn a_local_item_use_write_supersedes_a_pending_server_write() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(false));
    physics.apply_server_movement_flags(0, using);
    physics.write_local_using_item(true);
    assert_eq!(physics.take_server_using_item(), None);
}

/// Replay copies its item-use bit back even when no correction wrote it, so
/// a live set queued for a later frame gives way to the replayed clear.
#[test]
fn replay_copies_back_an_item_use_bit_no_correction_wrote() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(104, using);
    assert_eq!(physics.take_server_using_item(), Some(true));
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// A server swim start between ticks is the flag the next tick's swim blend
/// reads, so the blend starts rising on that tick.
#[test]
fn a_server_swim_start_advances_the_swim_blend_on_the_next_tick() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let swim = flags(|flags| flags.swimming = Some(true));
    physics.apply_server_movement_flags(0, swim);
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert!(physics.state().unwrap().swim_amount > 0.0);
}
