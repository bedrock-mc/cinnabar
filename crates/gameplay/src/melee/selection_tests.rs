use super::*;
use crate::test_support::survival_mining::ticker_with_ticks;

/// Attempts a block press against the production sample selector and a full queue.
fn reject_block_press(
    runtime: &mut MeleeRuntime,
    swings: &mut SwingTracker,
    movement: &crate::movement::MovementTicker,
    recent_ticks: usize,
) -> PressContext {
    runtime.synchronize(movement.interaction_authority_identity());
    swings.sync_ticks(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &crate::movement::LocalMovementEffectTimeline::default(),
    );
    runtime.observe_input(true, true);
    let sample = runtime
        .press_sample(Crosshair::Block, movement, recent_ticks, 1)
        .unwrap();
    let press = PressContext {
        tick: sample.tick,
        player_position: sample.position,
        input_mode: PlayerInputMode::Mouse,
        local_runtime_id: 1,
        selection: None,
        swing_duration: client_world::ACTOR_SWING_TICKS,
        now_millis: 1,
    };
    resolve_and_send(runtime, swings, Crosshair::Block, &press, 1, |packets| {
        assert_eq!(packets.len(), 1);
        Err(BatchSendError::Full)
    });
    press
}

#[test]
fn a_rejected_block_press_recovers_without_another_physics_tick() {
    let movement = ticker_with_ticks(1);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let press = reject_block_press(&mut runtime, &mut swings, &movement, 1);
    swings.published_progress(movement.completed_tick());
    let sample = runtime
        .press_sample(Crosshair::Block, &movement, 0, 2)
        .expect("an owned rejected swing keeps its retained unsent tick");
    assert_eq!(sample.tick, press.tick);
    assert_eq!(sample.position, press.player_position);
    let mut packets_sent = 0;
    resolve_and_send(
        &mut runtime,
        &mut swings,
        Crosshair::Block,
        &press,
        2,
        |packets| {
            assert_eq!(packets.len(), 1);
            packets_sent += packets.len();
            Ok(())
        },
    );
    assert_eq!(packets_sent, 1);
    assert!(!runtime.observe_input(false, false));
    assert_eq!(swings.take_started(), Some(press.swing_duration));
    assert_eq!(swings.published_progress(press.tick).java, [0.0; 2]);
    assert_eq!(
        swings.published_progress(press.tick + 1).java,
        [0.0, 1.0 / press.swing_duration as f32]
    );
}

#[test]
fn block_retry_selection_preserves_current_frame_priority_and_exact_tick() {
    let movement = ticker_with_ticks(3);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let press = reject_block_press(&mut runtime, &mut swings, &movement, 3);
    assert_eq!(press.tick, 101);
    assert_eq!(
        runtime
            .press_sample(Crosshair::Block, &movement, 0, 2)
            .unwrap()
            .tick,
        press.tick
    );
    assert_eq!(
        runtime
            .press_sample(Crosshair::Block, &movement, 1, 2)
            .unwrap()
            .tick,
        103
    );
    assert_eq!(
        runtime
            .press_sample(Crosshair::Miss, &movement, 0, 2)
            .unwrap()
            .tick,
        103
    );
}

#[test]
fn fresh_block_selection_cannot_borrow_another_owners_retry() {
    let movement = ticker_with_ticks(1);
    let mut runtime = MeleeRuntime::default();
    runtime.synchronize(movement.interaction_authority_identity());
    runtime.observe_input(true, true);
    let mut swings = SwingTracker::default();
    let mut candidate = swings.clone();
    assert!(candidate.try_swing(101, client_world::ACTOR_SWING_TICKS));
    swings.defer_unadmitted_attempt(&candidate);
    swings.published_progress(101);
    assert!(
        runtime
            .press_sample(Crosshair::Block, &movement, 0, 2)
            .is_none()
    );
}

#[test]
fn a_block_retry_revokes_selection_after_cancel_authority_change_or_expiry() {
    for reset in 0..4 {
        let movement = ticker_with_ticks(1);
        let mut runtime = MeleeRuntime::default();
        let mut swings = SwingTracker::default();
        reject_block_press(&mut runtime, &mut swings, &movement, 1);
        match reset {
            0 => runtime.cancel(),
            1 => runtime.synchronize((7, 1)),
            2 => runtime.defer(2 + MAX_PENDING_INTERACTION_FRAMES),
            _ => runtime.position_authority = Some((7, 1)),
        }
        assert!(
            runtime
                .press_sample(Crosshair::Block, &movement, 0, 2)
                .is_none()
        );
    }
}

#[test]
fn a_block_retry_does_not_select_a_different_retained_tick_after_its_tick_drains() {
    let mut movement = ticker_with_ticks(3);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    reject_block_press(&mut runtime, &mut swings, &movement, 3);
    crate::movement::flush_player_auth_inputs(
        &mut movement,
        1,
        Some(crate::test_support::survival_mining::evidence()),
        |_, _| Ok::<_, ()>(()),
    )
    .unwrap();
    assert_eq!(movement.newest_unsent_sample().unwrap().tick, 103);
    assert!(movement.unsent_sample_at(101).is_none());
    assert!(
        runtime
            .press_sample(Crosshair::Block, &movement, 0, 2)
            .is_none()
    );
}

