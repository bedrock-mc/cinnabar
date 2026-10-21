//! Witnesses for the transferred session classification: a retained
//! server-directed transfer must end the pump deterministically without a
//! trailing failure or stop record.

use super::*;
use simulation_clock::identity::SessionGeneration;
use std::collections::VecDeque;

fn transfer_target() -> SessionTransferTarget {
    SessionTransferTarget {
        host: "game.example.net".to_owned(),
        port: 19133,
    }
}

struct TransferredInboundSession {
    inbound: VecDeque<WorldEvent>,
    error: Option<&'static str>,
    transfer: Option<protocol::ServerTransferEvent>,
    transfer_after: usize,
}

impl TransferredInboundSession {
    fn take_transfer(&mut self) -> Option<protocol::ServerTransferEvent> {
        if self.transfer_after > 0 {
            self.transfer_after -= 1;
            return None;
        }
        self.transfer.take()
    }
}

impl NetworkSession for TransferredInboundSession {
    type Error = &'static str;
    type Outbound = PacketOutbound<&'static str>;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        match self.inbound.pop_front() {
            Some(event) => Ok(event),
            None => future::pending().await,
        }
    }

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        Ok(PacketOutbound::accepting())
    }

    fn decode_error_count(&self) -> u64 {
        4
    }

    fn take_server_disconnect(&mut self) -> Option<protocol::ServerDisconnectEvent> {
        None
    }

    fn take_server_transfer(&mut self) -> Option<protocol::ServerTransferEvent> {
        self.take_transfer()
    }
}

fn transferred(transfer_target: SessionTransferTarget) -> protocol::ServerTransferEvent {
    protocol::ServerTransferEvent {
        host: transfer_target.host,
        port: transfer_target.port,
        reload_world: false,
    }
}

