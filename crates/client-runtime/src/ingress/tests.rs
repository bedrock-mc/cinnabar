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

/// Fills both terrain lanes without dispatching their decode jobs.
fn full_stream() -> (chunk_pipeline::WorldStream, u64) {
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let count = (MAX_ADMITTED_HEAVY_EVENTS + MAX_DEFERRED_WORLD_EVENTS) as u64;
    for sequence in 1..=count {
        stream.submit(sequence, terrain(sequence as i32)).unwrap();
    }
    (stream, count)
}

#[test]
fn remote_moves_drain_when_heavy_and_deferred_terrain_are_full() {
    let (mut stream, terrain_count) = full_stream();
    assert_eq!(stream.remaining_admission_capacity(), 0);
    assert!(!stream.can_admit(&terrain(1000)));

    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    for sequence in terrain_count + 1..=terrain_count + 2 {
        sender.try_send(event(sequence, movement())).unwrap();
    }
    let now = Instant::now();
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    let mut pending = VecDeque::new();
    let mut applied = Vec::new();
    while let Some(ingress) = drain
        .next(&mut receiver, &mut pending, Some(&mut stream), now)
        .unwrap()
    {
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
    assert!(pending.is_empty());
    assert!(receiver.is_empty());
}

#[test]
fn expired_frame_leaves_the_buffered_head_untouched() {
    let first = event(1, movement());
    let mut pending = VecDeque::from([first.clone()]);
    let (_sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let now = Instant::now();
    let mut drain = WorldIngressDrain::new(now);
    assert!(
        drain
            .next(&mut receiver, &mut pending, None, now)
            .unwrap()
            .is_none()
    );
    assert_eq!(pending, [first]);
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
    let mut pending = VecDeque::from([barrier.clone()]);
    assert_eq!(
        drain.next(&mut receiver, &mut pending, None, now).unwrap(),
        Some(barrier)
    );
    assert!(
        drain
            .next(&mut receiver, &mut pending, None, now)
            .unwrap()
            .is_none()
    );
    assert!(pending.is_empty());
    assert_eq!(receiver.try_recv().unwrap(), replacement);
}

#[test]
fn pending_event_count_includes_the_buffered_head() {
    let (mut network, _epoch) = crate::network::NetworkHandle::<()>::stub();
    assert_eq!(network.pending_event_count(), 0);
    network
        .world_ingress_mut()
        .1
        .push_back(event(1, movement()));
    assert_eq!(network.pending_event_count(), 1);
    let _ = network.world_ingress_mut().1.pop_front();
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

/// A stalled terrain head must not keep independent consumers out of their lanes.
#[test]
fn stalled_chunk_head_still_admits_inventory_hud_movement_and_latency() {
    let (mut stream, full) = full_stream();
    let blocked_chunks = client_session::WORLD_EVENT_CAPACITY - 6;
    let mut events: Vec<_> = (0..blocked_chunks)
        .map(|index| terrain(1000 + index as i32))
        .collect();
    events.extend([
        WorldEvent::Inventory(protocol::InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        )),
        WorldEvent::Ui(protocol::UiEvent::Hud(protocol::HudEvent::Health {
            health: 7,
        })),
        movement(),
        WorldEvent::NetworkStackLatency(777),
        WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
            dimension: 0,
            position: [16_000, 0, 0],
            layer: 0,
            network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        }]),
        WorldEvent::BlockEntityUpdate(protocol::BlockEntityUpdateEvent {
            dimension: 0,
            position: [16_000, 0, 0],
            nbt: Default::default(),
        }),
    ]);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(events.len());
    for (index, value) in events.into_iter().enumerate() {
        sender
            .try_send(event(full + index as u64 + 1, value))
            .unwrap();
    }
    let now = Instant::now();
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    let mut pending = VecDeque::new();
    let mut admitted = Vec::new();
    while let Some(WorldIngress::Event(event)) = drain
        .next(&mut receiver, &mut pending, Some(&mut stream), now)
        .unwrap()
    {
        admitted.push(event.sequence.get());
        if matches!(event.event, WorldEvent::Inventory(_)) {
            stream.commit(event.sequence.get()).unwrap();
        } else {
            stream.submit(event.sequence.get(), event.event).unwrap();
        }
    }
    let first_light = full + blocked_chunks as u64 + 1;
    assert_eq!(
        admitted,
        [
            first_light,
            first_light + 1,
            first_light + 2,
            first_light + 3
        ]
    );
    assert_eq!(stream.take_committed_ui().len(), 1);
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [client_world::CommittedControlEvent::NetworkStackLatency {
            creation_time: 777,
            ..
        }]
    ));
    assert_eq!(stream.committed_sequence(), 0);
    assert_eq!(
        pending.len(),
        blocked_chunks + 2,
        "chunks, block update and block entity must still wait"
    );
    assert!(receiver.is_empty());
}

