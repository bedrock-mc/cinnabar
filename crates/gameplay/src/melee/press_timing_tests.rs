use super::*;

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
            outcome.packets[0].kind(),
            protocol::swing_arm_packet(42, SwingSource::Attack).kind()
        );
        assert!(protocol::is_aim_assist_rotation_action(&outcome.packets[1]));
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
        packets[0].kind(),
        protocol::swing_arm_packet(42, SwingSource::Attack).kind()
    );
    assert!(protocol::is_aim_assist_rotation_action(&packets[1]));
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
