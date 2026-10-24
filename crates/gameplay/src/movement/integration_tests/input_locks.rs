#[test]
fn server_input_locks_filter_processed_input_and_keep_raw_device_vectors() {
    let mut physics = LocalPhysicsController::default();
    let position = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    physics.reanchor_network_position(position, 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, position);
    ticker.set_source(MovementSource::Physics);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    for (index, locked) in [true, false].into_iter().enumerate() {
        let mut frame = speed_frame(Default::default());
        frame.now += Duration::from_millis(index as u64 * 50);
        frame.facts.input_locks = protocol::ClientInputLocks::from_bits(if locked {
            protocol::ClientInputLock::Movement as u32
        } else {
            0
        });
        frame.raw_movement = [0.25, 1.0];
        frame.analogue_movement = frame.raw_movement;
        frame.jump.held = true;
        frame.sneak.held = true;
        frame.movement_buttons.forward = true;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1)
        ));
        let sample = physics.sample_at(101 + index as u64).unwrap();
        assert_eq!(sample.raw_move_vector, [0.25, 1.0]);
        assert_eq!(sample.analogue_move_vector, [0.25, 1.0]);
        assert_eq!(sample.jumping, !locked);
        assert_eq!(sample.sneaking, !locked);
        assert_eq!(sample.move_vector[1] > 0.0, !locked);
        assert_eq!(sample.input.movement_buttons.forward, !locked);
    }
}

#[test]
fn server_input_locks_block_dismount_and_its_trigger_until_unlock() {
    use protocol::ClientInputLock::{Dismount, Jump, Movement, Sneak};
    // Both buttons are pressed together; a boat dismounts through whichever one is allowed.
    for (ride, locks, locked_sends) in [
        (super::RideKind::Boat, vec![Jump], 1),
        (super::RideKind::Boat, vec![Sneak], 1),
        (super::RideKind::Boat, vec![Jump, Sneak], 0),
        (super::RideKind::Boat, vec![Dismount], 0),
        (super::RideKind::Boat, vec![Movement], 0),
        (super::RideKind::Other, vec![Sneak], 0),
        (super::RideKind::Other, vec![Dismount], 0),
        (super::RideKind::Other, vec![Movement], 0),
    ] {
        let mut locals = super::LocomotionState::default();
        let mut frame = speed_frame(Default::default());
        frame.facts.ride = Some(ride);
        frame.facts.mount_runtime_id = Some(7);
        frame.facts.input_locks = protocol::ClientInputLocks::from_bits(
            locks.iter().fold(0, |bits, lock| bits | *lock as u32),
        );
        frame.jump.pressed = true;
        frame.sneak.pressed = true;
        locals.observe_dismount(&frame, 1);
        let mut sends = 0;
        locals.flush_dismount([0.0; 3], |_| {
            sends += 1;
            Ok(())
        });
        assert_eq!(sends, locked_sends, "{ride:?} {locks:?}");
        if locked_sends == 1 {
            continue;
        }
        frame.facts.input_locks = Default::default();
        locals.observe_dismount(&frame, 1);
        locals.flush_dismount([0.0; 3], |_| {
            sends += 1;
            Ok(())
        });
        assert_eq!(sends, 1, "{ride:?} {locks:?} after unlock");
    }
}
