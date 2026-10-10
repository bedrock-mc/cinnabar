//! Unthrottled movement authority records under the outbound trace switch.

use client_world::CommittedControlEvent;
use serde_json::json;

use super::{LocalPhysicsController, LocalPhysicsFrame, MovementTicker, trace};

/// Raw server attributes for correlating effective speed with modifier identities.
pub fn trace_local_attributes(
    session: u64,
    sequence: u64,
    server_tick: u64,
    attributes: &[protocol::ActorAttribute],
) {
    if !trace::movement_trace_enabled() {
        return;
    }
    let attributes = attributes
        .iter()
        .map(|attribute| {
            json!({
                "name": attribute.name.as_ref(), "current": attribute.current, "default": attribute.default,
                "min": attribute.min, "max": attribute.max,
                "modifiers": attribute.modifiers.iter().map(|modifier| json!({
                    "id": modifier.id.as_ref(), "name": modifier.name.as_ref(), "amount": modifier.amount,
                    "operation": modifier.operation, "operand": modifier.operand,
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    trace::write_trace_line(
        &json!({"schema":"rust-mcbe-speed-attributes-v1",
        "session_generation":session, "sequence":sequence, "server_tick":server_tick,
        "attributes":attributes})
        .to_string(),
    );
}

/// Fixed-tick displacement and controls, paired with real elapsed time, behind the trace switch.
pub(crate) fn trace_physics_frame(
    session: u64,
    elapsed: std::time::Duration,
    speed: Option<f64>,
    effective_speed: Option<f64>,
    frame: &LocalPhysicsFrame,
) {
    if !trace::movement_trace_enabled() {
        return;
    }
    for sample in &frame.samples {
        trace::write_trace_line(
            &json!({"schema":"rust-mcbe-local-speed-v1",
                "session_generation":session, "elapsed_seconds":elapsed.as_secs_f64(),
                "tick":sample.tick, "position":sample.position, "velocity":sample.velocity,
                "displacement":sample.movement, "move_vector":sample.move_vector,
                "prediction_walk_speed":speed, "effective_movement_speed":effective_speed,
                "sprinting":sample.processed.sprinting,
                "sneaking":sample.processed.sneaking, "jumping":sample.jumping,
                "grounded":sample.grounded_after_tick, "mode":format!("{:?}",sample.processed.mode),
            })
            .to_string(),
        );
    }
}

/// Records incoming movement state before a correction can rewrite history.
pub fn trace_server_control(
    ticker: &MovementTicker,
    physics: &LocalPhysicsController,
    control: &CommittedControlEvent,
) {
    if !trace::movement_trace_enabled() {
        return;
    }
    let mut record = match control {
        CommittedControlEvent::PlayerMovementCorrection { correction, .. } => {
            let sent = ticker.sent_history.iter().find(|sent| {
                sent.session_generation == ticker.session_generation && sent.tick == correction.tick
            });
            let retained = physics.sample_at(correction.tick);
            json!({
                "kind": "correct", "tick": correction.tick,
                "position": correction.position, "velocity": correction.delta,
                "on_ground": correction.on_ground,
                "pitch": correction.pitch, "yaw": correction.yaw,
                "sent_position": sent.map(|sent| sent.position),
                "retained_position": retained.map(|sample| sample.position),
                "retained_velocity": retained.map(|sample| sample.velocity),
                "retained_on_ground": retained.map(|sample| sample.grounded_after_tick),
            })
        }
        CommittedControlEvent::NetworkStackLatency {
            sequence,
            creation_time,
        } => json!({
            "kind": "latency", "sequence": sequence, "creation_time": creation_time,
        }),
        CommittedControlEvent::LocalActorMotion { event, .. } => json!({
            "kind": "motion", "tick": event.tick, "velocity": event.motion,
        }),
        CommittedControlEvent::LocalMovementSpeed {
            sequence,
            movement,
            tick,
            ..
        } => json!({
            "kind": "movement_speed", "sequence": sequence,
            "current": movement.map(|attribute| attribute.current), "tick": tick,
        }),
        CommittedControlEvent::MovePlayer { movement, .. } => json!({
            "kind": "move_player", "tick": movement.source_tick,
            "position": movement.position, "pitch": movement.pitch, "yaw": movement.yaw,
            "on_ground": movement.on_ground, "teleported": movement.mode.is_teleport(),
        }),
        _ => return,
    };
    record["schema"] = json!("rust-mcbe-movement-control-v1");
    record["session_generation"] = json!(ticker.session_generation);
    record["local_tick"] = json!(physics.state().map(|state| state.tick));
    record["next_input_tick"] = json!(ticker.next_tick);
    trace::write_trace_line(&record.to_string());
}
