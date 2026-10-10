//! Reuses the committed-authority harness to check movement/probe ordering.

use super::*;
use sim::{Aabb, CollisionQuery, CollisionWorld, MovementInput, WorldQueryError};
use std::time::Duration;
use {crate::runtime::network::NetworkHandle, gameplay::movement::MovementSource};

struct EmptyWorld;

impl CollisionWorld for EmptyWorld {
    /// Supplies empty space for the isolated impulse ordering witness.
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

/// Minimal admission context; the outbox refuses to flush queued inputs without one.
fn evidence_context() -> gameplay::movement::PhysicsTickEvidenceContext {
    gameplay::movement::PhysicsTickEvidenceContext {
        fifo_sequence: 1,
        pose_generation: 1,
        dimension: 0,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        look_delta: [0.0, 0.0],
        outbound_authorized: true,
        outbox_depth: 1,
        outbox_drops: 0,
        free_camera_packet_count: 0,
    }
}

/// Admits completed movement to the actual outbound command FIFO.
fn flush_inputs(app: &mut App) {
    let mut ticker = app.world_mut().remove_resource::<MovementTicker>().unwrap();
    let network = app.world().resource::<NetworkHandle>();
    gameplay::movement::flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |identity, packet| network.send_physics_packet(identity, packet, None),
    )
    .unwrap();
    app.insert_resource(ticker);
}

#[test]
fn old_input_precedes_probe_reply_and_new_input_follows_it() {
    let mut app = app();
    let (network, mut packets) = NetworkHandle::stub_capturing_packets();
    app.insert_resource(network);
    let anchor = [0.0, 70.0, 0.0];
    let sample = {
        let mut physics = app.world_mut().resource_mut::<LocalPhysicsController>();
        physics.reanchor_network_position(anchor, 0, false);
        physics
            .advance(
                Duration::from_millis(50),
                MovementInput::default(),
                &EmptyWorld,
            )
            .samples
            .remove(0)
    };
    let old_velocity = app
        .world()
        .resource::<LocalPhysicsController>()
        .state()
        .unwrap()
        .velocity;
    {
        let mut ticker = app.world_mut().resource_mut::<MovementTicker>();
        ticker.reset(1, 0, anchor);
        ticker.set_source(MovementSource::Physics);
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    {
        let mut world = app.world_mut().resource_mut::<ClientWorld>();
        let stream = world.stream.as_mut().unwrap();
        stream
            .submit(
                1,
                WorldEvent::ActorMotion(protocol::ActorMotionEvent {
                    actor_runtime_id: 42,
                    motion: [0.0, 0.4, 0.0],
                    tick: 0,
                }),
            )
            .unwrap();
        stream
            .submit(2, WorldEvent::NetworkStackLatency(777))
            .unwrap();
    }
    app.world_mut()
        .run_system_once(reconcile_world_stream_before_physics)
        .unwrap();
    // Knockback before the probe applies now; only the reply waits for the older input.
    let velocity = app
        .world()
        .resource::<LocalPhysicsController>()
        .state()
        .unwrap()
        .velocity;
    assert_ne!(velocity, old_velocity);
    assert_eq!(velocity.y, f64::from(0.4_f32));
    assert!(packets.drain().is_empty());
    assert!(
        !app.world()
            .resource::<MovementTicker>()
            .can_advance_physics_frame()
    );
    assert!(
        app.world()
            .resource::<MovementTicker>()
            .accepting_physics_admissions()
    );
    flush_inputs(&mut app);
    let sent = packets.drain();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        protocol::player_auth_input_trace_sample(&sent[0])
            .unwrap()
            .tick,
        1
    );

    app.world_mut()
        .run_system_once(reconcile_world_stream_before_physics)
        .unwrap();
    assert!(
        app.world()
            .resource::<MovementTicker>()
            .can_advance_physics_frame()
    );
    let echoes = packets.drain();
    let session = protocol::BedrockSession { shield_item_id: 0 };
    assert_eq!(echoes.len(), 1);
    assert_eq!(
        protocol::encode(&echoes[0], &session).unwrap(),
        protocol::encode(&protocol::network_stack_latency_reply(777), &session).unwrap()
    );
    let sample = app
        .world_mut()
        .resource_mut::<LocalPhysicsController>()
        .advance(
            Duration::from_millis(50),
            MovementInput::default(),
            &EmptyWorld,
        )
        .samples
        .remove(0);
    assert_eq!(sample.movement[1], 0.4);
    app.world_mut()
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample)
        .unwrap();
    flush_inputs(&mut app);
    let sent = packets.drain();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        protocol::player_auth_input_trace_sample(&sent[0])
            .unwrap()
            .tick,
        2
    );
}

/// A correction committed ahead of a fenced probe must not wait a frame for the probe's reply.
#[test]
fn correction_before_fenced_probe_applies_this_frame() {
    let mut app = app();
    let (network, mut packets) = NetworkHandle::stub_capturing_packets();
    app.insert_resource(network);
    let anchor = [0.0, 70.0, 0.0];
    let mut samples = {
        let mut physics = app.world_mut().resource_mut::<LocalPhysicsController>();
        physics.reanchor_network_position(anchor, 0, false);
        physics
            .advance(
                Duration::from_millis(100),
                MovementInput::default(),
                &EmptyWorld,
            )
            .samples
    };
    assert_eq!(samples.len(), 2);
    {
        let mut ticker = app.world_mut().resource_mut::<MovementTicker>();
        ticker.reset(1, 0, anchor);
        ticker.set_source(MovementSource::Physics);
        ticker.enqueue_completed_physics(samples.remove(0)).unwrap();
    }
    // The server corrects a sent tick while the next input is still unsent.
    flush_inputs(&mut app);
    assert_eq!(packets.drain().len(), 1);
    app.world_mut()
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(samples.remove(0))
        .unwrap();
    let corrected = [0.0, 75.0, 0.0];
    {
        let mut world = app.world_mut().resource_mut::<ClientWorld>();
        let stream = world.stream.as_mut().unwrap();
        stream
            .submit(
                1,
                WorldEvent::PlayerMovementCorrection(protocol::PlayerMovementCorrectionEvent {
                    position: corrected,
                    delta: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    subject: protocol::MovementCorrectionSubject::Player,
                    on_ground: false,
                    tick: 1,
                }),
            )
            .unwrap();
        stream
            .submit(2, WorldEvent::NetworkStackLatency(777))
            .unwrap();
    }
    app.world_mut()
        .run_system_once(reconcile_world_stream_before_physics)
        .unwrap();
    let position = app
        .world()
        .resource::<LocalPhysicsController>()
        .network_position()
        .unwrap();
    assert!(
        (position[1] - corrected[1]).abs() < 1.0,
        "correction from {corrected:?} not applied: {position:?}"
    );
    // A snapping correction discards stale inputs; the reply may only go out once none are queued.
    let mut sent = packets.drain();
    assert!(
        sent.is_empty() || !app.world().resource::<MovementTicker>().has_unsent_inputs(),
        "{sent:?}"
    );
    flush_inputs(&mut app);
    app.world_mut()
        .run_system_once(reconcile_world_stream_before_physics)
        .unwrap();
    sent.extend(packets.drain());
    let session = protocol::BedrockSession { shield_item_id: 0 };
    let reply = protocol::encode(&protocol::network_stack_latency_reply(777), &session).unwrap();
    let replies = sent
        .iter()
        .filter(|packet| protocol::encode(packet, &session).unwrap() == reply)
        .count();
    assert_eq!(replies, 1);
}
