//! Distinct ready messages retain a response before later server controls.
use super::*;

/// Exercises repeated positions, a later latency fence and a temporarily unavailable transport.
#[test]
fn respawn_ready_responses_follow_distinct_controls_before_latency() {
    for unavailable in [false, true] {
        let mut app = wiring_app(authorized_ticker(false), LocalPhysicsController::default());
        let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
        let mut network = Some(network);
        if !unavailable {
            app.insert_resource(network.take().unwrap());
        }
        app.add_systems(
            Update,
            advance_dimension_transfer
                .after(reconcile_world_stream_before_physics)
                .run_if(bevy::prelude::resource_exists::<NetworkHandle<PackApplication>>),
        );
        for sequence in [1, 2] {
            submit(
                &mut app,
                sequence,
                WorldEvent::Respawn(RespawnEvent {
                    position: [8.5, 71.620_01, -4.25],
                    state: 1,
                    runtime_entity_id: 0,
                }),
            );
        }
        submit(&mut app, 3, WorldEvent::NetworkStackLatency(123));
        app.update();
        if unavailable {
            assert!(captured.drain().is_empty());
            assert!(app.world().resource::<ClientWorld>().respawn.input_held());
            app.insert_resource(network.take().unwrap());
            app.update();
        }
        assert_eq!(
            captured.drain(),
            [
                protocol::respawn_ready_packet(1),
                protocol::respawn_ready_packet(1),
                protocol::network_stack_latency_reply(123),
            ],
            "distinct ready messages must complete before the later latency control",
        );
        assert!(!app.world().resource::<ClientWorld>().respawn.input_held());
    }
}

#[test]
fn ready_before_initial_zero_health_keeps_input_available_until_a_new_death() {
    for attributes in [false, true] {
        let mut app = wiring_app(authorized_ticker(false), LocalPhysicsController::default());
        let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
        app.insert_resource(network);
        submit(
            &mut app,
            1,
            WorldEvent::Respawn(RespawnEvent {
                position: [0.0, 70.0, 0.0],
                state: 1,
                runtime_entity_id: 0,
            }),
        );
        let health = |current| {
            if attributes {
                WorldEvent::Actor(protocol::ActorEvent::Attributes(
                    protocol::ActorAttributesUpdateEvent {
                        runtime_id: 1,
                        dimension: 0,
                        tick: 0,
                        attributes: std::sync::Arc::from([protocol::ActorAttribute {
                            name: protocol_ids::Attribute::Health.wire_name().into(),
                            min: 0.0,
                            max: 20.0,
                            current: current as f32,
                            default: None,
                            modifiers: Default::default(),
                        }]),
                    },
                ))
            } else {
                WorldEvent::Ui(protocol::UiEvent::Hud(protocol::HudEvent::Health {
                    health: current,
                }))
            }
        };
        submit(&mut app, 2, health(0));
        app.update();
        assert_eq!(captured.drain(), [protocol::respawn_ready_packet(1)]);
        assert!(!app.world().resource::<ClientWorld>().respawn.input_held());
        submit(&mut app, 3, health(20));
        submit(&mut app, 4, health(0));
        app.update();
        assert!(app.world().resource::<ClientWorld>().respawn.input_held());
    }
}
