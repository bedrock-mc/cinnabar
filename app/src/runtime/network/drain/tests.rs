use super::*;
use crate::runtime::network::{WORLD_INGRESS_DRAIN_BUDGET, session::SequencedWorldEvent};
use client_session::{WORLD_EVENT_CAPACITY, WorldIngress};
use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, ActorMoveEvent,
    ActorPositionOrigin, LevelChunkEvent, LevelChunkMode, WorldEvent,
};
use std::{sync::Arc, time::Instant};
use {
    chunk_pipeline::WorldStream,
    client_world::ingestion::{MAX_ADMITTED_HEAVY_EVENTS, MAX_ADMITTED_WORLD_EVENTS},
};

/// A drain whose clock the test holds still, so only admission and the channel stop it.
fn frame_drain(now: Instant) -> WorldIngressDrain {
    WorldIngressDrain::new(now + WORLD_INGRESS_DRAIN_BUDGET)
}

fn stream() -> WorldStream {
    WorldStream::new(protocol::WorldBootstrap {
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

fn event(sequence: u64, event: WorldEvent) -> WorldIngress {
    WorldIngress::Event(SequencedWorldEvent {
        session_generation: 1,
        sequence,
        event,
    })
}

fn remote_move(sequence: u64) -> WorldIngress {
    event(
        sequence,
        WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 9,
            position: [Some(sequence as f32); 3],
            position_origin: ActorPositionOrigin::NetworkOffset,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: None,
            teleported: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        })),
    )
}

fn level_chunk(sequence: u64) -> WorldEvent {
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x: sequence as i32,
        z: 0,
        mode: LevelChunkMode::Inline { count: 1 },
        payload: Vec::new(),
    })
}

/// Drains one frame through the real admission path; returns the consumed sequences.
fn drain_frame(
    stream: &mut WorldStream,
    receiver: &mut tokio::sync::mpsc::Receiver<WorldIngress>,
) -> Vec<u64> {
    let now = Instant::now();
    let mut drain = frame_drain(now);
    let mut consumed = Vec::new();
    while let Some(ingress) = drain.next(receiver, stream.remaining_admission_capacity(), now) {
        consumed.push(submit(stream, ingress));
    }
    consumed
}

/// Produces one local metadata event that retains both a UI delta and a movement control.
fn metadata(sequence: u64) -> WorldIngress {
    WorldIngress::Event(SequencedWorldEvent {
        session_generation: 1,
        sequence,
        event: WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
            runtime_id: 1,
            dimension: 0,
            metadata: Arc::from([ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(0),
            }]),
            properties: Arc::from([]),
            tick: sequence,
        })),
    })
}

/// Applies a test ingress packet through the real admission and consumer publication path.
fn submit(stream: &mut WorldStream, ingress: WorldIngress) -> u64 {
    let WorldIngress::Event(event) = ingress else {
        panic!("expected an ordinary world event");
    };
    stream
        .submit(event.sequence, event.event)
        .expect("backpressure must leave unadmitted events in the receiver");
    event.sequence
}

#[test]
fn world_ingress_rechecks_admission_after_consumer_fanout() {
    let mut stream = stream();
    let retained = MAX_ADMITTED_WORLD_EVENTS / 4;
    for sequence in 1..=retained as u64 {
        submit(&mut stream, metadata(sequence));
    }
    let count = MAX_ADMITTED_WORLD_EVENTS / 2;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(count);
    for sequence in retained + 1..=retained + count {
        sender.try_send(metadata(sequence as u64)).unwrap();
    }
    let mut consumed = drain_frame(&mut stream, &mut receiver);
    assert_eq!(consumed.len(), retained);
    assert_eq!(receiver.len(), count - retained);
    assert_eq!(stream.remaining_admission_capacity(), 0);
    assert_eq!(stream.take_committed_controls().len(), retained * 2);
    assert_eq!(stream.take_committed_ui().len(), retained * 2);

    consumed.extend(drain_frame(&mut stream, &mut receiver));
    assert_eq!(
        consumed,
        ((retained + 1) as u64..=(retained + count) as u64).collect::<Vec<_>>()
    );
    assert_eq!(receiver.len(), 0);
    assert_eq!(stream.take_committed_controls().len(), count - retained);
    assert_eq!(stream.take_committed_ui().len(), count - retained);
}

