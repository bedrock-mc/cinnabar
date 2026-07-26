use super::*;

struct PendingSendSession {
    started: Option<oneshot::Sender<()>>,
}

impl NetworkSession for PendingSendSession {
    type Error = &'static str;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        future::pending().await
    }

    async fn send_packet(&mut self, _packet: protocol::Packet) -> Result<(), Self::Error> {
        if let Some(started) = self.started.take() {
            let _ = started.send(());
        }
        future::pending().await
    }

    fn decode_error_count(&self) -> u64 {
        0
    }
}

struct CountingSendSession {
    sends: Arc<AtomicUsize>,
}

impl NetworkSession for CountingSendSession {
    type Error = &'static str;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        future::pending().await
    }

    async fn send_packet(&mut self, _packet: protocol::Packet) -> Result<(), Self::Error> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn decode_error_count(&self) -> u64 {
        0
    }
}

#[tokio::test]
async fn reanchor_cancels_an_admitted_but_unstarted_physics_send_before_socket_write() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let (reanchor, reanchor_rx) = watch::channel(0);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: Some(reanchor_rx),
        })
        .unwrap();
    reanchor.send_replace(1);
    let sends = Arc::new(AtomicUsize::new(0));
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        CountingSendSession {
            sends: Arc::clone(&sends),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::PhysicsPacketCancelled {
            identity: observed,
            definitely_unsent: true,
        })) if observed == identity
    ));
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    shutdown.send_replace(true);
    worker.await.unwrap();
}

#[tokio::test]
async fn physics_send_ack_is_emitted_only_after_successful_socket_write() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        ReadyInboundSession {
            inbound: None,
            inbound_selected: Arc::new(AtomicBool::new(false)),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::PhysicsPacketSent {
            identity: observed,
        })) if observed == identity
    ));
    shutdown.send_replace(true);
    worker.await.unwrap();
}

#[tokio::test]
async fn failed_physics_socket_write_never_emits_success_ack() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: None,
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
        !events
            .iter()
            .any(|event| matches!(event, NetworkControlEvent::PhysicsPacketSent { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, NetworkControlEvent::Failed { .. }))
    );
}

#[tokio::test]
async fn cancelled_pending_physics_socket_write_never_emits_success_ack() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (started_tx, started_rx) = oneshot::channel();
    let worker = tokio::spawn(run_network_pump(
        PendingSendSession {
            started: Some(started_tx),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    started_rx.await.unwrap();
    shutdown.send_replace(true);
    worker.await.unwrap();
    let events = std::iter::from_fn(|| controls.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, NetworkControlEvent::PhysicsPacketSent { .. }))
    );
}

#[tokio::test]
async fn reanchor_during_an_in_flight_physics_send_reports_indeterminate_cancellation() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let (reanchor, reanchor_rx) = watch::channel(0);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: Some(reanchor_rx),
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (started_tx, started_rx) = oneshot::channel();
    let worker = tokio::spawn(run_network_pump(
        PendingSendSession {
            started: Some(started_tx),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    started_rx.await.unwrap();
    reanchor.send_replace(1);
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::PhysicsPacketCancelled {
            identity: observed,
            definitely_unsent: false,
        })) if observed == identity
    ));
    shutdown.send_replace(true);
    worker.await.unwrap();
}
