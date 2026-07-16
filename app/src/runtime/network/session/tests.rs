use std::{
    future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use protocol::{
    ChangeDimensionEvent, MovePlayerEvent, PlayerMovementCorrectionEvent, WorldBootstrap,
    WorldEnvironmentBootstrap, WorldEvent,
};
use tokio::sync::{mpsc, oneshot, watch};

use super::{
    COMMAND_CAPACITY, CONTROL_EVENT_CAPACITY, NetworkCommand, NetworkControlEvent, NetworkHandle,
    NetworkPumpPreference, NetworkPumpWork, NetworkSequencer, NetworkSession, PacketSendError,
    SequencedWorldEvent, WORLD_EVENT_CAPACITY, run_network_pump, send_control_event_or_cancel,
    send_event_or_cancel, send_world_event_or_cancel, wait_for_login_or_cancel,
    wait_for_network_work_or_cancel, wait_for_send_or_cancel,
};

struct ReadyInboundSession {
    inbound: Option<WorldEvent>,
    inbound_selected: Arc<AtomicBool>,
}

impl NetworkSession for ReadyInboundSession {
    type Error = std::convert::Infallible;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        match self.inbound.take() {
            Some(event) => {
                self.inbound_selected.store(true, Ordering::SeqCst);
                Ok(event)
            }
            None => future::pending().await,
        }
    }

    async fn send_packet(&mut self, _packet: protocol::Packet) -> Result<(), Self::Error> {
        Ok(())
    }

    fn decode_error_count(&self) -> u64 {
        0
    }
}

#[test]
fn sequence_is_fifo_and_dimension_changes_apply_to_following_packets() {
    let mut sequencer = NetworkSequencer::new(2, 42);
    let first = sequencer.wrap(WorldEvent::ChunkRadiusUpdated(16));
    assert_eq!(first.sequence, 1);
    assert_eq!(sequencer.current_dimension(), 2);

    let change = sequencer.wrap(WorldEvent::ChangeDimension(ChangeDimensionEvent {
        dimension: 1,
        position: [0.0, 80.0, 0.0],
    }));
    assert_eq!(change.sequence, 2);
    assert_eq!(sequencer.current_dimension(), 1);

    let following = sequencer.wrap(WorldEvent::ChunkRadiusUpdated(8));
    assert_eq!(following.sequence, 3);
}

#[test]
fn foreign_player_movement_is_routed_to_the_actor_stream() {
    let mut sequencer = NetworkSequencer::new(0, 42);
    let movement = |runtime_id| {
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id,
            position: [1.0, 64.0, 2.0],
            pitch: 5.0,
            yaw: 90.0,
        })
    };

    assert!(matches!(
        sequencer.wrap(movement(42)).event,
        WorldEvent::MovePlayer(MovePlayerEvent { runtime_id: 42, .. })
    ));
    assert!(matches!(
        sequencer.wrap(movement(7)).event,
        WorldEvent::Actor(protocol::ActorEvent::Move(protocol::ActorMoveEvent {
            runtime_id: 7,
            dimension: 0,
            ..
        }))
    ));
}

#[test]
fn server_authoritative_correction_bypasses_foreign_player_runtime_filter() {
    let mut sequencer = NetworkSequencer::new(0, 42);
    let correction = WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
        position: [27.5, 111.0, 91.5],
        delta: [0.0; 3],
        pitch: -15.0,
        yaw: 90.0,
        on_ground: true,
        tick: 55,
    });

    assert!(matches!(
        sequencer.wrap(correction).event,
        WorldEvent::PlayerMovementCorrection(_)
    ));
}

