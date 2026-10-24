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
        force_local_move: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    }))
}

/// Creates a stream with independent terrain and light admission lanes.
fn stream() -> chunk_pipeline::WorldStream {
    chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

/// Fills both terrain lanes without dispatching their decode jobs.
fn full_stream() -> (chunk_pipeline::WorldStream, u64) {
    let mut stream = stream();
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
        .next(&mut receiver, &mut pending, Some(&mut stream), || now)
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
            .next(&mut receiver, &mut pending, None, || now)
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
        drain
            .next(&mut receiver, &mut pending, None, || now)
            .unwrap(),
        Some(barrier)
    );
    assert!(
        drain
            .next(&mut receiver, &mut pending, None, || now)
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
        .next(&mut receiver, &mut pending, Some(&mut stream), || now)
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
        .next(&mut receiver, &mut pending, Some(&mut stream), || now)
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
                    .next(&mut receiver, &mut pending, Some(&mut stream), || start)
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
                .next(&mut receiver, &mut pending, Some(&mut stream), || start)
                .unwrap()
                .is_none()
        );
        assert_eq!(pending.len(), limit);
        assert_eq!(receiver.len(), 1);
    }
}

/// Advances the supplied frame clock to its deadline after one item can be inspected.
fn one_item_clock(start: Instant) -> impl FnMut() -> Instant {
    let deadline = start + WORLD_INGRESS_DRAIN_BUDGET;
    let mut times = [start, start].into_iter();
    move || times.next().unwrap_or(deadline)
}

#[test]
fn lookahead_deadline_leaves_uninspected_receiver_payloads_for_next_frame() {
    let (mut stream, full) = full_stream();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
    for offset in 1..=3 {
        sender
            .try_send(event(full + offset, terrain(1000)))
            .unwrap();
    }
    let now = Instant::now();
    let mut pending = VecDeque::new();
    assert!(
        WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET)
            .next(
                &mut receiver,
                &mut pending,
                Some(&mut stream),
                one_item_clock(now)
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(pending.len(), 1);
    assert_eq!(receiver.len(), 2);
    assert_eq!(sequence(pending.front().unwrap()), full + 1);
    assert!(
        WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET)
            .next(&mut receiver, &mut pending, Some(&mut stream), || now)
            .unwrap()
            .is_none()
    );
    assert_eq!(pending.len(), 3);
    assert!(receiver.is_empty());
}

#[test]
fn lookahead_deadline_stops_retained_scan_before_later_light_admission() {
    let (mut stream, full) = full_stream();
    let mut pending = VecDeque::new();
    for (offset, value) in [terrain(1000), movement()].into_iter().enumerate() {
        let sequence = full + offset as u64 + 1;
        let footprint = stream.ingress_footprint(&value);
        stream
            .retain_ingress(sequence, footprint, client_session::WORLD_EVENT_CAPACITY)
            .unwrap();
        pending.push_back(event(sequence, value));
    }
    let (_sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let now = Instant::now();
    assert!(
        WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET)
            .next(
                &mut receiver,
                &mut pending,
                Some(&mut stream),
                one_item_clock(now)
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(pending.len(), 2);
    assert!(matches!(
        WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET)
            .next(&mut receiver, &mut pending, Some(&mut stream), || now).unwrap(),
        Some(WorldIngress::Event(event)) if event.sequence.get() == full + 2
    ));
}

/// Light consumer backpressure cannot spend the independent terrain allowance.
#[test]
fn full_light_lane_still_admits_terrain_head() {
    let mut stream = stream();
    let full = client_world::ingestion::MAX_ADMITTED_WORLD_EVENTS as u64;
    for sequence in 1..=full {
        stream
            .submit(sequence, WorldEvent::NetworkStackLatency(sequence))
            .unwrap();
    }
    assert_eq!(stream.light_admission_capacity(), 0);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    for sequence in full + 1..=full + 2 {
        sender
            .try_send(event(sequence, terrain(sequence as i32)))
            .unwrap();
    }
    let now = Instant::now();
    let mut pending = VecDeque::new();
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    for sequence in full + 1..=full + 2 {
        let WorldIngress::Event(event) = drain
            .next(&mut receiver, &mut pending, Some(&mut stream), || now)
            .unwrap()
            .unwrap()
        else {
            panic!("terrain still has admission credit");
        };
        assert_eq!(event.sequence.get(), sequence);
        stream.submit(sequence, event.event).unwrap();
    }
    assert_eq!(stream.light_admission_capacity(), 0);
    assert!(pending.is_empty());
    assert!(receiver.is_empty());
}

/// An unreserved light payload stops further reads until consumer fanout releases credit.
#[test]
fn full_light_lane_stops_lookahead_then_reserves_credit_when_consumers_drain() {
    let (mut stream, terrain_count) = full_stream();
    let light_count = client_world::ingestion::MAX_ADMITTED_WORLD_EVENTS as u64;
    for sequence in terrain_count + 1..=terrain_count + light_count {
        stream
            .submit(sequence, WorldEvent::NetworkStackLatency(sequence))
            .unwrap();
    }
    assert_eq!(stream.light_admission_capacity(), 0);
    let full = terrain_count + light_count;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
    sender.try_send(event(full + 1, terrain(1000))).unwrap();
    sender
        .try_send(event(
            full + 2,
            WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                runtime_id: 1,
                position: [100.5, 80.0, 100.5],
                mode: protocol::MovePlayerMode::Teleport,
                ..Default::default()
            }),
        ))
        .unwrap();
    sender.try_send(event(full + 3, movement())).unwrap();
    let now = Instant::now();
    let mut pending = VecDeque::new();
    for _ in 0..2 {
        assert!(
            WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET)
                .next(&mut receiver, &mut pending, Some(&mut stream), || now)
                .unwrap()
                .is_none()
        );
        assert_eq!(pending.len(), 2);
        assert_eq!(receiver.len(), 1);
    }
    assert_eq!(stream.take_committed_controls().len(), light_count as usize);
    let Some(WorldIngress::Event(event)) = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET)
        .next(&mut receiver, &mut pending, Some(&mut stream), || now)
        .unwrap()
    else {
        panic!("fanout released credit for the retained teleport and later movement");
    };
    assert_eq!(event.sequence.get(), full + 3);
    stream.submit(event.sequence.get(), event.event).unwrap();
    assert_eq!(stream.light_admission_capacity(), light_count as usize - 2);
    assert!(stream.take_committed_controls().is_empty());
    assert_eq!(pending.len(), 2);
    assert!(receiver.is_empty());
}

