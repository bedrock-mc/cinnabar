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
    for (ride, trigger) in [
        (super::RideKind::Boat, protocol::ClientInputLock::Jump),
        (super::RideKind::Other, protocol::ClientInputLock::Sneak),
    ] {
        for lock in [
            trigger,
            protocol::ClientInputLock::Dismount,
            protocol::ClientInputLock::Movement,
        ] {
            let mut locals = super::LocomotionState::default();
            let mut frame = speed_frame(Default::default());
            frame.facts.ride = Some(ride);
            frame.facts.mount_runtime_id = Some(7);
            frame.facts.input_locks = protocol::ClientInputLocks::from_bits(lock as u32);
            frame.jump.pressed = true;
            frame.sneak.pressed = true;
            locals.observe_dismount(&frame, 1);
            let mut sends = 0;
            locals.flush_dismount([0.0; 3], |_| {
                sends += 1;
                Ok(())
            });
            assert_eq!(sends, 0);
            frame.facts.input_locks = Default::default();
            locals.observe_dismount(&frame, 1);
            locals.flush_dismount([0.0; 3], |_| {
                sends += 1;
                Ok(())
            });
            assert_eq!(sends, 1);
        }
    }
}