#[tokio::test]
async fn saturated_event_queue_is_cancelled_without_waiting_for_capacity() {
    let (events, mut event_rx) = mpsc::channel(1);
    events
        .send(NetworkControlEvent::Stopped {
            decode_error_count: 1,
        })
        .await
        .unwrap();
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    shutdown.send_replace(true);

    let delivered = send_event_or_cancel(
        &events,
        &mut shutdown_rx,
        NetworkControlEvent::Stopped {
            decode_error_count: 2,
        },
    )
    .await;

    assert!(!delivered);
    assert!(matches!(
        event_rx.try_recv(),
        Ok(NetworkControlEvent::Stopped {
            decode_error_count: 1
        })
    ));
    assert!(event_rx.try_recv().is_err());
}

#[tokio::test]
async fn saturated_world_event_channel_does_not_block_request_sent_control_event() {
    assert_eq!(CONTROL_EVENT_CAPACITY, 64);
    let (world_events, mut world_event_rx) = mpsc::channel(1);
    world_events
        .try_send(SequencedWorldEvent {
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
        .unwrap();
    let (control_events, mut control_event_rx) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (_shutdown, mut shutdown_rx) = watch::channel(false);
    let sent_at = Instant::now();

    assert!(
        send_control_event_or_cancel(
            &control_events,
            &mut shutdown_rx,
            NetworkControlEvent::SubChunkRequestSent {
                chunk: world::ChunkKey::new(0, 4, -3),
                base_sub_chunk_y: -4,
                count: 24,
                sent_at,
            },
        )
        .await
    );

    assert!(matches!(
        control_event_rx.try_recv(),
        Ok(NetworkControlEvent::SubChunkRequestSent {
            chunk,
            base_sub_chunk_y: -4,
            count: 24,
            sent_at: observed,
        }) if chunk == world::ChunkKey::new(0, 4, -3) && observed == sent_at
    ));
    assert!(matches!(
        world_event_rx.try_recv(),
        Ok(SequencedWorldEvent {
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
    ));
}

#[tokio::test]
async fn single_worker_acks_ready_command_while_ready_inbound_waits_on_full_world_fifo() {
    let (world_event_tx, world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    for sequence in 1..=WORLD_EVENT_CAPACITY as u64 {
        world_event_tx
            .try_send(SequencedWorldEvent {
                sequence,
                event: WorldEvent::ChunkRadiusUpdated(sequence as i32),
            })
            .unwrap();
    }
    // Model zero main-thread admission by retaining the full receiver without
    // reading from it for the entire assertion window.
    assert_eq!(world_events.len(), WORLD_EVENT_CAPACITY);

    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    for index in 0..COMMAND_CAPACITY {
        commands
            .try_send(NetworkCommand::Send {
                packet: test_packet(),
                sub_chunk: Some(super::SubChunkRequestSend {
                    chunk: world::ChunkKey::new(0, index as i32, 0),
                    base_sub_chunk_y: -4,
                    count: 1,
                }),
            })
            .unwrap();
    }
    assert_eq!(commands.capacity(), 0);

    let (control_event_tx, mut control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let inbound_selected = Arc::new(AtomicBool::new(false));
    let worker = tokio::spawn(run_network_pump(
        ReadyInboundSession {
            inbound: Some(WorldEvent::ChunkRadiusUpdated(99)),
            inbound_selected: Arc::clone(&inbound_selected),
        },
        NetworkSequencer::new(0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    let acknowledgement = tokio::time::timeout(Duration::from_millis(100), control_events.recv())
        .await
        .expect("a ready command must progress while the selected inbound event is backpressured")
        .expect("control channel must remain open");
    assert!(
        inbound_selected.load(Ordering::SeqCst),
        "the inbound-preferred branch must have selected the ready world event first"
    );
    assert!(matches!(
        acknowledgement,
        NetworkControlEvent::SubChunkRequestSent {
            chunk,
            base_sub_chunk_y: -4,
            count: 1,
            ..
        } if chunk == world::ChunkKey::new(0, 0, 0)
    ));
    assert!(commands.capacity() > 0, "the worker must consume a command");
    assert_eq!(
        world_events.len(),
        WORLD_EVENT_CAPACITY,
        "world data must remain undrained at zero admission"
    );

    shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_millis(100), worker)
        .await
        .expect("shutdown must cancel the backpressured worker")
        .unwrap();
}

#[tokio::test]
async fn control_kinds_and_sequenced_world_data_use_only_their_own_channels() {
    let (control_events, mut control_event_rx) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_events, mut world_event_rx) = mpsc::channel(4);
    let (_shutdown, mut shutdown_rx) = watch::channel(false);
    let bootstrap = WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [1.0, 72.0, -2.0],
        world_spawn_position: [1, 64, -2],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    };
    let environment = WorldEnvironmentBootstrap {
        initial_time: 12_000,
        day_cycle_lock_time: 18_000,
        daylight_cycle_enabled: false,
        rain_level: 0.25,
        lightning_level: 0.75,
    };

    for event in [
        NetworkControlEvent::Bootstrap {
            world: bootstrap,
            environment,
        },
        NetworkControlEvent::Failed {
            message: "failure".to_owned(),
            decode_error_count: 7,
        },
        NetworkControlEvent::Stopped {
            decode_error_count: 8,
        },
    ] {
        assert!(send_control_event_or_cancel(&control_events, &mut shutdown_rx, event).await);
    }
    assert!(
        send_world_event_or_cancel(
            &world_events,
            &mut shutdown_rx,
            SequencedWorldEvent {
                sequence: 9,
                event: WorldEvent::ChunkRadiusUpdated(16),
            },
        )
        .await
    );

    assert_eq!(control_event_rx.len(), 3);
    assert_eq!(world_event_rx.len(), 1);
    assert!(matches!(
        control_event_rx.try_recv(),
        Ok(NetworkControlEvent::Bootstrap { world, environment: value })
            if world == bootstrap && value == environment
    ));
    assert!(matches!(
        control_event_rx.try_recv(),
        Ok(NetworkControlEvent::Failed {
            message,
            decode_error_count: 7,
        }) if message == "failure"
    ));
    assert!(matches!(
        control_event_rx.try_recv(),
        Ok(NetworkControlEvent::Stopped {
            decode_error_count: 8,
        })
    ));
    assert!(matches!(
        world_event_rx.try_recv(),
        Ok(SequencedWorldEvent {
            sequence: 9,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
    ));
}

#[tokio::test]
async fn login_wait_observes_shutdown_while_connect_is_pending() {
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    shutdown.send_replace(true);

    let result = wait_for_login_or_cancel(
        future::pending::<Result<(), &'static str>>(),
        &mut shutdown_rx,
    )
    .await;

    assert_eq!(result, None);
}

#[tokio::test]
async fn transport_send_observes_shutdown_after_the_send_is_pending() {
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let (started_tx, started_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        wait_for_send_or_cancel(
            async move {
                let _ = started_tx.send(());
                future::pending::<Result<(), &'static str>>().await
            },
            &mut shutdown_rx,
        )
        .await
    });

    started_rx.await.unwrap();
    shutdown.send_replace(true);
    let result = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("pending transport send should be cancelled")
        .unwrap();

    assert_eq!(result, None);
}

#[tokio::test]
async fn network_pump_round_robins_repeated_ready_work_and_preserves_command_fifo() {
    let (_shutdown, mut shutdown_rx) = watch::channel(false);
    let (commands, mut command_rx) = mpsc::channel(4);
    let mut preference = NetworkPumpPreference::Inbound;
    commands.try_send(10).unwrap();
    commands.try_send(20).unwrap();

    let first = wait_for_network_work_or_cancel(
        future::ready("inbound-1"),
        command_rx.recv(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(first, NetworkPumpWork::Inbound("inbound-1")));
    assert_eq!(command_rx.len(), 2);

    let second = wait_for_network_work_or_cancel(
        future::ready("inbound-2"),
        command_rx.recv(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(second, NetworkPumpWork::Command(Some(10))));

    let third = wait_for_network_work_or_cancel(
        future::ready("inbound-3"),
        command_rx.recv(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(third, NetworkPumpWork::Inbound("inbound-3")));

    let fourth = wait_for_network_work_or_cancel(
        future::ready("inbound-4"),
        command_rx.recv(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(fourth, NetworkPumpWork::Command(Some(20))));

    commands.try_send(30).unwrap();
    let inbound_pending = wait_for_network_work_or_cancel(
        future::pending::<&'static str>(),
        command_rx.recv(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(
        inbound_pending,
        NetworkPumpWork::Command(Some(30))
    ));

    commands.try_send(40).unwrap();
    let fifth = wait_for_network_work_or_cancel(
        future::ready("inbound-5"),
        command_rx.recv(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(fifth, NetworkPumpWork::Inbound("inbound-5")));

    let command_pending = wait_for_network_work_or_cancel(
        future::ready("inbound-6"),
        future::pending::<Option<i32>>(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(
        command_pending,
        NetworkPumpWork::Inbound("inbound-6")
    ));

    let final_command = wait_for_network_work_or_cancel(
        future::pending::<&'static str>(),
        command_rx.recv(),
        &mut shutdown_rx,
        &mut preference,
    )
    .await;
    assert!(matches!(final_command, NetworkPumpWork::Command(Some(40))));
}

#[test]
fn saturated_command_queue_preserves_packet_and_shutdown_does_not_join_on_ui_thread() {
    let (commands, _command_rx) = mpsc::channel(COMMAND_CAPACITY);
    for _ in 0..COMMAND_CAPACITY {
        commands
            .try_send(NetworkCommand::Send {
                packet: test_packet(),
                sub_chunk: None,
            })
            .unwrap();
    }
    let (control_event_tx, control_events) = mpsc::channel(1);
    let (world_event_tx, world_events) = mpsc::channel(1);
    drop(control_event_tx);
    drop(world_event_tx);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    let worker = thread::spawn(|| thread::sleep(Duration::from_millis(250)));
    let mut handle = NetworkHandle {
        control_events,
        world_events,
        commands,
        shutdown,
        thread: Some(worker),
    };

    let packet = test_packet();
    let error = handle.send_packet(packet).unwrap_err();
    assert!(matches!(error, PacketSendError::Full(_)));
    let started = Instant::now();
    handle.shutdown();

    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(*handle.shutdown.borrow());
}

#[test]
fn network_pending_counts_include_ingress_and_outbound_queues() {
    let (control_event_tx, control_events) = mpsc::channel(2);
    let (world_event_tx, world_events) = mpsc::channel(2);
    let (commands, mut command_rx) = mpsc::channel(2);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    let mut handle = NetworkHandle {
        control_events,
        world_events,
        commands,
        shutdown,
        thread: None,
    };

    assert_eq!(handle.pending_event_count(), 0);
    assert_eq!(handle.pending_command_count(), 0);
    control_event_tx
        .try_send(NetworkControlEvent::Stopped {
            decode_error_count: 0,
        })
        .unwrap();
    assert_eq!(handle.pending_event_count(), 1);
    world_event_tx
        .try_send(SequencedWorldEvent {
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
        .unwrap();
    assert_eq!(handle.pending_event_count(), 2);
    handle.control_events_mut().try_recv().unwrap();
    handle.world_events_mut().try_recv().unwrap();
    assert_eq!(handle.pending_event_count(), 0);

    handle.send_packet(test_packet()).unwrap();
    assert_eq!(handle.pending_command_count(), 1);
    command_rx.try_recv().unwrap();
    assert_eq!(handle.pending_command_count(), 0);
}

fn test_packet() -> protocol::Packet {
    protocol::request_sub_chunk_column(0, 0, 0, -4, 1).unwrap()
}
