//! Outbound batching, receipts, transfer barriers and the split write path.

use super::*;

/// Yields one event, then a second only once the outbound batch has been written.
struct GatedTransferSession {
    before_write: Option<WorldEvent>,
    after_write: Option<oneshot::Receiver<()>>,
    written: Option<oneshot::Sender<()>>,
    rotations: Arc<AtomicUsize>,
}

impl NetworkSession for GatedTransferSession {
    type Error = &'static str;
    type Outbound = PacketOutbound<&'static str>;

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        let mut written = self.written.take();
        Ok(PacketOutbound::new(move |_| {
            if let Some(written) = written.take() {
                let _ = written.send(());
            }
            future::ready(Ok(()))
        }))
    }

    async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
        if let Some(event) = self.before_write.take() {
            return Ok(event);
        }
        if let Some(after_write) = self.after_write.as_mut() {
            let _ = after_write.await;
            self.after_write = None;
            return Ok(WorldEvent::ChunkRadiusUpdated(8));
        }
        future::pending().await
    }

    fn decode_error_count(&self) -> u64 {
        0
    }

    fn arm_blob_cache_reset_for_fast_transfer(&mut self) {
        self.rotations.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn successful_fast_transfer_flushes_decoded_pending_ingress_then_enqueues_marker() {
    let (world_event_tx, mut world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: Some(super::super::ChatPacketSend {
                session: 7,
                sequence: 11,
                fast_transfer_action: Some(protocol::FastTransferAction::TransferSm3),
            }),
            physics: None,
            physics_reanchor: None,
            interaction: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let rotations = Arc::new(AtomicUsize::new(0));
    let (written, after_write) = oneshot::channel();
    commands.try_send(NetworkCommand::FlushFrame).unwrap();
    let worker = tokio::spawn(run_network_pump(
        GatedTransferSession {
            before_write: Some(WorldEvent::ChunkRadiusUpdated(16)),
            after_write: Some(after_write),
            written: Some(written),
            rotations: Arc::clone(&rotations),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), world_events.recv()).await,
        Ok(Some(WorldIngress::Event(SequencedWorldEvent {
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })))
    ));
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), world_events.recv()).await,
        Ok(Some(WorldIngress::FastTransferBarrier {
            session_generation: 7,
            sequence: 2,
            action_sequence: 11,
        }))
    ));
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), world_events.recv()).await,
        Ok(Some(WorldIngress::Event(SequencedWorldEvent {
            session_generation: 7,
            sequence: 3,
            event: WorldEvent::ChunkRadiusUpdated(8),
        })))
    ));
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::ChatPacketSent {
            session: 7,
            sequence: 11,
        }))
    ));
    assert_eq!(rotations.load(Ordering::SeqCst), 1);
    shutdown.send_replace(true);
    worker.await.unwrap();
}

#[tokio::test]
async fn failed_fast_transfer_never_arms_a_reset() {
    let (world_event_tx, mut world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: Some(super::super::ChatPacketSend {
                session: 8,
                sequence: 12,
                fast_transfer_action: Some(protocol::FastTransferAction::TransferSm3),
            }),
            physics: None,
            physics_reanchor: None,
            interaction: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);

    commands.try_send(NetworkCommand::FlushFrame).unwrap();
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
    assert!(world_events.try_recv().is_err());
    assert!(events.iter().any(|event| matches!(
        event,
        NetworkControlEvent::ChatPacketSendFailed {
            session: 8,
            sequence: 12,
            ..
        }
    )));
}

#[tokio::test]
async fn successful_non_transfer_chat_does_not_arm_blob_rotation() {
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: Some(super::super::ChatPacketSend {
                session: 7,
                sequence: 11,
                fast_transfer_action: None,
            }),
            physics: None,
            physics_reanchor: None,
            interaction: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let rotations = Arc::new(AtomicUsize::new(0));
    commands.try_send(NetworkCommand::FlushFrame).unwrap();
    let worker = tokio::spawn(run_network_pump(
        QueuedInboundSession {
            inbound: VecDeque::new(),
            rotations: Arc::clone(&rotations),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), controls.recv()).await,
        Ok(Some(NetworkControlEvent::ChatPacketSent {
            session: 7,
            sequence: 11,
        }))
    ));
    assert_eq!(rotations.load(Ordering::SeqCst), 0);
    shutdown.send_replace(true);
    worker.await.unwrap();
}

