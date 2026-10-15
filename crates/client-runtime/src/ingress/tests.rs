use super::*;
use client_session::{SequencedWorldEvent, WorldIngress};
use client_world::ingestion::{MAX_ADMITTED_HEAVY_EVENTS, MAX_DEFERRED_WORLD_EVENTS};
use protocol::{LevelChunkEvent, LevelChunkMode, WorldEvent};
use simulation_clock::identity::SessionGeneration;

/// Wraps an event in one session's ordered ingress.
fn event(sequence: u64, event: WorldEvent) -> WorldIngress {
    WorldIngress::Event(SequencedWorldEvent {
        session_generation: SessionGeneration::new(1),
        sequence: protocol::identity::FifoSequence::new(sequence),
        event,
    })
}

/// Creates terrain that waits for a decode without dispatching a worker.
fn terrain(x: i32) -> WorldEvent {
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x,
        z: 0,
        mode: LevelChunkMode::Inline { count: 1 },
        payload: Vec::new(),
    })
}

/// Creates a remote movement event whose lane has no terrain dependency.
fn movement() -> WorldEvent {
    WorldEvent::Actor(protocol::ActorEvent::Move(protocol::ActorMoveEvent {
        dimension: 0,
        runtime_id: 9,
        position: [Some(1.0); 3],
        position_origin: protocol::ActorPositionOrigin::NetworkOffset,
        pitch: None,
        yaw: None,
        head_yaw: None,
        on_ground: None,
        teleported: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    }))
}

#[test]
fn remote_moves_drain_when_heavy_and_deferred_terrain_are_full() {
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let terrain_count = (MAX_ADMITTED_HEAVY_EVENTS + MAX_DEFERRED_WORLD_EVENTS) as u64;
    for sequence in 1..=terrain_count {
        stream.submit(sequence, terrain(sequence as i32)).unwrap();
    }
    assert_eq!(stream.remaining_admission_capacity(), 0);
    assert!(!stream.can_admit(&terrain(1000)));

    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    for sequence in terrain_count + 1..=terrain_count + 2 {
        sender.try_send(event(sequence, movement())).unwrap();
    }
    let now = Instant::now();
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    let mut pending = None;
    let mut applied = Vec::new();
    while let Some(ingress) = drain.next(
        &mut receiver,
        &mut pending,
        |ingress| match ingress {
            WorldIngress::Event(event) => stream.can_admit(&event.event),
            _ => unreachable!("only movement events were queued"),
        },
        now,
    ) {
        let WorldIngress::Event(event) = ingress else {
            unreachable!();
        };
        applied.push(event.sequence.get());
        stream.submit(event.sequence.get(), event.event).unwrap();
    }
    assert_eq!(applied, [terrain_count + 1, terrain_count + 2]);
    assert_eq!(stream.committed_sequence(), 0);
    assert_eq!(
        stream.inventory_committed_through(),
        Some(terrain_count + 2)
    );
    assert!(pending.is_none());
    assert!(receiver.is_empty());
}

#[test]
fn blocked_head_survives_frames_without_later_events_overtaking() {
    let first = event(1, terrain(0));
    let second = event(2, movement());
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    sender.try_send(first.clone()).unwrap();
    sender.try_send(second.clone()).unwrap();
    let now = Instant::now();
    let mut pending = None;
    for frame in 0..2 {
        let start = now + WORLD_INGRESS_DRAIN_BUDGET * frame;
        let mut drain = WorldIngressDrain::new(start + WORLD_INGRESS_DRAIN_BUDGET);
        assert!(
            drain
                .next(&mut receiver, &mut pending, |_| false, start)
                .is_none()
        );
        assert_eq!(pending.as_ref(), Some(&first));
        assert_eq!(receiver.len(), 1);
    }
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    assert_eq!(
        drain.next(&mut receiver, &mut pending, |_| true, now),
        Some(first)
    );
    assert_eq!(
        drain.next(&mut receiver, &mut pending, |_| true, now),
        Some(second)
    );
}

#[test]
fn expired_frame_leaves_the_buffered_head_untouched() {
    let first = event(1, movement());
    let mut pending = Some(first.clone());
    let (_sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let now = Instant::now();
    let mut drain = WorldIngressDrain::new(now);
    assert!(
        drain
            .next(
                &mut receiver,
                &mut pending,
                |_| panic!("expired frame checked admission"),
                now
            )
            .is_none()
    );
    assert_eq!(pending, Some(first));
}

#[test]
fn buffered_transfer_barrier_stops_before_replacement_events() {
    let barrier = WorldIngress::FastTransferBarrier {
        session_generation: SessionGeneration::new(1),
        sequence: protocol::identity::FifoSequence::new(1),
        action_sequence: 7,
    };
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let replacement = event(2, movement());
    sender.try_send(replacement.clone()).unwrap();
    let now = Instant::now();
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    let mut pending = Some(barrier.clone());
    assert_eq!(
        drain.next(&mut receiver, &mut pending, |_| true, now),
        Some(barrier)
    );
    assert!(
        drain
            .next(&mut receiver, &mut pending, |_| true, now)
            .is_none()
    );
    assert!(pending.is_none());
    assert_eq!(receiver.try_recv().unwrap(), replacement);
}

#[test]
fn pending_event_count_includes_the_buffered_head() {
    let (mut network, _epoch) = crate::network::NetworkHandle::<()>::stub();
    assert_eq!(network.pending_event_count(), 0);
    *network.world_ingress_mut().1 = Some(event(1, movement()));
    assert_eq!(network.pending_event_count(), 1);
    let _ = network.world_ingress_mut().1.take();
    assert_eq!(network.pending_event_count(), 0);
}

/// Terrain requests and movement receipts fit without blocking the writer across busy frames.
#[test]
fn frame_control_drain_keeps_up_with_a_full_frame_of_send_receipts() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(client_session::CONTROL_EVENT_CAPACITY);
    for frame in 0..1_000 {
        for receipt in 0..=OUTBOUND_SEND_BUDGET_PER_FRAME {
            assert!(
                tx.try_send((frame, receipt)).is_ok(),
                "receipts backed up by frame {frame}"
            );
        }
        let drained = drain_network_controls(&mut rx, NETWORK_CONTROL_BUDGET_PER_FRAME);
        assert_eq!(
            drained,
            (0..=OUTBOUND_SEND_BUDGET_PER_FRAME)
                .map(|receipt| (frame, receipt))
                .collect::<Vec<_>>()
        );
    }
}
