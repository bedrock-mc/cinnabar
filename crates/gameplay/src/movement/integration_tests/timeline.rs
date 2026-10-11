// Tick-stamped authoritative updates entering the rewind timeline.

fn walked_physics(ticks: u64) -> (LocalPhysicsController, MovementTicker) {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for _ in 0..ticks {
        let sample = run_one_tick(&mut physics, &VersionedFloor(1));
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    (physics, ticker)
}

/// Delayed knockback must rewind to its tick instead of waiting for a coincidental replay.
#[test]
fn delayed_server_motion_rewinds_to_its_tick_and_matches_on_time_delivery() {
    let motion = [0.45, 0.42, -0.35];
    let (mut on_time, _) = walked_physics(2);
    assert_eq!(on_time.queue_server_motion(motion, 102), None);
    run_one_tick(&mut on_time, &VersionedFloor(1));
    run_one_tick(&mut on_time, &VersionedFloor(1));

    let (mut delayed, mut ticker) = walked_physics(4);
    assert_eq!(delayed.queue_server_motion(motion, 102), Some(102));
    let outcome =
        reconcile_timeline_rewind(&mut ticker, &mut delayed, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 102,
            replayed_ticks: 2,
        }
    );
    assert_eq!(delayed.state(), on_time.state());
    let pending: Vec<_> = ticker
        .pending_samples()
        .iter()
        .map(|pending| pending.snapshot.position)
        .collect();
    assert_eq!(
        pending.last(),
        delayed.network_position().as_ref(),
        "unsent samples carry the replayed knockback"
    );
}

#[test]
fn stale_server_motion_clamps_to_the_oldest_retained_frame() {
    let (mut physics, _) = walked_physics(3);
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 200, true);
    for _ in 0..3 {
        run_one_tick(&mut physics, &VersionedFloor(1));
    }
    assert_eq!(
        physics.queue_server_motion([0.1, 0.0, 0.0], 150),
        Some(201),
        "older than history: rewind from the oldest retained frame"
    );
}

#[test]
fn current_and_future_server_motion_apply_to_live_state() {
    let (mut physics, _) = walked_physics(2);
    assert_eq!(physics.queue_server_motion([0.3, 0.0, 0.0], 102), None);
    assert_eq!(physics.state().unwrap().velocity.x, f64::from(0.3_f32));
    assert_eq!(physics.queue_server_motion([-0.2, 0.0, 0.0], 150), None);
    assert_eq!(physics.state().unwrap().velocity.x, f64::from(-0.2_f32));
}

fn run_tick_with(physics: &mut LocalPhysicsController, input: MovementInput) {
    let frame = physics.advance(Duration::from_millis(50), input, &VersionedFloor(1));
    assert_eq!(frame.completed_ticks, 1, "{:?}", frame.blocked);
}