#[tokio::test]
async fn chat_send_failure_identifies_the_exact_outbox_item() {
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: Some(super::super::ChatPacketSend {
                session: 8,
                sequence: 12,
                fast_transfer_action: None,
            }),
            physics: None,
            physics_reanchor: None,
            interaction: None,
        })
        .unwrap();
    let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);
    commands.try_send(NetworkCommand::FlushFrame).unwrap();
    run_network_pump(
        FailingSendSession,
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    )
    .await;

    assert!(matches!(
        controls.recv().await,
        Some(NetworkControlEvent::ChatPacketSendFailed {
            session: 8,
            sequence: 12,
            ref message,
        }) if message == "socket write failed"
    ));
    assert!(matches!(
        controls.recv().await,
        Some(NetworkControlEvent::Failed { .. })
    ));
}

#[tokio::test]
async fn fast_transfer_trace_arms_before_send_and_cancels_after_send_failure() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: Some(super::super::ChatPacketSend {
                session: 8,
                sequence: 12,
                fast_transfer_action: Some(protocol::FastTransferAction::TransferSm3),
            }),
            physics: None,
            physics_reanchor: None,
            interaction: None,
        })
        .unwrap();
    let (control_event_tx, _controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, shutdown_rx) = watch::channel(false);

    commands.try_send(NetworkCommand::FlushFrame).unwrap();
    run_network_pump(
        TraceOrderingFailSession {
            calls: Arc::clone(&calls),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    )
    .await;

    assert_eq!(*calls.lock().unwrap(), ["begin", "send", "cancel"]);
}

#[tokio::test]
async fn ready_outbound_hook_is_handled_before_ready_inbound_work() {
    let (_shutdown, mut shutdown_rx) = watch::channel(false);

    let work = wait_for_network_work_or_cancel(
        future::ready("inbound"),
        future::ready("hook"),
        &mut shutdown_rx,
    )
    .await;

    assert!(matches!(work, NetworkPumpWork::Hook("hook")));
}

/// Movement, chat and sub-chunk receipts wait for the batch's write, and a failed write
/// publishes none of them.
#[tokio::test]
async fn receipts_wait_for_a_blocked_write_and_never_follow_a_failed_one() {
    struct BlockedWriteSession(Option<oneshot::Receiver<Result<(), &'static str>>>);
    impl NetworkSession for BlockedWriteSession {
        type Error = &'static str;
        type Outbound = PacketOutbound<&'static str>;

        fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
            let mut outcome = self.0.take();
            Ok(PacketOutbound::new(move |_| {
                let outcome = outcome.take();
                async move {
                    match outcome {
                        Some(outcome) => outcome.await.unwrap_or(Err("write abandoned")),
                        None => Ok(()),
                    }
                }
            }))
        }

        async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
            future::pending().await
        }

        fn decode_error_count(&self) -> u64 {
            0
        }
    }

    for outcome in [Ok(()), Err("socket write failed")] {
        let identity = protocol::PhysicsSendIdentity {
            session_generation: 7,
            tick: 101,
            admission_id: 3,
            reanchor_epoch: 0,
        };
        let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        for (physics, chat, sub_chunk) in [
            (Some(identity), None, None),
            (
                None,
                Some(super::super::ChatPacketSend {
                    session: 7,
                    sequence: 11,
                    fast_transfer_action: None,
                }),
                None,
            ),
            (
                None,
                None,
                Some(super::super::SubChunkRequestSend {
                    chunk: world::ChunkKey::new(0, 1, 2),
                    base_sub_chunk_y: -4,
                    count: 1,
                }),
            ),
        ] {
            commands
                .try_send(NetworkCommand::Send {
                    packet: test_packet(),
                    sub_chunk,
                    chat,
                    physics,
                    physics_reanchor: None,
                    interaction: None,
                })
                .unwrap();
        }
        commands.try_send(NetworkCommand::FlushFrame).unwrap();
        let (finish_write, write_outcome) = oneshot::channel();
        let (control_event_tx, mut controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
        let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(run_network_pump(
            BlockedWriteSession(Some(write_outcome)),
            NetworkSequencer::new(7, 0, 42),
            command_rx,
            control_event_tx,
            world_event_tx,
            shutdown_rx,
        ));
        for _ in 0..64 {
            tokio::task::yield_now().await;
        }
        assert!(
            controls.try_recv().is_err(),
            "nothing may be published while the write is blocked"
        );

        let failed = outcome.is_err();
        finish_write.send(outcome).unwrap();
        let mut events = Vec::new();
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_secs(5), controls.recv()).await
        {
            let terminal = matches!(event, NetworkControlEvent::Failed { .. });
            events.push(event);
            if terminal || events.len() == 3 {
                break;
            }
        }
        let receipts = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    NetworkControlEvent::PhysicsPacketSent { .. }
                        | NetworkControlEvent::ChatPacketSent { .. }
                        | NetworkControlEvent::SubChunkRequestSent { .. }
                )
            })
            .count();
        assert_eq!(receipts, if failed { 0 } else { 3 });
        shutdown.send_replace(true);
        worker.await.unwrap();
    }
}

