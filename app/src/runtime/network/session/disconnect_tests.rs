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
    }) = controls.recv().await
    else {
        panic!("receive failure must end the pump with Failed");
    };
    assert_eq!(message, "socket read failed");
    assert_eq!(decode_error_count, 3);
    assert_eq!(server_disconnect.as_ref(), Some(&kicked_disconnect()));
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
