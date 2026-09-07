use super::*;
use protocol::ServerDisconnectEvent;

fn kicked_disconnect() -> ServerDisconnectEvent {
    ServerDisconnectEvent {
        reason: "Kicked".to_owned(),
        message: Some("We've detected movement cheats".to_owned()),
        filtered_message: None,
    }
}

struct KickedInboundSession {
    error: Option<&'static str>,
    disconnect: Option<ServerDisconnectEvent>,
}

impl NetworkSession for KickedInboundSession {
    type Error = &'static str;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        match self.error.take() {
            Some(error) => Err(error),
            None => future::pending().await,
        }
    }

    async fn send_packet(&mut self, _packet: protocol::Packet) -> Result<(), Self::Error> {
        Ok(())
    }

    fn decode_error_count(&self) -> u64 {
        3
    }

    fn take_server_disconnect(&mut self) -> Option<ServerDisconnectEvent> {
        self.disconnect.take()
    }
}

#[tokio::test]
async fn receive_failure_attaches_the_retained_server_disconnect() {
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (_commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);

    run_network_pump(
        KickedInboundSession {
            error: Some("socket read failed"),
            disconnect: Some(kicked_disconnect()),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    )
    .await;

    let Some(NetworkControlEvent::Failed {
        message,
        decode_error_count,
        server_disconnect,
        origin,
    }) = controls.recv().await
    else {
        panic!("receive failure must end the pump with Failed");
    };
    assert_eq!(message, "socket read failed");
    assert_eq!(decode_error_count, 3);
    assert_eq!(server_disconnect.as_ref(), Some(&kicked_disconnect()));
    assert_eq!(origin, NetworkFailureOrigin::Receive);
}

#[tokio::test]
async fn send_failure_reports_the_local_send_origin() {
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: None,
            physics_reanchor: None,
            mining: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);

    run_network_pump(
        FailingSendSession,
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    )
    .await;

    let events = std::iter::from_fn(|| controls.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        events.iter().any(|event| matches!(
            event,
            NetworkControlEvent::Failed {
                origin: NetworkFailureOrigin::Send,
                ..
            }
        )),
        "an outbound write failure must never claim a remote-initiated close"
    );
}

#[tokio::test]
async fn receive_failure_reports_the_remote_receive_origin() {
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (_commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);

    run_network_pump(
        KickedInboundSession {
            error: Some("upstream read timed out"),
            disconnect: None,
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    )
    .await;

    assert!(matches!(
        controls.recv().await,
        Some(NetworkControlEvent::Failed {
            origin: NetworkFailureOrigin::Receive,
            ..
        })
    ));
}

#[tokio::test]
async fn terminal_marker_includes_the_server_disconnect_only_when_present() {
    let disconnect = kicked_disconnect();
    let mut with_reason = Vec::new();
    write_network_pump_terminal_marker(
        &mut with_reason,
        "receive",
        "connection closed",
        2,
        Some(&disconnect),
    );
    let payload = String::from_utf8(with_reason).expect("marker is UTF-8");
    let marker: serde_json::Value = serde_json::from_str(
        payload
            .trim()
            .strip_prefix("RUST_MCBE_NETWORK_PUMP_TERMINAL=")
            .expect("marker prefix"),
    )
    .expect("marker JSON");
    assert_eq!(
        marker["server_disconnect"]["message"],
        "We've detected movement cheats"
    );
    assert_eq!(marker["server_disconnect"]["reason"], "Kicked");

    let mut without_reason = Vec::new();
    write_network_pump_terminal_marker(&mut without_reason, "send", "socket write failed", 0, None);
    let payload = String::from_utf8(without_reason).expect("marker is UTF-8");
    let marker: serde_json::Value = serde_json::from_str(
        payload
            .trim()
            .strip_prefix("RUST_MCBE_NETWORK_PUMP_TERMINAL=")
            .expect("marker prefix"),
    )
    .expect("marker JSON");
    assert!(marker.get("server_disconnect").is_none());
}

#[test]
fn failure_display_prefers_the_server_reason_and_falls_back_cleanly() {
    let transport_message = "network read failed: closed";
    let with_reason = session_failure_display(transport_message, Some(&kicked_disconnect()));
    assert_eq!(
        with_reason,
        "server disconnected: We've detected movement cheats (network read failed: closed)"
    );
    let without_reason = session_failure_display(transport_message, None);
    assert_eq!(
        without_reason,
        format!("network session failed: {transport_message}")
    );
}

#[test]
fn failure_display_falls_back_through_filtered_message_and_reason() {
    let transport = "connection closed";
    let filtered = ServerDisconnectEvent {
        reason: "Kicked".to_owned(),
        message: Some(String::new()),
        filtered_message: Some("Policy message".to_owned()),
    };
    assert_eq!(
        session_failure_display(transport, Some(&filtered)),
        "server disconnected: Policy message (connection closed)"
    );

    let reason_only = ServerDisconnectEvent {
        reason: "TimedOut".to_owned(),
        message: None,
        filtered_message: Some("   ".to_owned()),
    };
    assert_eq!(
        session_failure_display(transport, Some(&reason_only)),
        "server disconnected: TimedOut (connection closed)"
    );
}