/// The transfer barrier, and every inbound event after it, waits for the transfer write.
#[tokio::test]
async fn transfer_barrier_waits_for_its_write_to_complete() {
    struct HeldTransferSession {
        inbound: VecDeque<WorldEvent>,
        write_gate: Option<oneshot::Receiver<()>>,
    }
    impl NetworkSession for HeldTransferSession {
        type Error = &'static str;
        type Outbound = PacketOutbound<&'static str>;

        fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
            let mut gate = self.write_gate.take();
            Ok(PacketOutbound::new(move |_| {
                let gate = gate.take();
                async move {
                    if let Some(gate) = gate {
                        let _ = gate.await;
                    }
                    Ok(())
                }
            }))
        }

        async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
            match self.inbound.pop_front() {
                Some(event) => Ok(event),
                None => future::pending().await,
            }
        }

        fn decode_error_count(&self) -> u64 {
            0
        }
    }

    let (world_event_tx, mut world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: Some(super::super::ChatPacketSend {
                session: 7,
                sequence: 11,
                fast_transfer_action: Some(protocol::FastTransferAction::TransferSm3),
            }),
            physics: None,
            physics_reanchor: None,
            interaction: None,
        })
        .unwrap();
    commands.try_send(NetworkCommand::FlushFrame).unwrap();
    let (open_write, write_gate) = oneshot::channel();
    let (control_event_tx, _controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        HeldTransferSession {
            inbound: VecDeque::from([WorldEvent::ChunkRadiusUpdated(8)]),
            write_gate: Some(write_gate),
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));
    let mut before_write = Vec::new();
    for _ in 0..64 {
        tokio::task::yield_now().await;
        while let Ok(event) = world_events.try_recv() {
            before_write.push(event);
        }
    }
    assert!(
        !before_write
            .iter()
            .any(|event| matches!(event, WorldIngress::FastTransferBarrier { .. })),
        "the barrier must not land before its write completes"
    );

    open_write.send(()).unwrap();
    let mut events = before_write;
    while !events
        .iter()
        .any(|event| matches!(event, WorldIngress::FastTransferBarrier { .. }))
        || events.len() < 2
    {
        events.push(
            tokio::time::timeout(Duration::from_secs(5), world_events.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    let barrier = events
        .iter()
        .position(|event| matches!(event, WorldIngress::FastTransferBarrier { .. }))
        .unwrap();
    // Whatever was decoded before the trace armed precedes the barrier; nothing decoded while
    // the write was pending may slip ahead of it.
    assert!(events[..barrier].len() <= 1);
    shutdown.send_replace(true);
    worker.await.unwrap();
}

/// Packets queued in one frame leave as one batch, in queue order, and an empty frame sends
/// nothing.
#[tokio::test]
async fn each_frame_leaves_as_one_ordered_batch_and_an_empty_frame_sends_nothing() {
    struct BatchSession(Arc<Mutex<Vec<Vec<Vec<u8>>>>>);
    struct BatchOutbound(Arc<Mutex<Vec<Vec<Vec<u8>>>>>);
    impl OutboundSession for BatchOutbound {
        type Error = &'static str;

        async fn send_batch(&mut self, packets: Vec<protocol::Packet>) -> Result<(), Self::Error> {
            let session = protocol::BedrockSession { shield_item_id: 0 };
            let batch = packets
                .iter()
                .map(|packet| protocol::encode(packet, &session).unwrap().to_vec())
                .collect();
            self.0.lock().unwrap().push(batch);
            Ok(())
        }
    }
    impl NetworkSession for BatchSession {
        type Error = &'static str;
        type Outbound = BatchOutbound;

        fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
            Ok(BatchOutbound(Arc::clone(&self.0)))
        }

        async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
            future::pending().await
        }

        fn decode_error_count(&self) -> u64 {
            0
        }
    }

    let (mut handle, _) = NetworkHandle::stub();
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    handle.commands = commands;
    let frames = [
        vec![
            protocol::modal_form_cancel_response(1),
            protocol::modal_form_cancel_response(2),
            protocol::modal_form_cancel_response(3),
        ],
        vec![protocol::modal_form_cancel_response(4)],
    ];
    for frame in &frames {
        for packet in frame {
            handle.send_hotbar_packet(packet.clone()).unwrap();
        }
        handle.flush_frame();
        handle.flush_frame();
    }
    let queued = handle.pending_command_count();
    let batches = Arc::new(Mutex::new(Vec::new()));
    let (control_event_tx, _controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        BatchSession(Arc::clone(&batches)),
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        while handle.pending_command_count() > 0 || batches.lock().unwrap().len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both frames are written");
    shutdown.send_replace(true);
    worker.await.unwrap();

    let session = protocol::BedrockSession { shield_item_id: 0 };
    let expected = frames
        .iter()
        .map(|frame| {
            frame
                .iter()
                .map(|packet| protocol::encode(packet, &session).unwrap().to_vec())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(*batches.lock().unwrap(), expected);
    assert_eq!(queued, 6, "an empty frame must not queue a second flush");
}

/// An outbound command queued while the inbound side is inside a decode is written before
/// that decode finishes.
#[test]
fn command_queued_during_a_blocked_inbound_decode_is_written_before_the_decode_ends() {
    struct DecodingSession {
        started: std::sync::mpsc::Sender<()>,
        gate: std::sync::mpsc::Receiver<()>,
        written: std::sync::mpsc::Sender<()>,
    }
    impl NetworkSession for DecodingSession {
        type Error = &'static str;
        type Outbound = PacketOutbound<&'static str>;

        fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
            let written = self.written.clone();
            Ok(PacketOutbound::new(move |_| {
                let _ = written.send(());
                future::ready(Ok(()))
            }))
        }

        async fn receive_world_event(&mut self, _: i32) -> Result<WorldEvent, Self::Error> {
            // A CPU-bound decode holds its thread until it returns.
            let _ = self.started.send(());
            let _ = self.gate.recv();
            future::pending().await
        }

        fn decode_error_count(&self) -> u64 {
            0
        }
    }

    let (started_tx, started) = std::sync::mpsc::channel();
    let (gate, gate_rx) = std::sync::mpsc::channel();
    let (written_tx, written) = std::sync::mpsc::channel();
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (control_event_tx, _controls) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let driver = thread::spawn(move || {
        started.recv().unwrap();
        commands
            .try_send(NetworkCommand::Send {
                packet: test_packet(),
                sub_chunk: None,
                chat: None,
                physics: None,
                physics_reanchor: None,
                interaction: None,
            })
            .unwrap();
        commands.try_send(NetworkCommand::FlushFrame).unwrap();
        let written_during_decode = written.recv_timeout(Duration::from_secs(5)).is_ok();
        shutdown.send_replace(true);
        gate.send(()).unwrap();
        written_during_decode
    });
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(run_network_pump(
        DecodingSession {
            started: started_tx,
            gate: gate_rx,
            written: written_tx,
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    assert!(
        driver.join().unwrap(),
        "the batch must be written while the inbound decode still holds the pump"
    );
}
