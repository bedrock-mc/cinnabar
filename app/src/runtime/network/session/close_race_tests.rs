use super::*;

#[test]
fn closed_command_deferral_ends_after_nonterminal_controls_are_drained() {
    let (mut handle, control_sender) = NetworkHandle::stub_with_control_sender();
    for _ in 0..CONTROL_EVENT_CAPACITY.min(17) {
        control_sender
            .try_send(NetworkControlEvent::BlobCacheTelemetry {
                enabled: true,
                stats: BlobCacheStats::default(),
            })
            .unwrap();
    }
    drop(control_sender);

    assert!(handle.closed_command_has_pending_control());
    for _ in 0..16 {
        handle.control_events_mut().try_recv().unwrap();
    }
    assert!(handle.closed_command_has_pending_control());
    handle.control_events_mut().try_recv().unwrap();
    assert!(!handle.closed_command_has_pending_control());
    assert!(matches!(
        handle.send_packet(test_packet()),
        Err(PacketSendError::Closed(_))
    ));
}

#[test]
fn closed_command_deferral_preserves_transfer_controls() {
    let (handle, control_sender) = NetworkHandle::stub_with_control_sender();
    control_sender
        .try_send(NetworkControlEvent::Transferred {
            target: SessionTransferTarget {
                host: "transfer.example.net".to_owned(),
                port: 19132,
            },
            decode_error_count: 0,
        })
        .unwrap();

    assert!(handle.closed_command_has_pending_control());
}

#[test]
fn closed_command_predicate_catches_terminal_queued_after_precheck() {
    let (control_sender, control_events) = mpsc::channel(2);
    let (_world_sender, world_events) = mpsc::channel(1);
    let (commands, command_receiver) = mpsc::channel(1);
    let (physics_reanchor, _physics_reanchor_rx) = watch::channel(0);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    let handle = NetworkHandle {
        control_events,
        world_events,
        commands,
        physics_reanchor,
        shutdown,
        thread: None,
        readiness_ingress: Arc::new(ReadinessIngressCounter::default()),
    };

    assert!(!handle.closed_command_has_pending_control());
    control_sender
        .try_send(NetworkControlEvent::Stopped {
            decode_error_count: 0,
        })
        .unwrap();
    drop(command_receiver);
    assert!(matches!(
        handle.send_packet(test_packet()),
        Err(PacketSendError::Closed(_))
    ));
    assert!(handle.closed_command_has_pending_control());
}