/// A delayed movement-speed attribute applies from its tick through replay, not from arrival.
#[test]
fn delayed_movement_speed_rewinds_to_its_tick_and_matches_on_time_delivery() {
    let faster = MovementInput {
        movement_speed: Some(0.2),
        ..forward_physics_input()
    };
    let (mut on_time, _) = walked_physics(2);
    run_tick_with(&mut on_time, faster);
    run_tick_with(&mut on_time, faster);

    let (mut delayed, mut ticker) = walked_physics(4);
    assert_eq!(
        delayed
            .retime_movement_speed(102, attribute(0.2, 0.2, None))
            .unwrap()
            .0,
        Some(102)
    );
    reconcile_timeline_rewind(&mut ticker, &mut delayed, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(delayed.state(), on_time.state());
    assert_eq!(
        delayed
            .retime_movement_speed(102, attribute(0.2, 0.2, None))
            .unwrap()
            .0,
        None,
        "a repeated value changes nothing and needs no replay"
    );
}

#[test]
fn live_and_stale_movement_speed_stamps_leave_retained_inputs_alone() {
    let (mut physics, _) = walked_physics(3);
    assert_eq!(
        physics.retime_movement_speed(0, attribute(0.2, 0.2, None)),
        None
    );
    assert_eq!(
        physics.retime_movement_speed(103, attribute(0.2, 0.2, None)),
        None
    );
    assert_eq!(
        physics.retime_movement_speed(50, attribute(0.2, 0.2, None)),
        None
    );
}

#[test]
fn empty_modifier_server_sprint_runs_at_one_boost_and_custom_speed_survives() {
    fn ground_speed(current: f32, sprinting: bool) -> f64 {
        let (mut physics, _) = walked_physics(0);
        let mut authority =
            crate::movement::speed_authority::LocalMovementSpeedAuthority::default();
        authority.begin_session(7, 0);
        assert!(authority.apply(7, 1, 0, attribute(f64::from(current), current, None)));
        authority.adopt_server_sprinting(Some(sprinting));
        let mut speed = 0.0;
        for _ in 0..40 {
            authority.set_sprinting(sprinting);
            let input = MovementInput {
                sprinting,
                movement_speed: authority.prediction_speed(),
                ..forward_physics_input()
            };
            let before = physics.state().unwrap().position;
            run_tick_with(&mut physics, input);
            let movement = physics.state().unwrap().position - before;
            speed = movement.x.hypot(movement.z) * f64::from(world::TICKS_PER_SECOND);
        }
        speed
    }
    let walking = ground_speed(0.1, false);
    let sprinting = ground_speed(0.13, true);
    assert!((walking - 4.3173).abs() < 0.001, "{walking}");
    assert!((sprinting - 5.6125).abs() < 0.001, "{sprinting}");
    assert!((sprinting / walking - sim::SPRINT_SPEED_MULTIPLIER).abs() < 0.0001);
    let custom = ground_speed(0.12, false);
    assert!((custom / walking - f64::from(0.12_f32 / 0.1_f32)).abs() < 0.0001);
}

#[test]
fn delayed_effective_speed_replays_only_sprint_edges_after_its_stamp() {
    let (mut physics, _) = walked_physics(0);
    for sprinting in [true, true, false, false, true] {
        run_tick_with(&mut physics, sprinting_input(sprinting));
    }
    assert_eq!(physics.queue_server_motion([0.3, 0.2, -0.1], 102), Some(102));
    let (_, speed) = physics
        .retime_movement_speed(101, attribute(f64::from(0.13_f32), 0.1, None))
        .unwrap();
    // Re-entry recalculates from packet default after the empty-modifier stop.
    assert_eq!(speed.prediction_speed(), Some(f64::from(0.1_f32)));
}

fn sprinting_input(sprinting: bool) -> MovementInput {
    MovementInput {
        sprinting,
        ..forward_physics_input()
    }
}

fn flags(
    update: impl FnOnce(&mut client_world::MovementFlagUpdate),
) -> client_world::MovementFlagUpdate {
    let mut flags = client_world::MovementFlagUpdate::default();
    update(&mut flags);
    flags
}

/// A server sprint stop applies from its tick through replay and reaches the live latch.
#[test]
fn delayed_sprint_stop_rewinds_to_its_tick_and_is_adopted_live() {
    let (mut on_time, _) = walked_physics(0);
    for sprinting in [true, true, false, false] {
        run_tick_with(&mut on_time, sprinting_input(sprinting));
    }
    let (mut delayed, mut ticker) = walked_physics(0);
    for _ in 0..4 {
        let frame = delayed.advance(
            Duration::from_millis(50),
            sprinting_input(true),
            &VersionedFloor(1),
        );
        ticker
            .enqueue_completed_physics(frame.samples[0].clone())
            .unwrap();
    }
    let stop = flags(|flags| flags.sprinting = Some(false));
    assert_eq!(delayed.apply_server_movement_flags(102, stop), Some(102));
    reconcile_timeline_rewind(&mut ticker, &mut delayed, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(delayed.state(), on_time.state());
    assert_eq!(delayed.latest_sneak_sprint(), Some((false, false)));
    let adopted = delayed.take_server_control_flags().unwrap();
    assert_eq!(adopted.sprinting, Some(false));
    assert_eq!(adopted.sneaking, None);
}

#[test]
fn a_redundant_flag_echo_changes_nothing() {
    let (mut physics, _) = walked_physics(0);
    for _ in 0..3 {
        run_tick_with(&mut physics, sprinting_input(true));
    }
    let before = physics.state().cloned();
    let echo = flags(|flags| {
        flags.sprinting = Some(true);
        flags.sneaking = Some(false);
    });
    assert_eq!(physics.apply_server_movement_flags(102, echo), None);
    assert_eq!(physics.state().cloned(), before);
    assert_eq!(physics.take_server_control_flags(), None);
}

#[test]
fn a_later_client_transition_outranks_an_older_server_flag() {
    let (mut physics, _) = walked_physics(0);
    for sprinting in [true, true, false, false] {
        run_tick_with(&mut physics, sprinting_input(sprinting));
    }
    let stop = flags(|flags| flags.sprinting = Some(false));
    assert_eq!(physics.apply_server_movement_flags(101, stop), Some(101));
    assert_eq!(
        physics.take_server_control_flags(),
        None,
        "the client already stopped on its own, so the latch is left alone"
    );
}

#[test]
fn a_server_glide_clear_ends_the_retained_glide_and_the_live_mode() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 50.620_01, 0.0], 100, false);
    let context = PhysicsSampleContext {
        mode_intent: super::ModeIntent {
            elytra_ready: true,
            ..super::ModeIntent::default()
        },
        ..PhysicsSampleContext::default()
    };
    for jumping in [false, true, false, false] {
        let input = MovementInput {
            jumping,
            ..MovementInput::default()
        };
        let frame = physics.advance_with_context(
            Duration::from_millis(50),
            input,
            context,
            &VersionedFloor(1),
        );
        assert_eq!(frame.completed_ticks, 1);
    }
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    let clear = flags(|flags| flags.gliding = Some(false));
    assert_eq!(physics.apply_server_movement_flags(102, clear), Some(102));
    assert_eq!(physics.mode(), sim::MovementMode::Walking);
}

