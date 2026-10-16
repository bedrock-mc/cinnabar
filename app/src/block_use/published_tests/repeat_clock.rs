use super::*;

/// A strict deadline admits the next tick even when it is less than a millisecond late.
#[test]
fn held_repeats_keep_submillisecond_deadline_precision() {
    for lateness in [
        Duration::ZERO,
        Duration::from_nanos(1),
        Duration::from_micros(500),
    ] {
        let (mut world, mut captured) = fixture();
        set_hotbar_stack(&mut world, 0, Some("minecraft:stone"));
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_micros(123));
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
        hold_use(&mut world);

        let state = world
            .resource::<MovementTicker>()
            .build_action_state()
            .unwrap();
        let mut sample = gameplay::test_support::survival_mining::completed(state.tick + 1);
        sample.position = state.position;
        world
            .resource_mut::<MovementTicker>()
            .enqueue_completed_physics(sample)
            .unwrap();
        let mut physics = client_interaction::movement::LocalPhysicsController::default();
        physics.reanchor_network_position(state.position, state.tick + 1, true);
        world.insert_resource(physics);
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(300) + lateness);
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(
            transaction_targets(&mut captured).len(),
            usize::from(!lateness.is_zero()),
            "repeat at {lateness:?} past the deadline"
        );
    }
}
