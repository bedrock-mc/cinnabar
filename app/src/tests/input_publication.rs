//! Packet latency across the production preparation/publication boundary.
use std::time::{Duration, Instant};

use bevy::prelude::{App, IntoScheduleConfigs, ResMut, Resource, Update};
use sha2::{Digest, Sha256};

use {crate::app::configure_client_frame_schedule, client_state::ClientFrameSet};

#[derive(Resource)]
struct SendTrace {
    started: Instant,
    sent_after: Option<Duration>,
    delay: Duration,
    order: Vec<&'static str>,
    packet: protocol::Packet,
    sent: Vec<u8>,
}

/// Freezes a tick with a mining action, as the movement outbox does before transport.
fn packet() -> protocol::Packet {
    let mut actions = protocol::BlockActions::new();
    actions
        .push(protocol::BlockAction {
            kind: protocol::BlockActionKind::StartDestroy,
            position: [2, 64, -3],
            face: 1,
        })
        .unwrap();
    protocol::player_auth_input_with_interactions(
        protocol::PlayerAuthInputSnapshot {
            tick: 101,
            position: [0.5, 65.62, 0.5],
            delta: [0.1, 0.0, 0.0],
            move_vector: [1.0, 0.0],
            analogue_move_vector: [1.0, 0.0],
            raw_move_vector: [1.0, 0.0],
            pitch: 0.0,
            yaw: 90.0,
            head_yaw: 90.0,
            camera_orientation: [1.0, 0.0, 0.0],
            flags: protocol::PlayerInputFlags::NONE,
            input_mode: protocol::PlayerInputMode::Mouse,
        },
        &protocol::PlayerAuthInputInteractions {
            block_actions: actions,
            block_interaction: None,
        },
    )
    .unwrap()
}

/// Records the same immutable bytes at the movement transport boundary.
fn enqueue(mut trace: ResMut<SendTrace>) {
    trace.sent_after = Some(trace.started.elapsed());
    trace.order.push("send");
    trace.sent = protocol::encode(
        &trace.packet,
        &protocol::BedrockSession { shield_item_id: 0 },
    )
    .unwrap()
    .to_vec();
}

/// Models expensive pure visual work without altering the queued input.
fn publish_actor(mut trace: ResMut<SendTrace>) {
    trace.order.push("publish");
    std::thread::sleep(trace.delay);
}

/// Models independent UI publication after the actor pass.
fn publish_ui(mut trace: ResMut<SendTrace>) {
    trace.order.push("publish");
    std::thread::sleep(trace.delay);
}

/// Runs the old ordering or the production ordering against the same frozen tick.
fn measure(legacy: bool, delay: Duration) -> (Duration, Vec<u8>, Vec<&'static str>) {
    let mut app = App::new();
    if legacy {
        app.configure_sets(
            Update,
            (
                ClientFrameSet::ActorPublication,
                ClientFrameSet::UiPublication,
                ClientFrameSet::NetworkSend,
            )
                .chain(),
        );
    } else {
        configure_client_frame_schedule(&mut app);
    }
    app.add_systems(Update, enqueue.in_set(ClientFrameSet::NetworkSend));
    app.add_systems(
        Update,
        publish_actor.in_set(ClientFrameSet::ActorPublication),
    );
    app.add_systems(Update, publish_ui.in_set(ClientFrameSet::UiPublication));
    app.insert_resource(SendTrace {
        started: Instant::now(),
        sent_after: None,
        delay,
        order: Vec::new(),
        packet: packet(),
        sent: Vec::new(),
    });
    app.update();
    let trace = app.world_mut().remove_resource::<SendTrace>().unwrap();
    (trace.sent_after.unwrap(), trace.sent, trace.order)
}

#[test]
fn immutable_input_is_sent_before_pure_visual_publication() {
    let (_, old_packet, old_order) = measure(true, Duration::ZERO);
    let (_, packet, order) = measure(false, Duration::ZERO);
    assert_eq!(old_order, ["publish", "publish", "send"]);
    assert_eq!(order, ["send", "publish", "publish"]);
    assert_eq!(packet, old_packet);
}

#[test]
#[ignore = "reports a controlled publication stall's contribution to input enqueue latency"]
fn input_publication_latency_bench() {
    let delay = Duration::from_millis(10);
    let (before, old_packet, _) = measure(true, delay);
    let (after, packet, order) = measure(false, delay);
    assert_eq!(packet, old_packet);
    assert_eq!(order[0], "send");
    eprintln!(
        "INPUT_PUBLICATION_BENCH injected_visual_ms={} before_send_ms={:.3} after_send_ms={:.3} bytes={} sha256={:x}",
        delay.as_millis() * 2,
        before.as_secs_f64() * 1_000.0,
        after.as_secs_f64() * 1_000.0,
        packet.len(),
        Sha256::digest(&packet),
    );
}
