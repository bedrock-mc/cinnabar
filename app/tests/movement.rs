#[path = "../src/movement.rs"]
mod movement;

use std::time::Duration;

use movement::{
    MovementInputSample, MovementSendError, MovementTicker, OUTBOX_CAPACITY,
    flush_player_auth_inputs,
};
use protocol::PlayerInputFlags;

fn sample(position: [f32; 3]) -> MovementInputSample {
    MovementInputSample {
        position,
        move_vector: [0.0, 1.0],
        pitch: 10.0,
        yaw: 20.0,
        head_yaw: 20.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        sprinting: false,
    }
}

#[test]
fn deterministic_accumulator_emits_exactly_twenty_ticks_per_second() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);

    for frame in 0..60 {
        ticker.advance(
            Duration::from_secs_f64(1.0 / 60.0),
            sample([1.0, 64.0, 2.0]),
        );
        if frame < 2 {
            assert_eq!(ticker.pending_count(), 0);
        }
    }

    let snapshots: Vec<_> = std::iter::from_fn(|| ticker.pop_pending()).collect();
    assert_eq!(snapshots.len(), 20);
    assert_eq!(snapshots.first().unwrap().tick, 1_001);
    assert_eq!(snapshots.last().unwrap().tick, 1_020);
}

#[test]
fn tick_snapshots_encode_held_and_edge_flags_and_position_delta() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 41, [1.0, 64.0, 2.0]);
    let mut pressed = sample([1.25, 64.0, 1.5]);
    pressed.jumping = true;
    pressed.sprinting = true;

    ticker.advance(Duration::from_millis(50), pressed);
    let first = ticker.pop_pending().unwrap();
    assert_eq!(first.tick, 42);
    assert_eq!(first.delta, [0.25, 0.0, -0.5]);
    assert_eq!(first.move_vector, [0.0, 1.0]);
    assert_eq!(first.position, pressed.position);
    assert_ne!(first.flags.bits() & PlayerInputFlags::UP.bits(), 0);
    assert_ne!(first.flags.bits() & PlayerInputFlags::JUMPING.bits(), 0);
    assert_ne!(
        first.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
        0
    );
    assert_ne!(
        first.flags.bits() & PlayerInputFlags::JUMP_PRESSED_RAW.bits(),
        0
    );
    assert_ne!(first.flags.bits() & PlayerInputFlags::SPRINTING.bits(), 0);
    assert_ne!(
        first.flags.bits() & PlayerInputFlags::START_SPRINTING.bits(),
        0
    );

    ticker.advance(Duration::from_millis(50), pressed);
    let held = ticker.pop_pending().unwrap();
    assert_eq!(held.tick, 43);
    assert_eq!(held.delta, [0.0; 3]);
    assert_eq!(
        held.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
        0
    );
    assert_eq!(
        held.flags.bits() & PlayerInputFlags::START_SPRINTING.bits(),
        0
    );
    assert_ne!(
        held.flags.bits() & PlayerInputFlags::JUMP_CURRENT_RAW.bits(),
        0
    );

    let released = sample(pressed.position);
    ticker.advance(Duration::from_millis(50), released);
    let released = ticker.pop_pending().unwrap();
    assert_ne!(
        released.flags.bits() & PlayerInputFlags::JUMP_RELEASED_RAW.bits(),
        0
    );
    assert_ne!(
        released.flags.bits() & PlayerInputFlags::STOP_SPRINTING.bits(),
        0
    );
}

#[test]
fn overdue_ticks_distribute_the_frame_position_delta_evenly() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, [0.0, 64.0, 0.0]);

    ticker.advance(Duration::from_millis(150), sample([3.0, 64.0, -1.5]));

    let snapshots: Vec<_> = std::iter::from_fn(|| ticker.pop_pending()).collect();
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].position, [1.0, 64.0, -0.5]);
    assert_eq!(snapshots[1].position, [2.0, 64.0, -1.0]);
    assert_eq!(snapshots[2].position, [3.0, 64.0, -1.5]);
    assert_eq!(snapshots[0].delta, [1.0, 0.0, -0.5]);
    assert_eq!(snapshots[1].delta, [1.0, 0.0, -0.5]);
    assert_eq!(snapshots[2].delta, [1.0, 0.0, -0.5]);
}

