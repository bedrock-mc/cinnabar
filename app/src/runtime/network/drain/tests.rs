use super::*;
use crate::runtime::network::{NETWORK_INGRESS_BUDGET_PER_FRAME, session::SequencedWorldEvent};
use chunk_pipeline::{MAX_ADMITTED_WORLD_EVENTS, WorldStream};
use client_session::WorldIngress;
use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, WorldEvent,
};
use std::sync::Arc;

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
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let retained = MAX_ADMITTED_WORLD_EVENTS / 4;
    for sequence in 1..=retained as u64 {
        submit(&mut stream, metadata(sequence));
    }
    let count = NETWORK_INGRESS_BUDGET_PER_FRAME;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(count);
    for sequence in retained + 1..=retained + count {
        sender.try_send(metadata(sequence as u64)).unwrap();
    }
    let mut consumed = Vec::new();
    let mut drain = WorldIngressDrain::new(count);
    while let Some(ingress) = drain.next(&mut receiver, stream.remaining_admission_capacity()) {
        consumed.push(submit(&mut stream, ingress));
    }
    assert_eq!(consumed.len(), retained);
    assert_eq!(receiver.len(), count - retained);
    assert_eq!(stream.remaining_admission_capacity(), 0);
    assert_eq!(stream.take_committed_controls().len(), retained * 2);
    assert_eq!(stream.take_committed_ui().len(), retained * 2);

    let mut drain = WorldIngressDrain::new(count);
    while let Some(ingress) = drain.next(&mut receiver, stream.remaining_admission_capacity()) {
        consumed.push(submit(&mut stream, ingress));
    }
    assert_eq!(
        consumed,
        ((retained + 1) as u64..=(retained + count) as u64).collect::<Vec<_>>()
    );
    assert_eq!(receiver.len(), 0);
    assert_eq!(stream.take_committed_controls().len(), count - retained);
    assert_eq!(stream.take_committed_ui().len(), count - retained);
}

#[test]
fn world_ingress_cursor_preserves_frame_cap_and_fifo_with_available_admission() {
    let count = NETWORK_INGRESS_BUDGET_PER_FRAME;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(count + 1);
    for sequence in 1..=count + 1 {
        sender.try_send(metadata(sequence as u64)).unwrap();
    }
    let mut drain = WorldIngressDrain::new(count);
    for sequence in 1..=count {
        assert!(matches!(
            drain.next(&mut receiver, 1),
            Some(WorldIngress::Event(event)) if event.sequence == sequence as u64
        ));
    }
    assert!(drain.next(&mut receiver, 1).is_none());
    assert_eq!(receiver.len(), 1);
    let mut next_frame = WorldIngressDrain::new(count);
    assert!(matches!(
        next_frame.next(&mut receiver, 1),
        Some(WorldIngress::Event(event)) if event.sequence == (count + 1) as u64
    ));
}

#[test]
fn world_ingress_cursor_leaves_zero_admission_events_queued() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    sender.try_send(metadata(1)).unwrap();
    let mut drain = WorldIngressDrain::new(1);
    assert!(drain.next(&mut receiver, 0).is_none());
    assert_eq!(receiver.len(), 1);
    assert!(matches!(
        drain.next(&mut receiver, 1),
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
    let before = thread_allocations();
    let mut drain = WorldIngressDrain::new(2);
    let blocked = drain.next(&mut receiver, 0).is_none();
    let first = matches!(
        drain.next(&mut receiver, 1),
        Some(WorldIngress::Event(event)) if event.sequence == 1
    );
    let second = matches!(
        drain.next(&mut receiver, 1),
        Some(WorldIngress::Event(event)) if event.sequence == 2
    );
    let capped = drain.next(&mut receiver, 1).is_none();
    let mut next_frame = WorldIngressDrain::new(2);
    let third = matches!(
        next_frame.next(&mut receiver, 1),
        Some(WorldIngress::Event(event)) if event.sequence == 3
    );
    let empty = next_frame.next(&mut receiver, 1).is_none();
    let allocated = thread_allocations() - before;

    assert_eq!(allocated, 0);
    assert!(blocked && first && second && capped && third && empty);
}