#[test]
fn world_ingress_stops_at_its_time_budget_and_preserves_fifo() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
    for sequence in 1..=3 {
        sender.try_send(metadata(sequence)).unwrap();
    }
    let start = Instant::now();
    let mut drain = frame_drain(start);
    assert!(matches!(
        drain.next(&mut receiver, 1, start),
        Some(WorldIngress::Event(event)) if event.sequence == 1
    ));
    let spent = start + WORLD_INGRESS_DRAIN_BUDGET;
    assert!(drain.next(&mut receiver, 1, spent).is_none());
    assert_eq!(receiver.len(), 2);
    let mut next_frame = frame_drain(spent);
    assert!(matches!(
        next_frame.next(&mut receiver, 1, spent),
        Some(WorldIngress::Event(event)) if event.sequence == 2
    ));
}

/// Terrain at its admission bound neither stops the drain nor delays later light events.
#[test]
fn light_ingress_keeps_committing_while_heavy_admission_is_full() {
    let mut stream = stream();
    let heavy = MAX_ADMITTED_HEAVY_EVENTS as u64;
    for sequence in 1..=heavy {
        stream.submit(sequence, level_chunk(sequence)).unwrap();
    }
    let (sender, mut receiver) = tokio::sync::mpsc::channel(WORLD_EVENT_CAPACITY);
    sender
        .try_send(event(heavy + 1, level_chunk(heavy + 1)))
        .unwrap();
    for sequence in heavy + 2..=heavy + 9 {
        sender.try_send(remote_move(sequence)).unwrap();
    }
    let consumed = drain_frame(&mut stream, &mut receiver);
    assert_eq!(consumed, (heavy + 1..=heavy + 9).collect::<Vec<_>>());
    assert_eq!(
        stream.committed_sequence(),
        0,
        "no poll has decoded a chunk"
    );
    assert_eq!(stream.inventory_committed_through(), Some(heavy + 9));
}

/// A queued burst far above any per-frame count commits in a single drain.
#[test]
fn two_hundred_queued_moves_commit_in_one_drain() {
    const MOVES: u64 = 200;
    let mut stream = stream();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(WORLD_EVENT_CAPACITY);
    for sequence in 1..=MOVES {
        sender.try_send(remote_move(sequence)).unwrap();
    }
    assert_eq!(drain_frame(&mut stream, &mut receiver).len() as u64, MOVES);
    assert_eq!(stream.committed_sequence(), MOVES);
}

#[test]
fn world_ingress_cursor_leaves_zero_admission_events_queued() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    sender.try_send(metadata(1)).unwrap();
    let now = Instant::now();
    let mut drain = frame_drain(now);
    assert!(drain.next(&mut receiver, 0, now).is_none());
    assert_eq!(receiver.len(), 1);
    assert!(matches!(
        drain.next(&mut receiver, 1, now),
        Some(WorldIngress::Event(event)) if event.sequence == 1
    ));
}

#[test]
fn world_ingress_cursor_receives_without_allocating() {
    use crate::tests::alloc_count::thread_allocations;

    let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
    // Warm Tokio's semaphore mutex, which allocates on first use on macOS.
    sender.try_send(metadata(0)).unwrap();
    assert!(receiver.try_recv().is_ok());
    assert!(receiver.try_recv().is_err());
    for sequence in 1..=3 {
        sender.try_send(metadata(sequence)).unwrap();
    }
    let now = Instant::now();
    let spent = now + WORLD_INGRESS_DRAIN_BUDGET;
    let before = thread_allocations();
    let mut drain = frame_drain(now);
    let blocked = drain.next(&mut receiver, 0, now).is_none();
    let first = matches!(
        drain.next(&mut receiver, 1, now),
        Some(WorldIngress::Event(event)) if event.sequence == 1
    );
    let second = matches!(
        drain.next(&mut receiver, 1, now),
        Some(WorldIngress::Event(event)) if event.sequence == 2
    );
    let capped = drain.next(&mut receiver, 1, spent).is_none();
    let mut next_frame = frame_drain(spent);
    let third = matches!(
        next_frame.next(&mut receiver, 1, spent),
        Some(WorldIngress::Event(event)) if event.sequence == 3
    );
    let empty = next_frame.next(&mut receiver, 1, spent).is_none();
    let allocated = thread_allocations() - before;

    assert_eq!(allocated, 0);
    assert!(blocked && first && second && capped && third && empty);
}