#[test]
fn a_fresh_block_press_expires_while_waiting_for_a_physics_tick() {
    let mut movement = ticker_with_ticks(1);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    swings.published_progress(movement.completed_tick());
    runtime.synchronize(movement.interaction_authority_identity());
    runtime.observe_input(true, false);
    for frame in [1, 2 + MAX_PENDING_INTERACTION_FRAMES] {
        assert!(
            runtime
                .press_sample(Crosshair::Block, &movement, 0, frame)
                .is_none()
        );
    }
    assert!(
        !runtime.observe_input(false, false),
        "a released press must expire during a physics stall"
    );
    movement
        .enqueue_completed_physics(crate::test_support::survival_mining::completed(102))
        .unwrap();
    let sample = runtime
        .press_sample(
            Crosshair::Block,
            &movement,
            1,
            3 + MAX_PENDING_INTERACTION_FRAMES,
        )
        .unwrap();
    let press = PressContext {
        tick: sample.tick,
        player_position: sample.position,
        input_mode: PlayerInputMode::Mouse,
        local_runtime_id: 1,
        selection: None,
        swing_duration: client_world::ACTOR_SWING_TICKS,
        now_millis: 1,
    };
    assert!(
        runtime
            .resolve(Crosshair::Block, &press, &mut swings)
            .packets
            .is_empty(),
        "resumed simulation must not fire stale input"
    );
    assert_eq!(swings.take_started(), None);
}

#[test]
fn a_quick_released_block_press_waits_through_a_short_physics_stall() {
    let movement = ticker_with_ticks(1);
    let mut runtime = MeleeRuntime::default();
    runtime.synchronize(movement.interaction_authority_identity());
    runtime.observe_input(true, false);
    for frame in [1, 1 + MAX_PENDING_INTERACTION_FRAMES] {
        assert!(
            runtime
                .press_sample(Crosshair::Block, &movement, 0, frame)
                .is_none()
        );
        assert!(
            runtime.observe_input(false, false),
            "release must retain a recent quick click"
        );
    }
}

#[test]
fn an_owned_block_retry_expires_after_repeated_full_admissions() {
    let movement = ticker_with_ticks(1);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let press = reject_block_press(&mut runtime, &mut swings, &movement, 1);
    swings.published_progress(movement.completed_tick());
    for frame in 2..=2 + MAX_PENDING_INTERACTION_FRAMES {
        let sample = runtime
            .press_sample(Crosshair::Block, &movement, 0, frame)
            .unwrap();
        assert_eq!(sample.tick, press.tick);
        resolve_and_send(
            &mut runtime,
            &mut swings,
            Crosshair::Block,
            &press,
            frame,
            |_| Err(BatchSendError::Full),
        );
    }
    assert!(!runtime.observe_input(false, false));
    assert!(
        runtime
            .press_sample(
                Crosshair::Block,
                &movement,
                0,
                3 + MAX_PENDING_INTERACTION_FRAMES
            )
            .is_none()
    );
}

#[test]
fn a_rejected_old_block_tick_waits_for_a_new_tick_after_catchup_publication() {
    let mut movement = ticker_with_ticks(3);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let mut press = reject_block_press(&mut runtime, &mut swings, &movement, 3);
    assert_eq!(press.tick, 101);
    let published = swings.published_progress(movement.completed_tick());
    let retained = runtime
        .press_sample(Crosshair::Block, &movement, 0, 2)
        .unwrap();
    assert_eq!(retained.tick, press.tick);
    let mut sends = 0;
    resolve_and_send(
        &mut runtime,
        &mut swings,
        Crosshair::Block,
        &press,
        2,
        |_| {
            sends += 1;
            Ok(())
        },
    );
    assert_eq!(sends, 0, "an old block tick cannot submit an empty retry");
    assert!(
        runtime.observe_input(false, false),
        "the un-replayable block press waits for a fresh tick"
    );
    assert_eq!(swings.published_progress(103), published);
    movement
        .enqueue_completed_physics(crate::test_support::survival_mining::completed(104))
        .unwrap();
    let sample = runtime
        .press_sample(Crosshair::Block, &movement, 1, 3)
        .unwrap();
    press.tick = sample.tick;
    press.player_position = sample.position;
    resolve_and_send(
        &mut runtime,
        &mut swings,
        Crosshair::Block,
        &press,
        3,
        |packets| {
            assert_eq!(packets.len(), 1);
            sends += packets.len();
            Ok(())
        },
    );
    assert_eq!(sends, 1);
    assert!(!runtime.observe_input(false, false));
    assert_eq!(swings.take_started(), Some(press.swing_duration));
    assert_eq!(swings.published_progress(104).java, [0.0; 2]);
    assert_eq!(
        swings.published_progress(105).java,
        [0.0, 1.0 / press.swing_duration as f32]
    );
}

#[test]
fn an_unreplayable_owned_block_tick_remains_bounded() {
    let movement = ticker_with_ticks(3);
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let press = reject_block_press(&mut runtime, &mut swings, &movement, 3);
    swings.published_progress(movement.completed_tick());
    for frame in [2, 2 + MAX_PENDING_INTERACTION_FRAMES] {
        assert_eq!(
            runtime
                .press_sample(Crosshair::Block, &movement, 0, frame)
                .unwrap()
                .tick,
            press.tick
        );
        resolve_and_send(
            &mut runtime,
            &mut swings,
            Crosshair::Block,
            &press,
            frame,
            |_| {
                panic!("an old rejected block tick must wait without submitting");
            },
        );
    }
    assert!(!runtime.observe_input(false, false));
    assert!(
        runtime
            .press_sample(
                Crosshair::Block,
                &movement,
                0,
                3 + MAX_PENDING_INTERACTION_FRAMES
            )
            .is_none()
    );
}
