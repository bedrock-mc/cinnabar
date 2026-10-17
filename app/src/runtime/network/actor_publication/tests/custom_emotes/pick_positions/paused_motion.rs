use super::*;
use client_interaction::movement::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
    LocomotionState, MovementTicker,
};
use gameplay::test_support::survival_mining::ticker_with_ticks;

/// Checks that a completed actor step is not repeated while local simulation is paused.
fn assert_remote_actor_motion_pauses(ticker: gameplay::movement::MovementTicker) {
    let mut movement = MovementTicker::default();
    *movement = ticker;
    assert!(movement.physics_is_authorized());
    assert!(!movement.can_advance_physics_frame());
    let local_tick = movement.completed_tick();
    let mut app = App::new();
    app.add_systems(
        Update,
        capture_predicted_box.in_set(client_state::ClientFrameSet::NetworkSend),
    );
    let (mut schedule, mut world) = production(&mut app);
    world.insert_resource(movement);
    world.init_resource::<PredictedBox>();
    world.init_resource::<client_state::SemanticInputSnapshot>();
    world.init_resource::<LocalMovementSpeedAuthority>();
    world.init_resource::<LocomotionState>();
    world.insert_resource(client_presentation::camera::AutoFly::new(false));
    #[cfg(feature = "acceptance")]
    world.insert_resource(acceptance::AcceptanceRun::new(None, None, false, false));
    #[cfg(not(feature = "acceptance"))]
    world.init_resource::<crate::acceptance::AcceptanceRun>();
    let anchor = world
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .resolved_server_position()
        .position;
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(anchor, local_tick, true);
    world.insert_resource(physics);
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(3, move_actor(2, [2.0, 64.0, 9.0], 0.0))
            .unwrap();
        stream.poll([0.0, 64.0, 0.0], 0);
    }
    // Publish the previous frame's completed tick before entering paused frames.
    world
        .resource_mut::<LocalMovementEffectTimeline>()
        .commit_successful_tick();
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    schedule.run(&mut world);
    let (position, actor_tick) = {
        let client = world.resource::<ClientWorld>();
        let authority = client.stream.as_ref().unwrap().authority();
        (
            authority.actor(2).unwrap().position,
            authority.actor_rig(2).unwrap().completed_tick,
        )
    };
    assert!((position[2] - 3.0).abs() < 1e-4);
    let predicted = world.resource::<PredictedBox>().0;
    assert!(predicted.is_some());

    for millis in [16, 25, 50] {
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(millis));
        world
            .run_system_cached(crate::movement::advance_local_physics)
            .unwrap();
        schedule.run(&mut world);

        assert_eq!(world.resource::<PredictedBox>().0, predicted);
        let client = world.resource::<ClientWorld>();
        let authority = client.stream.as_ref().unwrap().authority();
        assert_eq!(authority.actor(2).unwrap().position, position);
        assert_eq!(authority.actor_rig(2).unwrap().completed_tick, actor_tick);
        assert_eq!(
            world
                .resource::<LocalPhysicsController>()
                .state()
                .unwrap()
                .tick,
            local_tick
        );
        assert_eq!(
            world.resource::<MovementTicker>().completed_tick(),
            local_tick
        );
        assert_eq!(
            world
                .resource::<LocalMovementEffectTimeline>()
                .recent_tick_count(),
            0
        );
    }
}

#[test]
fn remote_actor_motion_pauses_for_control_fence() {
    let mut movement = ticker_with_ticks(1);
    movement.set_control_fence_pending(true);
    assert_remote_actor_motion_pauses(movement);
}

#[test]
fn remote_actor_motion_pauses_for_movement_backpressure() {
    let movement = ticker_with_ticks(gameplay::movement::OUTBOX_CAPACITY as u64);
    assert_remote_actor_motion_pauses(movement);
}
