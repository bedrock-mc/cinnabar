//! Witnesses for the transferred session classification: a retained
//! server-directed transfer must end the pump deterministically without a
//! trailing failure or stop record.

use super::*;
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
}

impl TransferredInboundSession {
    fn take_transfer(&mut self) -> Option<protocol::ServerTransferEvent> {
        self.transfer.take()
    }
}

impl NetworkSession for TransferredInboundSession {
    type Error = &'static str;

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

    async fn send_packet(&mut self, _packet: protocol::Packet) -> Result<(), Self::Error> {
        Ok(())
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
            transfer: Some(transferred(transfer_target())),
        },
        NetworkSequencer::new(7, 0, 42),
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
            if sequenced.sequence == 1
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
            transfer: Some(transferred(transfer_target())),
        },
        NetworkSequencer::new(7, 0, 42),
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
        .strip_prefix("RUST_MCBE_NETWORK_PUMP_TERMINAL=")
        .expect("durable marker prefix");
    let marker: serde_json::Value = serde_json::from_str(payload).expect("marker JSON");

    assert_eq!(marker["schema"], "rust-mcbe-network-pump-terminal-v1");
    assert_eq!(marker["outcome"], "transferred");
    assert_eq!(marker["target"]["host"], "game.example.net");
    assert_eq!(marker["target"]["port"], 19133);
    assert_eq!(marker["reload_world"], true);
    assert_eq!(marker["decode_error_count"], 9);
}
