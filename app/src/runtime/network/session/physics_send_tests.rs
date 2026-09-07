use super::*;

struct PendingSendSession {
    started: Option<oneshot::Sender<()>>,
    complete: Option<oneshot::Receiver<()>>,
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
        if let Some(complete) = self.complete.take() {
            let _ = complete.await;
            return Ok(());
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

struct RecordingSendSession {
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

fn traced_movement_packets() -> (protocol::Packet, protocol::Packet) {
    let snapshot = protocol::PlayerAuthInputSnapshot {
        tick: 101,
        position: [0.5, 2.620_01, 0.5],
        delta: [0.0; 3],
        move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        camera_orientation: [0.0, 0.0, -1.0],
        flags: protocol::PlayerInputFlags::NONE,
        input_mode: protocol::PlayerInputMode::Mouse,
    };
    let movement = protocol::player_auth_input(snapshot).unwrap();
    let mut block_actions = protocol::BlockActions::new();
    block_actions
        .push(protocol::BlockAction {
            kind: protocol::BlockActionKind::StartDestroy,
            position: [0, 1, -3],
            face: 3,
        })
        .unwrap();
    let combined = protocol::player_auth_input_with_interactions(
        snapshot,
        &protocol::PlayerAuthInputInteractions {
            block_actions,
            block_destroy: None,
        },
    )
    .unwrap();
    (combined, movement)
}

#[test]
fn guarded_trace_uses_the_final_packet_once_and_skips_ordinary_movement() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let (combined, movement) = traced_movement_packets();
    let combined_bytes =
        protocol::encode(&combined, &protocol::BedrockSession { shield_item_id: 0 })
            .unwrap()
            .to_vec();
    let movement_bytes =
        protocol::encode(&movement, &protocol::BedrockSession { shield_item_id: 0 })
            .unwrap()
            .to_vec();

    for (revoked, expected) in [(false, combined_bytes), (true, movement_bytes)] {
        let (authority, authority_rx) = watch::channel(0);
        if revoked {
            authority.send_replace(1);
        }
        let (packet, guarded) = finalize_mining_packet(
            combined.clone(),
            Some(crate::movement::MiningPacketGuard::testing(
                0,
                authority_rx,
                movement.clone(),
            )),
        );
        let mut formatted = Vec::new();
        let trace = guarded_physics_trace_line_with(
            Some(identity),
            guarded,
            &packet,
            |session_generation, final_packet| {
                assert_eq!(session_generation, identity.session_generation);
                formatted.push(
                    protocol::encode(
                        final_packet,
                        &protocol::BedrockSession { shield_item_id: 0 },
                    )
                    .unwrap()
                    .to_vec(),
                );
                crate::movement::trace_line_if(true, session_generation, final_packet)
            },
        );
        let trace = trace.expect("the final PlayerAuthInput produces one trace line");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&trace).unwrap()["tick"],
            101
        );
        assert_eq!(formatted, [expected]);
    }

    let (ordinary, guarded) = finalize_mining_packet(movement, None);
    let mut ordinary_formats = 0;
    assert_eq!(
        guarded_physics_trace_line_with(Some(identity), guarded, &ordinary, |_, _| {
            ordinary_formats += 1;
            Some("duplicate".to_owned())
        }),
        None
    );
    assert_eq!(ordinary_formats, 0);
}

impl NetworkSession for RecordingSendSession {
    type Error = &'static str;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        future::pending().await
    }

    async fn send_packet(&mut self, packet: protocol::Packet) -> Result<(), Self::Error> {
        let bytes = protocol::encode(&packet, &protocol::BedrockSession { shield_item_id: 0 })
            .unwrap()
            .to_vec();
        self.sent.lock().unwrap().push(bytes);
        Ok(())
    }

    fn decode_error_count(&self) -> u64 {
        0
    }
}

#[tokio::test]
async fn revoked_mining_is_removed_before_write_without_suppressing_its_movement_tick() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let combined = protocol::request_sub_chunk_column(0, 7, 8, -4, 1).unwrap();
    let movement = test_packet();
    let expected = protocol::encode(&movement, &protocol::BedrockSession { shield_item_id: 0 })
        .unwrap()
        .to_vec();
    let (mining_authority, mining_authority_rx) = watch::channel(0);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: combined,
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: None,
            mining: Some(crate::movement::MiningPacketGuard::testing(
                0,
                mining_authority_rx,
                movement,
            )),
        })
        .unwrap();
    mining_authority.send_replace(1);
    let sent = Arc::new(Mutex::new(Vec::new()));
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        RecordingSendSession {
            sent: Arc::clone(&sent),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::PhysicsPacketSent { identity: observed }))
            if observed == identity
    ));
    assert_eq!(sent.lock().unwrap().as_slice(), [expected]);
    shutdown.send_replace(true);
    worker.await.unwrap();
}

#[tokio::test]
async fn current_mining_command_reaches_the_write_byte_exact() {
    let identity = crate::movement::PhysicsSendIdentity {
        session_generation: 7,
        tick: 101,
        admission_id: 3,
        reanchor_epoch: 0,
    };
    let combined = protocol::request_sub_chunk_column(0, 7, 8, -4, 1).unwrap();
    let expected = protocol::encode(&combined, &protocol::BedrockSession { shield_item_id: 0 })
        .unwrap()
        .to_vec();
    let (_mining_authority, mining_authority_rx) = watch::channel(0);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: combined,
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: None,
            mining: Some(crate::movement::MiningPacketGuard::testing(
                0,
                mining_authority_rx,
                test_packet(),
            )),
        })
        .unwrap();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        RecordingSendSession {
            sent: Arc::clone(&sent),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::PhysicsPacketSent { identity: observed }))
            if observed == identity
    ));
    assert_eq!(sent.lock().unwrap().as_slice(), [expected]);
    shutdown.send_replace(true);
    worker.await.unwrap();
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
    let (_mining_authority, mining_authority_rx) = watch::channel(0);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: None,
            physics: Some(identity),
            physics_reanchor: Some(reanchor_rx),
            mining: Some(crate::movement::MiningPacketGuard::testing(
                0,
                mining_authority_rx,
                test_packet(),
            )),
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
            mining: None,
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
            mining: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (started_tx, started_rx) = oneshot::channel();
    let worker = tokio::spawn(run_network_pump(
        PendingSendSession {
            started: Some(started_tx),
            complete: None,
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
async fn reanchor_during_an_in_flight_physics_send_preserves_the_socket_result() {
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
            mining: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (started_tx, started_rx) = oneshot::channel();
    let (complete_tx, complete_rx) = oneshot::channel();
    let worker = tokio::spawn(run_network_pump(
        PendingSendSession {
            started: Some(started_tx),
            complete: Some(complete_rx),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    started_rx.await.unwrap();
    reanchor.send_replace(1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), controls.recv())
            .await
            .is_err(),
        "an in-flight socket write must not be reclassified as cancelled"
    );
    complete_tx.send(()).unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::PhysicsPacketSent {
            identity: observed,
        })) if observed == identity
    ));
    shutdown.send_replace(true);
    worker.await.unwrap();
}
