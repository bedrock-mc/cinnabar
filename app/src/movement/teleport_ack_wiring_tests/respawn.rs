use crate::runtime::network::resource_packs::PackApplication;
use crate::runtime::world::advance_dimension_transfer;
use bevy::prelude::IntoScheduleConfigs;
use client_runtime::network::NetworkHandle;
use {super::*, gameplay::movement::flush_player_auth_inputs};

#[path = "respawn/ready_order.rs"]
mod ready_order;
#[path = "respawn/riptide.rs"]
mod riptide;

#[test]
fn respawn_search_dimension_and_ready_preserve_the_clock_and_complete_before_input() {
    assert_respawn_input_lifecycle(
        0,
        WorldEvent::Respawn(RespawnEvent {
            position: [300.5, 32_767.0, 400.5],
            state: 0,
            runtime_entity_id: 0,
        }),
    );
}

#[test]
fn local_death_without_respawn_search_withholds_input_across_dimensions() {
    for dimension in [0, 1, 2] {
        for death in [
            WorldEvent::Ui(protocol::UiEvent::Hud(protocol::HudEvent::Health {
                health: 0,
            })),
            WorldEvent::Actor(protocol::ActorEvent::Status(protocol::ActorStatusEvent {
                runtime_id: 1,
                kind: protocol::ActorStatusKind::Death,
                data: 0,
            })),
            WorldEvent::Actor(protocol::ActorEvent::Attributes(
                protocol::ActorAttributesUpdateEvent {
                    runtime_id: 1,
                    dimension,
                    tick: 100,
                    attributes: std::sync::Arc::from([protocol::ActorAttribute {
                        name: protocol_ids::Attribute::Health.wire_name().into(),
                        min: 0.0,
                        max: 20.0,
                        current: 0.0,
                        default: None,
                        modifiers: Default::default(),
                    }]),
                },
            )),
        ] {
            assert_respawn_input_lifecycle(dimension, death);
        }
    }
}

/// Exercises the production input FIFO from death through a cross-dimension respawn.
fn assert_respawn_input_lifecycle(dimension: i32, death: WorldEvent) {
    let life_death = !matches!(death, WorldEvent::Respawn(_));
    let initial = [0.0, 70.0, 0.0];
    let destination = [8.5, 71.620_01, -4.25];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(initial, 100, false);
    let mut app = wiring_app(authorized_ticker(false), physics);
    app.world_mut().resource_mut::<ClientWorld>().stream =
        Some(fixture_stream_in_dimension(dimension));
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    app.insert_resource(network)
        .insert_resource(client_presentation::camera::AutoFly::new(false))
        .init_resource::<client_state::SemanticInputSnapshot>()
        .add_systems(
            Update,
            (
                advance_dimension_transfer,
                super::super::advance_local_physics,
            )
                .chain()
                .after(reconcile_world_stream_before_physics),
        );
    submit(&mut app, 1, death);
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
    let actor = world.stream.as_ref().unwrap().local_player_runtime_id();
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
            .resource::<NetworkHandle<PackApplication>>()
            .pending_command_count(),
        0
    );
    submit(
        &mut app,
        2,
        WorldEvent::ChangeDimension(ChangeDimensionEvent {
            dimension: if dimension == 0 { 1 } else { 0 },
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
            .resource::<NetworkHandle<PackApplication>>()
            .pending_command_count(),
        1
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
            .resource::<NetworkHandle<PackApplication>>()
            .pending_command_count(),
        2
    );
    if life_death {
        // Health can lag readiness; a repeated zero must not start another input hold.
        submit(
            &mut app,
            4,
            WorldEvent::Ui(protocol::UiEvent::Hud(protocol::HudEvent::Health {
                health: 0,
            })),
        );
        app.update();
        assert!(!app.world().resource::<ClientWorld>().respawn.input_held());
    }
    // The existing transfer hold still permits zero-motion input after native Action7.
    app.world_mut()
        .resource_mut::<bevy::prelude::Time<bevy::time::Real>>()
        .advance_by(std::time::Duration::from_millis(50));
    app.update();
    assert!(app.world().resource::<MovementTicker>().completed_tick() > 102);
    let mut ticker = app.world_mut().remove_resource::<MovementTicker>().unwrap();
    let network = app.world().resource::<NetworkHandle<PackApplication>>();
    flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |_identity, packet| network.send_movement_packet(packet),
    )
    .unwrap();
    let packets = captured.drain();
    assert!(
        packets.len() > 2,
        "ready phase resumes input on the global clock"
    );
    assert_eq!(
        &packets[..2],
        &[
            protocol::loading_screen_packet(protocol::LoadingScreenPhase::Start, None),
            protocol::respawn_ready_packet(actor),
        ],
        "respawn completion must precede resumed PAI in the production FIFO"
    );
    for packet in &packets[2..] {
        assert!(player_auth_input_trace_sample(packet).unwrap().tick > 102);
    }
}
