use super::*;
use crate::runtime::network::NetworkHandle;
use crate::runtime::world::advance_dimension_transfer;
use bevy::prelude::IntoScheduleConfigs;

#[test]
fn respawn_search_dimension_and_ready_preserve_the_clock_and_complete_before_input() {
    let initial = [0.0, 70.0, 0.0];
    let destination = [8.5, 71.620_01, -4.25];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(initial, 100, false);
    let mut app = wiring_app(authorized_ticker(false), physics);
    let (network, _guard) = NetworkHandle::with_command_capacity(3);
    app.insert_resource(network)
        .insert_resource(crate::camera::AutoFly::new(false))
        .init_resource::<crate::semantic_controls::SemanticInputSnapshot>()
        .add_systems(
            Update,
            (
                advance_dimension_transfer,
                super::super::advance_local_physics,
            )
                .chain()
                .after(reconcile_world_stream_before_physics),
        );
    submit(
        &mut app,
        1,
        WorldEvent::Respawn(RespawnEvent {
            position: [300.5, 32_767.0, 400.5],
            state: 0,
            runtime_entity_id: 0,
        }),
    );
    app.world_mut()
        .resource_mut::<bevy::prelude::Time<bevy::time::Real>>()
        .advance_by(std::time::Duration::from_millis(100));
    app.update();
    assert_eq!(
        app.world()
            .resource::<LocalPhysicsController>()
            .network_position(),
        Some(initial)
    );
    let world = app.world().resource::<ClientWorld>();
    assert_eq!(
        world
            .stream
            .as_ref()
            .unwrap()
            .resolved_server_position()
            .position,
        initial
    );
    assert!(world.respawn.input_held());
    let ticker = app.world().resource::<MovementTicker>();
    assert_eq!(ticker.completed_tick(), 102);
    assert!(
        !ticker.has_unsent_inputs(),
        "search ticks must not become PAI"
    );
    assert_eq!(
        app.world()
            .resource::<NetworkHandle>()
            .pending_command_count(),
        0
    );
    submit(
        &mut app,
        2,
        WorldEvent::ChangeDimension(ChangeDimensionEvent {
            dimension: 1,
            position: destination,
            respawn: true,
            loading_screen_id: None,
        }),
    );
    app.update();
    assert!(
        app.world()
            .resource::<ClientWorld>()
            .dimension_transfer
            .active()
    );
    assert!(app.world().resource::<ClientWorld>().respawn.input_held());
    assert_eq!(
        app.world().resource::<MovementTicker>().completed_tick(),
        102
    );
    assert!(!app.world().resource::<MovementTicker>().has_unsent_inputs());
    assert_eq!(
        app.world()
            .resource::<NetworkHandle>()
            .pending_command_count(),
        2
    );
    submit(
        &mut app,
        3,
        WorldEvent::Respawn(RespawnEvent {
            position: destination,
            state: 1,
            runtime_entity_id: 0,
        }),
    );
    app.update();
    assert_eq!(
        app.world()
            .resource::<LocalPhysicsController>()
            .network_position(),
        Some(destination)
    );
    assert!(!app.world().resource::<ClientWorld>().respawn.input_held());
    assert_eq!(
        app.world()
            .resource::<NetworkHandle>()
            .pending_command_count(),
        3
    );
    // The existing transfer hold still permits zero-motion input after native Action7.
    app.world_mut()
        .resource_mut::<bevy::prelude::Time<bevy::time::Real>>()
        .advance_by(std::time::Duration::from_millis(50));
    app.update();
    assert!(app.world().resource::<MovementTicker>().completed_tick() > 102);
    let mut ticker = app.world_mut().remove_resource::<MovementTicker>().unwrap();
    let packets = flush_capturing(&mut ticker);
    assert!(
        !packets.is_empty(),
        "ready phase resumes input on the global clock"
    );
    for packet in packets {
        assert!(player_auth_input_trace_sample(&packet).unwrap().tick > 102);
    }
}
