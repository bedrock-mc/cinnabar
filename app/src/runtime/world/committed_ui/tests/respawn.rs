use super::*;

/// Commits one server event through the production world stream.
fn submit(app: &mut App, sequence: u64, event: WorldEvent) {
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(sequence, event)
        .unwrap();
}

/// Publishes authoritative health at its ordered world position.
fn health(app: &mut App, sequence: u64, health: i32) {
    submit(
        app,
        sequence,
        WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health })),
    );
}

/// Commits readiness without manufacturing a health update.
fn ready(app: &mut App, sequence: u64) {
    submit(
        app,
        sequence,
        WorldEvent::Respawn(protocol::RespawnEvent {
            state: 1,
            runtime_entity_id: 0,
            position: [0.5, 70.0, 0.5],
        }),
    );
}

#[test]
fn later_death_in_one_ui_drain_replaces_old_pending_request() {
    let (mut app, _) = fixture_app();
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .apply_death_rules(protocol::DeathRules {
            immediate_respawn: Some(true),
            ..Default::default()
        });
    health(&mut app, 1, 0);
    app.update();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .send_respawn_request(|| true);
    health(&mut app, 2, 20);
    ready(&mut app, 3);
    health(&mut app, 4, 0);
    app.update();
    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    assert!(menu.view().death_loading);
    assert!(
        menu.send_respawn_request(|| true),
        "a fresh death needs its own request"
    );
    assert!(!menu.send_respawn_request(|| panic!("one request per death")));
    app.update();
    assert!(
        !app.world_mut()
            .resource_mut::<MenuRuntime>()
            .send_respawn_request(|| panic!("unchanged death must not reopen"))
    );
    ready(&mut app, 5);
    health(&mut app, 6, 0);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
}

#[test]
fn ready_before_first_zero_health_does_not_open_a_death_route() {
    let (mut app, _) = fixture_app();
    ready(&mut app, 1);
    health(&mut app, 2, 0);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    assert_eq!(
        app.world()
            .resource::<UiRuntimeResource>()
            .local_player_alive(),
        Some(false)
    );
}

#[test]
fn readiness_closes_death_while_health_is_unavailable() {
    let (mut app, _) = fixture_app();
    health(&mut app, 1, 0);
    app.update();
    assert!(app.world().resource::<MenuRuntime>().is_visible());
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .publish_local_actor_health(None);
    ready(&mut app, 2);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
}

#[test]
fn readiness_retires_the_route_while_completion_waits_for_queue_capacity() {
    let (mut app, _) = fixture_app();
    let (network, mut packets) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(1);
    app.insert_resource(network);
    health(&mut app, 1, 0);
    app.update();
    assert!(app.world().resource::<MenuRuntime>().is_visible());
    packets.drain();
    let blocker = protocol::modal_form_cancel_response(7);
    app.world()
        .resource::<NetworkHandle<PackApplication>>()
        .send_inventory_packet(blocker.clone())
        .unwrap();
    ready(&mut app, 2);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    assert!(app.world().resource::<ClientWorld>().respawn.input_held());
    assert_eq!(packets.drain(), [blocker]);
    app.update();
    assert_eq!(packets.drain(), [protocol::respawn_ready_packet(42)]);
    assert!(!app.world().resource::<ClientWorld>().respawn.input_held());
    app.update();
    assert!(packets.drain().is_empty());
}

#[test]
fn respawn_readiness_holds_input_behind_an_older_latency_control() {
    let (mut app, _) = fixture_app();
    let (network, mut packets) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(1);
    app.insert_resource(network);
    health(&mut app, 1, 0);
    app.update();
    packets.drain();
    let blocker = protocol::modal_form_cancel_response(7);
    app.world()
        .resource::<NetworkHandle<PackApplication>>()
        .send_inventory_packet(blocker.clone())
        .unwrap();
    submit(&mut app, 2, WorldEvent::NetworkStackLatency(123));
    app.update();
    assert!(app.world().resource::<MenuRuntime>().is_visible());
    ready(&mut app, 3);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    assert!(
        app.world().resource::<ClientWorld>().respawn.input_held(),
        "committed readiness must withhold gameplay before its control can be consumed"
    );
    let mut observed = packets.drain();
    for _ in 0..3 {
        app.update();
        observed.extend(packets.drain());
    }
    assert_eq!(
        observed,
        [
            blocker,
            protocol::network_stack_latency_reply(123),
            protocol::respawn_ready_packet(42)
        ]
    );
    assert!(!app.world().resource::<ClientWorld>().respawn.input_held());
}

#[test]
fn immediate_respawn_ready_closes_death_without_waiting_for_health() {
    let (mut app, _) = fixture_app();
    let (network, mut packets) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    app.insert_resource(network);
    app.world_mut()
        .resource_mut::<UiRuntimeResource>()
        .apply_death_rules(protocol::DeathRules {
            immediate_respawn: Some(true),
            ..Default::default()
        });
    submit(
        &mut app,
        1,
        WorldEvent::Ui(UiEvent::Hud(protocol::HudEvent::Health { health: 0 })),
    );
    app.update();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        assert!(menu.view().death_loading);
        menu.send_respawn_request(|| true);
    }
    packets.drain();
    submit(
        &mut app,
        2,
        WorldEvent::Respawn(protocol::RespawnEvent {
            state: 1,
            runtime_entity_id: 0,
            position: [0.5, 70.0, 0.5],
        }),
    );
    app.update();
    assert!(
        !app.world().resource::<MenuRuntime>().is_visible(),
        "ready must retire death without a health refresh"
    );
    assert_eq!(
        app.world()
            .resource::<UiRuntimeResource>()
            .local_player_alive(),
        Some(false),
        "respawn readiness must not invent positive health"
    );
    assert_eq!(packets.drain(), [protocol::respawn_ready_packet(42)]);
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs(3));
    app.update();
    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    assert!(
        !menu.is_visible(),
        "the unchanged zero-health snapshot must not reopen death"
    );
    assert!(!menu.send_respawn_request(|| panic!("ready must cancel retries")));
    assert!(packets.drain().is_empty());
}