/// A retained teleport and block mutation keep latency and actor consumers in wire order.
#[test]
fn retained_light_and_mutation_keep_later_latency_and_movement_ordered() {
    let (mut stream, full) = full_stream();
    let teleport = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [100.5, 80.0, 100.5],
        mode: protocol::MovePlayerMode::Teleport,
        ..Default::default()
    });
    let values = [
        terrain(1000),
        teleport,
        WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
            dimension: 0,
            position: [16_000, 0, 0],
            layer: 0,
            network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        }]),
        WorldEvent::NetworkStackLatency(777),
        movement(),
    ];
    let (sender, mut receiver) = tokio::sync::mpsc::channel(values.len());
    for (offset, value) in values.into_iter().enumerate() {
        sender
            .try_send(event(full + offset as u64 + 1, value))
            .unwrap();
    }
    let now = Instant::now();
    let mut pending = VecDeque::new();
    let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
    while let Some(WorldIngress::Event(event)) = drain
        .next(&mut receiver, &mut pending, Some(&mut stream), || now)
        .unwrap()
    {
        stream.submit(event.sequence.get(), event.event).unwrap();
    }
    assert_eq!(
        pending.iter().map(sequence).collect::<Vec<_>>(),
        [full + 1, full + 2, full + 3]
    );
    assert!(stream.take_committed_controls().is_empty());
    assert_eq!(stream.committed_sequence(), 0);
    assert_eq!(stream.inventory_committed_through(), Some(full + 1));
    assert!(receiver.is_empty());
}

/// A retained head spends its own light slot, including when that head ends a session.
#[test]
fn retained_light_head_spends_its_reserved_credit_when_lane_is_full() {
    let full = client_world::ingestion::MAX_ADMITTED_WORLD_EVENTS as u64;
    for transfer in [false, true] {
        let mut stream = stream();
        let ingress = if transfer {
            WorldIngress::FastTransferBarrier {
                session_generation: SessionGeneration::new(1),
                sequence: protocol::identity::FifoSequence::new(1),
                action_sequence: 7,
            }
        } else {
            event(
                1,
                WorldEvent::MovePlayer(protocol::MovePlayerEvent {
                    runtime_id: 1,
                    position: [100.5, 80.0, 100.5],
                    mode: protocol::MovePlayerMode::Teleport,
                    ..Default::default()
                }),
            )
        };
        assert!(
            stream
                .retain_ingress(
                    1,
                    footprint(&ingress, &stream),
                    client_session::WORLD_EVENT_CAPACITY
                )
                .unwrap()
        );
        let mut pending = VecDeque::from([ingress.clone()]);
        for sequence in 2..=full {
            stream.submit(sequence, movement()).unwrap();
        }
        assert_eq!(stream.light_admission_capacity(), 0);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        sender.try_send(event(full + 1, movement())).unwrap();
        let now = Instant::now();
        let mut drain = WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET);
        let admitted = drain
            .next(&mut receiver, &mut pending, Some(&mut stream), || now)
            .unwrap()
            .unwrap();
        assert_eq!(admitted, ingress);
        match admitted {
            WorldIngress::Event(event) => stream.submit(event.sequence.get(), event.event).unwrap(),
            WorldIngress::FastTransferBarrier { sequence, .. } => {
                stream.commit_barrier(sequence.get()).unwrap()
            }
            WorldIngress::LevelChunk { .. } => unreachable!(),
        }
        assert_eq!(stream.committed_sequence(), full);
        assert!(pending.is_empty());
        if transfer {
            assert!(
                drain
                    .next(&mut receiver, &mut pending, Some(&mut stream), || now)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(receiver.len(), 1);
    }
}
