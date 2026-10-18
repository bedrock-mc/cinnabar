use super::*;

#[test]
fn delivered_mining_carriers_no_longer_block_fresh_input() {
    let mut movement = crate::test_support::survival_mining::ticker_with_ticks(1);
    let mut runtime = MeleeRuntime::default();
    runtime.observe_input(true, false);
    runtime.queue.entries.front_mut().unwrap().work = queue::Work::Carrying { tick: 101 };
    assert!(runtime.has_queued_work(&movement));
    assert!(
        crate::movement::flush_player_auth_inputs(
            &mut movement,
            1,
            Some(crate::test_support::survival_mining::evidence()),
            |_, _| Err(()),
        )
        .is_err()
    );
    assert!(runtime.has_queued_work(&movement));
    crate::movement::flush_player_auth_inputs(
        &mut movement,
        1,
        Some(crate::test_support::survival_mining::evidence()),
        |_, _| Ok::<_, ()>(()),
    )
    .unwrap();
    assert!(!runtime.has_queued_work(&movement));
    runtime.observe_input(true, false);
    assert!(
        runtime.has_queued_work(&movement),
        "a new press still owns admission"
    );
}

#[test]
fn block_press_at_fifo_front_survives_a_maximum_catchup_frame() {
    let count = crate::movement::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME;
    let movement = crate::test_support::survival_mining::ticker_with_ticks(count as u64);
    let expected = movement.first_unsent_sample_in_frame(count).unwrap();
    assert!(movement.interaction_before_tick(expected.tick).is_none());
    let mut runtime = MeleeRuntime::default();
    runtime.synchronize(movement.interaction_authority_identity());
    runtime.observe_input(true, true);
    let sample = runtime
        .press_sample(Crosshair::Block, &movement, count, 1_000)
        .expect("a block press needs the queue boundary, not an evicted preceding pose");
    assert_eq!(sample.tick, expected.tick);
    assert_eq!(sample.position, expected.position);
}

#[test]
fn block_press_waits_for_older_movement_before_its_current_frame_tick() {
    for recent_ticks in [1, 0] {
        let mut movement = crate::test_support::survival_mining::ticker_with_ticks(4);
        let mut runtime = MeleeRuntime::default();
        runtime.synchronize(movement.interaction_authority_identity());
        runtime.observe_input(true, true);
        if recent_ticks == 0 {
            runtime.queue.front_pending_mut().unwrap().rejected_tick = Some(104);
        }
        assert!(
            runtime
                .press_sample(Crosshair::Block, &movement, recent_ticks, 1_000)
                .is_none(),
            "the block swing must wait behind older input"
        );
        crate::movement::flush_player_auth_inputs(
            &mut movement,
            1,
            Some(crate::test_support::survival_mining::evidence()),
            |_, _| Ok::<_, ()>(()),
        )
        .unwrap();
        assert!(
            runtime
                .press_sample(Crosshair::Block, &movement, recent_ticks, 1_001)
                .is_none()
        );
        crate::movement::flush_player_auth_inputs(
            &mut movement,
            2,
            Some(crate::test_support::survival_mining::evidence()),
            |_, _| Ok::<_, ()>(()),
        )
        .unwrap();
        let sample = runtime
            .press_sample(Crosshair::Block, &movement, recent_ticks, 1_002)
            .unwrap();
        assert_eq!(sample.tick, 104);
        assert_eq!(
            sample.position,
            movement.newest_unsent_sample().unwrap().position
        );
    }
}