/// Earlier block mutations remain a local-authority fence even during ingress lookahead.
#[test]
fn retained_block_mutation_keeps_later_inventory_and_latency_ordered() {
    let (mut stream, full) = full_stream();
    let updates = WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
        dimension: 0,
        position: [16, 0, 0],
        layer: 0,
        network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
    }]);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
    sender.try_send(event(full + 1, updates)).unwrap();
    sender
        .try_send(event(
            full + 2,
            WorldEvent::Inventory(protocol::InventoryEvent::Authority(
                protocol::InventoryAuthority::Server,
            )),
        ))
        .unwrap();
    sender
        .try_send(event(full + 3, WorldEvent::NetworkStackLatency(777)))
        .unwrap();
    let now = Instant::now();
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    let mut pending = VecDeque::new();
    while let Some(WorldIngress::Event(event)) = drain
        .next(&mut receiver, &mut pending, Some(&mut stream), now)
        .unwrap()
    {
        if matches!(event.event, WorldEvent::Inventory(_)) {
            stream.commit(event.sequence.get()).unwrap();
        } else {
            stream.submit(event.sequence.get(), event.event).unwrap();
        }
    }
    assert_eq!(pending.len(), 1);
    assert_eq!(stream.inventory_committed_through(), Some(full));
    assert!(stream.take_committed_controls().is_empty());
}

/// Definition and transfer markers stop lookahead before a replacement world's packets.
#[test]
fn stalled_terrain_keeps_dimension_and_transfer_barriers_ahead_of_later_events() {
    for transfer in [false, true] {
        let (mut stream, full) = full_stream();
        let marker = if transfer {
            WorldIngress::FastTransferBarrier {
                session_generation: SessionGeneration::new(1),
                sequence: protocol::identity::FifoSequence::new(full + 2),
                action_sequence: 7,
            }
        } else {
            event(
                full + 2,
                WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                    dimension: 1,
                    position: [0.0; 3],
                    respawn: false,
                    loading_screen_id: None,
                }),
            )
        };
        let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
        sender.try_send(event(full + 1, terrain(1000))).unwrap();
        sender.try_send(marker).unwrap();
        let later = event(full + 3, WorldEvent::NetworkStackLatency(777));
        sender.try_send(later.clone()).unwrap();
        let now = Instant::now();
        let mut pending = VecDeque::new();
        for frame in 0..2 {
            let start = now + WORLD_INGRESS_DRAIN_BUDGET * frame;
            let mut drain = WorldIngressDrain::new(start + WORLD_INGRESS_DRAIN_BUDGET);
            assert!(
                drain
                    .next(&mut receiver, &mut pending, Some(&mut stream), start)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(pending.len(), 2);
            assert_eq!(receiver.len(), 1);
        }
        assert_eq!(receiver.try_recv().unwrap(), later);
        assert_eq!(stream.current_dimension(), 0);
        assert!(stream.take_committed_controls().is_empty());
    }
}

/// A prolonged stall cannot grow retained payloads or ordering records without a bound.
#[test]
fn ingress_lookahead_stays_bounded_across_stalled_frames() {
    let (mut stream, full) = full_stream();
    let limit = client_session::WORLD_EVENT_CAPACITY;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(limit + 1);
    for index in 0..=limit {
        sender
            .try_send(event(full + index as u64 + 1, terrain(1000 + index as i32)))
            .unwrap();
    }
    let now = Instant::now();
    let mut pending = VecDeque::new();
    for frame in 0..3 {
        let start = now + WORLD_INGRESS_DRAIN_BUDGET * frame;
        let mut drain = WorldIngressDrain::new(start + WORLD_INGRESS_DRAIN_BUDGET);
        assert!(
            drain
                .next(&mut receiver, &mut pending, Some(&mut stream), start)
                .unwrap()
                .is_none()
        );
        assert_eq!(pending.len(), limit);
        assert_eq!(receiver.len(), 1);
    }
}