#[test]
fn outbox_is_bounded_and_session_reset_discards_stale_ticks_and_input_edges() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.advance(Duration::from_secs(2), sample([2.0, 3.0, 4.0]));
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);
    assert_eq!(ticker.dropped_tick_count(), 40 - OUTBOX_CAPACITY as u64);

    let retry = ticker.pop_pending().unwrap();
    ticker.retry_front(retry).unwrap();
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);

    ticker.reset(2, 5_000, [9.0, 10.0, 11.0]);
    assert_eq!(ticker.session_generation(), 2);
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.dropped_tick_count(), 0);
    ticker.advance(Duration::from_millis(50), sample([9.0, 10.0, 11.0]));
    let new_session = ticker.pop_pending().unwrap();
    assert_eq!(new_session.tick, 5_001);
    assert_eq!(new_session.delta, [0.0; 3]);
    assert_eq!(
        new_session.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
        0
    );
    ticker.deactivate();
    ticker.advance(Duration::from_millis(50), sample([9.0, 10.0, 11.0]));
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn retry_front_rejects_over_capacity_without_losing_the_snapshot() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0; 3]);
    ticker.advance(
        Duration::from_millis(50 * OUTBOX_CAPACITY as u64),
        sample([0.0; 3]),
    );
    let snapshot = ticker.pop_pending().unwrap();
    ticker.retry_front(snapshot).unwrap();
    let duplicate = *ticker.peek_pending().unwrap();
    let error = ticker.retry_front(duplicate).unwrap_err();
    assert_eq!(error, duplicate);
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);
}

#[test]
fn server_correction_reanchors_delta_and_discards_unacknowledged_prediction_ticks() {
    let mut ticker = MovementTicker::default();
    ticker.reset(3, 100, [1.0, 64.0, 1.0]);
    ticker.advance(Duration::from_millis(100), sample([2.0, 64.0, 2.0]));
    assert_eq!(ticker.pending_count(), 2);

    ticker.apply_server_correction(150, [8.0, 70.0, 9.0]);
    assert_eq!(ticker.pending_count(), 0);
    ticker.advance(Duration::from_millis(50), sample([8.0, 70.0, 9.0]));
    let corrected = ticker.pop_pending().unwrap();
    assert_eq!(corrected.tick, 151);
    assert_eq!(corrected.delta, [0.0; 3]);
}

#[test]
fn bounded_flush_restores_the_exact_front_snapshot_when_transport_is_full() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.advance(Duration::from_millis(100), sample([1.0, 2.0, 3.0]));
    let expected = *ticker.peek_pending().unwrap();

    let error = flush_player_auth_inputs(&mut ticker, 8, |_packet| Err("full")).unwrap_err();
    assert!(matches!(error, MovementSendError::Transport("full")));
    assert_eq!(ticker.pending_count(), 2);
    assert_eq!(*ticker.peek_pending().unwrap(), expected);

    let mut sent_packets = 0;
    let sent = flush_player_auth_inputs(&mut ticker, 8, |_packet| {
        sent_packets += 1;
        Ok::<_, &str>(())
    })
    .unwrap();
    assert_eq!(sent, 2);
    assert_eq!(sent_packets, 2);
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn keyboard_diagonal_is_normalized_without_losing_the_raw_vector() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0; 3]);
    let mut diagonal = sample([0.0; 3]);
    diagonal.move_vector = [1.0, 1.0];
    ticker.advance(Duration::from_millis(50), diagonal);
    let snapshot = ticker.pop_pending().unwrap();

    let component = 1.0_f32 / 2.0_f32.sqrt();
    assert!((snapshot.move_vector[0] - component).abs() < 1e-6);
    assert!((snapshot.move_vector[1] - component).abs() < 1e-6);
    assert_eq!(snapshot.raw_move_vector, [1.0, 1.0]);
    assert_eq!(snapshot.analogue_move_vector, snapshot.move_vector);
}