/// Supplies one verified empty hand at a fixed tick and position.
fn press(tick: u64) -> PressContext {
    let stack = protocol::NetworkItemStack::empty();
    PressContext {
        tick,
        player_position: [0.0, 1.62, 0.0],
        input_mode: PlayerInputMode::Mouse,
        local_runtime_id: 42,
        selection: Some(FrozenMiningSelection {
            slot: 0,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        swing_duration: actor_animation::ACTOR_SWING_TICKS,
        now_millis: 1_000,
        item_attack: None,
    }
}

#[test]
fn separate_attack_presses_can_swing_before_the_same_tick() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let target = Crosshair::Actor(ActorHit {
        runtime_id: 7,
        distance: 2.0,
        point: [0.0, 1.62, -2.0],
    });
    for _ in 0..2 {
        runtime.observe_input(true, false);
        let outcome = runtime.resolve(target, &press(100), &mut swings);
        assert_eq!(outcome.packets.len(), 2);
        assert_eq!(
            outcome.packets[1].kind(),
            protocol::swing_arm_packet(42, SwingSource::Attack).kind()
        );
        assert!(protocol::is_aim_assist_rotation_action(&outcome.packets[0]));
        assert_eq!(
            swings.take_started(),
            Some(actor_animation::ACTOR_SWING_TICKS)
        );
    }
    runtime.observe_input(false, true);
    assert!(
        runtime
            .resolve(target, &press(100), &mut swings)
            .packets
            .is_empty()
    );
}

#[test]
fn a_second_press_retains_its_swing_when_admission_waits_past_the_tick() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let target = Crosshair::Actor(ActorHit {
        runtime_id: 7,
        distance: 2.0,
        point: [0.0, 1.62, -2.0],
    });
    runtime.observe_input(true, false);
    resolve_and_send(
        &mut runtime,
        &mut swings,
        target,
        &press(100),
        1_000,
        |_| Ok(()),
    );
    swings.take_started();
    runtime.observe_input(true, false);
    resolve_and_send(
        &mut runtime,
        &mut swings,
        target,
        &press(100),
        1_001,
        |_| Err(BatchSendError::Full),
    );
    swings.published_progress(100);
    let mut packets = Vec::new();
    resolve_and_send(
        &mut runtime,
        &mut swings,
        target,
        &press(100),
        1_002,
        |sent| {
            packets = sent;
            Ok(())
        },
    );
    assert_eq!(packets.len(), 2);
    assert_eq!(
        packets[1].kind(),
        protocol::swing_arm_packet(42, SwingSource::Attack).kind()
    );
    assert!(protocol::is_aim_assist_rotation_action(&packets[0]));
}

#[test]
fn fresh_press_still_obeys_the_half_swing_guard_after_the_tick() {
    let mut swings = SwingTracker::default();
    let duration = actor_animation::ACTOR_SWING_TICKS;
    assert!(swings.try_attack_swing(100, duration));
    assert!(swings.try_attack_swing(100, duration));
    swings.published_progress(100);
    assert!(!swings.try_attack_swing(101, duration));
    assert!(swings.try_attack_swing(100 + (duration / 2) as u64 + 1, duration));
}

#[test]
fn refused_ordered_press_does_not_survive_publication() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let mut mining = crate::survival_mining::SurvivalMiningRuntime::default();
    let mut movement = crate::test_support::survival_mining::ticker_with_ticks(3);
    let press = press(101);
    runtime.observe_input(true, false);
    let mut attempts = 0;
    for (now, full) in [(1_000, true), (1_001, true), (1_002, false)] {
        if now > 1_000 {
            swings.published_progress(movement.completed_tick());
        }
        let outcome = admit_attack_press(
            &mut runtime,
            &mut mining,
            &mut swings,
            &mut movement,
            AttackFrame {
                attack: Some((Crosshair::Block, press.clone())),
                mining_target: None,
                allow_mining_start: true,
                attack_held: false,
                authority: crate::survival_mining::BlockBreakingAuthority::Server,
                now_millis: now,
                local_runtime_id: press.local_runtime_id,
            },
            AttackEffects {
                send: |_| {
                    attempts += 1;
                    if full {
                        Err(BatchSendError::Full)
                    } else {
                        Ok(())
                    }
                },
                swing_duration: |_| press.swing_duration,
                request_id: |_, _| None,
                predict_break: |_| panic!("a swing without a mining target cannot break terrain"),
            },
        );
        assert!(!runtime.press_pending());
        assert!(outcome.admitted.is_none());
    }
    assert_eq!(
        attempts, 1,
        "later calls must not replay the refused ordered press"
    );
    assert!(!runtime.has_queued_work(&movement));
    runtime = MeleeRuntime::default();
    runtime.observe_input(true, false);
    resolve_and_send(
        &mut runtime,
        &mut swings,
        Crosshair::Block,
        &press,
        1_003,
        |_| panic!("a fresh press cannot reuse another press's published tick"),
    );
}