#[tokio::test]
async fn retained_transfer_ends_the_pump_after_prior_world_ingress_without_a_stop_record() {
    let (world_event_tx, mut world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (_commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);

    run_network_pump(
        TransferredInboundSession {
            inbound: VecDeque::from([WorldEvent::ChunkRadiusUpdated(16)]),
            error: None,
            transfer_after: 0,
            transfer: Some(transferred(transfer_target())),
        },
        NetworkSequencer::new(SessionGeneration::new(7), 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    )
    .await;

    // Pre-transfer ingress stays FIFO-ordered ahead of the boundary record.
    assert!(matches!(
        world_events.recv().await,
        Some(WorldIngress::Event(sequenced))
            if sequenced.sequence.get() == 1
                && matches!(sequenced.event, WorldEvent::ChunkRadiusUpdated(16))
    ));
    let Some(NetworkControlEvent::Transferred {
        target,
        decode_error_count,
    }) = controls.recv().await
    else {
        panic!("a retained transfer must classify the session as transferred");
    };
    assert_eq!(target, transfer_target());
    assert_eq!(decode_error_count, 4);
    assert!(
        controls.recv().await.is_none(),
        "the transferred pump must not emit a trailing Stopped record"
    );
}

#[tokio::test]
async fn a_retained_transfer_takes_precedence_over_a_later_receive_failure() {
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (_commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);

    run_network_pump(
        TransferredInboundSession {
            inbound: VecDeque::new(),
            error: Some("socket read failed"),
            transfer_after: 0,
            transfer: Some(transferred(transfer_target())),
        },
        NetworkSequencer::new(SessionGeneration::new(7), 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    )
    .await;

    let Some(NetworkControlEvent::Transferred { target, .. }) = controls.recv().await else {
        panic!("the transfer boundary outranks the later transport failure");
    };
    assert_eq!(target, transfer_target());
    assert!(controls.recv().await.is_none());
}

#[test]
fn transferred_terminal_marker_names_the_exact_target() {
    let mut output = Vec::new();
    write_network_pump_transfer_marker(&mut output, &transfer_target(), true, 9);
    let line = String::from_utf8(output).expect("marker is UTF-8");
    let payload = line
        .trim()
        .strip_prefix("CINNABAR_NETWORK_PUMP_TERMINAL=")
        .expect("durable marker prefix");
    let marker: serde_json::Value = serde_json::from_str(payload).expect("marker JSON");

    assert_eq!(marker["schema"], "rust-mcbe-network-pump-terminal-v1");
    assert_eq!(marker["outcome"], "transferred");
    assert_eq!(marker["target"]["host"], "game.example.net");
    assert_eq!(marker["target"]["port"], 19133);
    assert_eq!(marker["reload_world"], true);
    assert_eq!(marker["decode_error_count"], 9);
}

#[tokio::test]
async fn review_transfer_after_reserving_the_last_world_slot_does_not_deadlock() {
    let (world_tx, mut world_events) = mpsc::channel(1);
    let (_commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (control_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);
    let session = TransferredInboundSession {
        inbound: VecDeque::from([WorldEvent::ChunkRadiusUpdated(16)]),
        error: None,
        transfer_after: 1,
        transfer: Some(transferred(transfer_target())),
    };
    let task = tokio::spawn(run_network_pump(
        session,
        NetworkSequencer::new(SessionGeneration::new(7), 0, 42),
        command_rx,
        control_tx,
        world_tx,
        shutdown_rx,
    ));
    tokio::time::timeout(std::time::Duration::from_secs(1), world_events.recv())
        .await
        .expect("the reserved slot must publish the pending world event")
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(1), controls.recv())
            .await
            .unwrap(),
        Some(NetworkControlEvent::Transferred { .. })
    ));
    task.await.unwrap();
}

/// An inbound backlog the pump has not read yet, ending in a Transfer the session retains only
/// once that backlog is read, as a core that closed after the Transfer leaves it.
struct BehindInboundSession {
    inbound: VecDeque<WorldEvent>,
    transfer: Option<protocol::ServerTransferEvent>,
    retained: Option<protocol::ServerTransferEvent>,
    reached: Option<oneshot::Sender<()>>,
}

impl NetworkSession for BehindInboundSession {
    type Error = &'static str;
    type Outbound = PacketOutbound<&'static str>;

    async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
        if let Some(event) = self.inbound.pop_front() {
            return Ok(event);
        }
        self.retained = self.transfer.take();
        if let Some(reached) = self.reached.take() {
            let _ = reached.send(());
        }
        Err("session boundary")
    }

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        Ok(PacketOutbound::new(|_| future::ready(Err("broken pipe"))))
    }

    fn decode_error_count(&self) -> u64 {
        0
    }

    fn take_server_transfer(&mut self) -> Option<protocol::ServerTransferEvent> {
        self.retained.take()
    }
}

/// A write that fails because the core closed after a Transfer still ends the session as
/// transferred, even while unread world packets sit ahead of that Transfer.
#[tokio::test]
async fn a_failed_write_behind_an_unread_transfer_still_transfers() {
    let (world_tx, mut world_events) = mpsc::channel(1);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (control_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);
    let (reached, boundary) = oneshot::channel();
    let task = tokio::spawn(run_network_pump(
        BehindInboundSession {
            inbound: (0..8).map(WorldEvent::ChunkRadiusUpdated).collect(),
            transfer: Some(transferred(transfer_target())),
            retained: None,
            reached: Some(reached),
        },
        NetworkSequencer::new(SessionGeneration::new(7), 0, 42),
        command_rx,
        control_tx,
        world_tx,
        shutdown_rx,
    ));
    // The full world queue keeps the pump from reading the backlog itself.
    world_events
        .recv()
        .await
        .expect("the first event is published");
    commands
        .send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: None,
            physics_reanchor: None,
            interaction: None,
        })
        .await
        .unwrap();
    commands
        .send(NetworkCommand::FlushFrame {
            cache_opportunity: false,
        })
        .await
        .unwrap();
    tokio::select! {
        reached = boundary => reached.expect("the drain reached the Transfer"),
        event = controls.recv() => panic!("the write failure masked the Transfer: {event:?}"),
    }
    let drained = tokio::spawn(async move { while world_events.recv().await.is_some() {} });
    let Some(NetworkControlEvent::Transferred { target, .. }) = controls.recv().await else {
        panic!("the session must end as transferred");
    };
    assert_eq!(target, transfer_target());
    task.await.unwrap();
    drained.await.unwrap();
}