#[test]
fn an_unstamped_flag_update_applies_live() {
    let (mut physics, _) = walked_physics(1);
    let stop = flags(|flags| flags.sprinting = Some(false));
    assert_eq!(physics.apply_server_movement_flags(0, stop), None);
    assert_eq!(
        physics
            .take_server_control_flags()
            .and_then(|flags| flags.sprinting),
        Some(false)
    );
}

#[test]
fn start_game_rewind_history_size_follows_the_vanilla_initializer() {
    let mut physics = LocalPhysicsController::default();
    for (wire, expected) in [(20, 20), (0, 1), (65_536, 1), (5_000, 1_000), (-1, 1_000)] {
        physics.set_rewind_history_size(wire);
        assert_eq!(physics.history_capacity(), expected, "wire {wire}");
    }
}

/// A server window longer than the outbox must still replay from its oldest tick.
#[test]
fn a_server_window_longer_than_the_outbox_retains_and_replays_every_tick() {
    let mut physics = LocalPhysicsController::default();
    physics.set_rewind_history_size(40);
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for _ in 0..45 {
        let sample = run_one_tick(&mut physics, &VersionedFloor(1));
        ticker.enqueue_completed_physics(sample).unwrap();
        ticker.pop_pending().unwrap();
    }
    assert_eq!(physics.history_len(), 40);
    let oldest = 106;
    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.5, 2.620_01, 0.0],
        oldest,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    assert_eq!(
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: oldest,
            replayed_ticks: 39,
        }
    );
}

/// A nearby tick-stamped teleport rewinds with motion cleared; a distant one resets history.
#[test]
fn a_nearby_retained_teleport_rewinds_and_a_distant_one_snaps() {
    let (mut physics, mut ticker) = walked_physics(4);
    let target = [1.0, 2.620_01, 1.0];
    let outcome = crate::movement::reconcile_move_player_teleport(
        &mut ticker,
        &mut physics,
        target,
        102,
        true,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(matches!(
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 102,
            replayed_ticks: 2,
        }
    ));
    assert_eq!(physics.history_len(), 4, "the rewind keeps history");

    let far = [40.0, 2.620_01, 1.0];
    let outcome = crate::movement::reconcile_move_player_teleport(
        &mut ticker,
        &mut physics,
        far,
        103,
        true,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(matches!(outcome, PhysicsCorrectionOutcome::Snapped { .. }));
    assert_eq!(physics.history_len(), 0);
}

#[test]
fn an_item_use_modifier_slows_the_simulated_walk() {
    let walk = physics_movement_input([0.0, 1.0], 180.0, true, false, false, false, None);
    let drawing = physics_movement_input([0.0, 1.0], 180.0, true, false, false, false, Some(0.35));
    let (mut plain, _) = walked_physics(0);
    let (mut slowed, _) = walked_physics(0);
    for _ in 0..5 {
        run_tick_with(&mut plain, walk);
        run_tick_with(&mut slowed, drawing);
    }
    let travel = |physics: &LocalPhysicsController| physics.state().unwrap().position.z.abs();
    assert!(
        travel(&slowed) < travel(&plain) * 0.5,
        "{} vs {}",
        travel(&slowed),
        travel(&plain)
    );
}

/// An oversized finite motion is skipped instead of pushing the next sweep past the query extent.
#[test]
fn an_unsimulable_server_motion_is_skipped_and_prediction_keeps_running() {
    let (mut physics, _) = walked_physics(1);
    let mut unguarded = physics.clone();
    unguarded.replace_live_velocity([1.0e6, 0.0, 0.0]);
    let failed = unguarded.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    assert!(
        failed.blocked.is_some(),
        "the raw value would stop prediction"
    );
    let before = physics.state().unwrap().velocity;
    assert_eq!(physics.queue_server_motion([1.0e6, 0.0, 0.0], 0), None);
    assert_eq!(physics.state().unwrap().velocity, before);
    let frame = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
}

fn vertical_input(vertical_physics: sim::VerticalPhysics) -> MovementInput {
    MovementInput {
        vertical_physics,
        ..forward_physics_input()
    }
}

/// A delayed `HasGravity` clear stops gravity from its tick through replay.
#[test]
fn delayed_gravity_clear_rewinds_to_its_tick_and_matches_on_time_delivery() {
    let weightless = sim::VerticalPhysics {
        has_gravity: false,
        ..sim::VerticalPhysics::default()
    };
    let (mut on_time, _) = walked_physics(2);
    run_tick_with(&mut on_time, vertical_input(weightless));
    run_tick_with(&mut on_time, vertical_input(weightless));

    let (mut delayed, mut ticker) = walked_physics(4);
    assert_ne!(delayed.state(), on_time.state());
    let clear = flags(|flags| flags.has_gravity = Some(false));
    assert_eq!(delayed.apply_server_movement_flags(102, clear), Some(102));
    reconcile_timeline_rewind(&mut ticker, &mut delayed, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(delayed.state(), on_time.state());
    assert_eq!(delayed.state().unwrap().velocity.y, 0.0);
    let echo = flags(|flags| flags.has_gravity = Some(false));
    assert_eq!(delayed.apply_server_movement_flags(102, echo), None);
}

/// A delayed air-drag modifier applies from its tick through replay, not from arrival.
#[test]
fn delayed_air_drag_modifier_rewinds_to_its_tick_and_matches_on_time_delivery() {
    let doubled = sim::VerticalPhysics {
        air_drag_modifier: Some(2.0),
        ..sim::VerticalPhysics::default()
    };
    let (mut on_time, _) = walked_physics(2);
    run_tick_with(&mut on_time, vertical_input(doubled));
    run_tick_with(&mut on_time, vertical_input(doubled));

    let (mut delayed, mut ticker) = walked_physics(4);
    assert_ne!(delayed.state(), on_time.state());
    assert_eq!(delayed.retime_air_drag_modifier(102, 2.0), Some(102));
    reconcile_timeline_rewind(&mut ticker, &mut delayed, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(delayed.state(), on_time.state());
    assert_eq!(
        delayed.retime_air_drag_modifier(102, 2.0),
        None,
        "a repeated value changes nothing and needs no replay"
    );
    assert_eq!(delayed.retime_air_drag_modifier(0, 3.0), None);
    assert_eq!(delayed.retime_air_drag_modifier(150, 3.0), None);
}

/// A later update with the same stamp replaces the earlier one in replay and sync.
#[test]
fn same_tick_server_updates_replay_the_latest_value() {
    let tripled = sim::VerticalPhysics {
        air_drag_modifier: Some(3.0),
        ..sim::VerticalPhysics::default()
    };
    let (mut on_time, _) = walked_physics(2);
    run_tick_with(&mut on_time, vertical_input(tripled));
    run_tick_with(&mut on_time, vertical_input(tripled));

    let (mut delayed, mut ticker) = walked_physics(4);
    assert_eq!(delayed.retime_air_drag_modifier(102, 2.0), Some(102));
    assert_eq!(delayed.retime_air_drag_modifier(102, 3.0), Some(102));
    reconcile_timeline_rewind(&mut ticker, &mut delayed, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(delayed.state(), on_time.state());

    let (untouched, _) = walked_physics(4);
    let (mut restored, mut ticker) = walked_physics(4);
    let clear = flags(|flags| flags.has_gravity = Some(false));
    let restore = flags(|flags| flags.has_gravity = Some(true));
    assert_eq!(restored.apply_server_movement_flags(102, clear), Some(102));
    assert_eq!(
        restored.apply_server_movement_flags(102, restore),
        Some(102)
    );
    reconcile_timeline_rewind(&mut ticker, &mut restored, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(restored.state(), untouched.state());
}

/// Glides from a jump press at height, optionally boosted live from `boost_from` on.
fn glide_with_boost(
    ticks: u64,
    boost_from: Option<(u64, super::BoostSpan)>,
) -> (
    LocalPhysicsController,
    MovementTicker,
    super::LocalMovementEffectTimeline,
) {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 50.620_01, 0.0], 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 50.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    let mut effects = super::LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    let context = PhysicsSampleContext {
        pitch: 20.0,
        mode_intent: super::ModeIntent {
            elytra_ready: true,
            ..super::ModeIntent::default()
        },
        ..PhysicsSampleContext::default()
    };
    for index in 0..ticks {
        if let Some((tick, span)) = boost_from
            && physics.state().unwrap().tick == tick
        {
            effects.set_movement_boost(1, 1, super::MovementBoost::Glide, Some(span));
        }
        let input = MovementInput {
            jumping: index == 1,
            ..MovementInput::default()
        };
        let frame = physics.advance_with_context_and_effects(
            Duration::from_millis(50),
            input,
            context,
            &VersionedFloor(1),
            &mut effects,
        );
        assert_eq!(frame.completed_ticks, 1, "{:?}", frame.blocked);
        for sample in frame.samples {
            ticker.enqueue_completed_physics(sample).unwrap();
        }
    }
    (physics, ticker, effects)
}

/// A delayed firework boost rewinds to its stamp and matches on-time delivery.
#[test]
fn delayed_glide_boost_rewinds_to_its_tick_and_matches_on_time_delivery() {
    use super::physics::MovementEffectSource;
    let span = super::BoostSpan::Ticks(5);
    let (on_time, _, mut on_time_effects) = glide_with_boost(6, Some((103, span)));
    let (mut delayed, mut ticker, _) = glide_with_boost(6, None);
    assert_eq!(delayed.mode(), sim::MovementMode::Gliding);
    assert_ne!(delayed.state(), on_time.state());

    let retime = delayed.retime_movement_boost(super::MovementBoost::Glide, 103, span);
    assert_eq!(retime.rewind, Some(103));
    assert_eq!(retime.remaining, Some(super::BoostSpan::Ticks(2)));
    reconcile_timeline_rewind(&mut ticker, &mut delayed, 103, &VersionedFloor(1)).unwrap();
    assert_eq!(delayed.state(), on_time.state());
    for _ in 0..2 {
        assert!(on_time_effects.snapshot().glide_boost);
        on_time_effects.commit_successful_tick();
    }
    assert!(!on_time_effects.snapshot().glide_boost);

    let live = delayed.retime_movement_boost(super::MovementBoost::Glide, 106, span);
    assert_eq!(
        (live.rewind, live.remaining),
        (None, Some(span)),
        "a current stamp boosts from the next tick"
    );
}

/// A delayed liquid speed attribute rewrites the retained inputs after its stamp once.
#[test]
fn delayed_liquid_movement_speeds_rewrite_retained_inputs_once() {
    let (mut physics, _) = walked_physics(4);
    let speeds = super::speed_authority::LiquidMovementSpeeds {
        underwater: Some(0.05),
        lava: None,
    };
    assert_eq!(
        physics.retime_liquid_movement_speeds(102, speeds),
        Some(102)
    );
    assert_eq!(physics.retime_liquid_movement_speeds(102, speeds), None);
    assert_eq!(
        physics.retime_liquid_movement_speeds(104, speeds),
        None,
        "a current stamp applies live"
    );
}

/// Reverting a retime whose replay failed restores the retained inputs, so the
/// boost is written (and later replayed) exactly once.
#[test]
fn a_reverted_glide_boost_leaves_retained_inputs_unboosted() {
    let span = super::BoostSpan::Ticks(2);
    let (mut physics, _, _) = glide_with_boost(6, None);
    let before = physics.state().cloned();
    let retime = physics.retime_movement_boost(super::MovementBoost::Glide, 103, span);
    assert_eq!(retime.rewind, Some(103));
    physics.revert_movement_boost(retime);
    assert_eq!(physics.state().cloned(), before);
    let again = physics.retime_movement_boost(super::MovementBoost::Glide, 103, span);
    assert_eq!(
        again.rewind,
        Some(103),
        "the reverted history no longer holds the boost"
    );
    let repeat = physics.retime_movement_boost(super::MovementBoost::Glide, 103, span);
    assert_eq!(repeat.rewind, None, "an applied boost is not written twice");
}
