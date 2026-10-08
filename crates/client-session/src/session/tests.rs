use std::{
    collections::VecDeque,
    future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
type NetworkControlEvent = super::NetworkControlEvent<()>;
type NetworkHandle = super::NetworkHandle<()>;

#[path = "alloc_count.rs"]
pub(super) mod alloc_count;
mod forms;
mod queues;

use protocol::{
    ActorPositionOrigin, BlobCacheStats, ChangeDimensionEvent, InventoryAuthority, InventoryEvent,
    MovePlayerEvent, PLAYER_NETWORK_OFFSET, PlayerGameMode, PlayerMovementCorrectionEvent,
    SetTimeEvent, WorldBootstrap, WorldEnvironmentBootstrap, WorldEvent,
};
use tokio::sync::{mpsc, oneshot, watch};

use super::{
    COMMAND_CAPACITY, CONTROL_EVENT_CAPACITY, NetworkCommand, NetworkFailureOrigin,
    NetworkPumpWork, NetworkSequencer, NetworkSession, OutboundSession, PacketSendError,
    ReadinessIngressCounter, SequencedWorldEvent, SessionTransferTarget, WORLD_EVENT_CAPACITY,
    WorldIngress, bounded_counter_log_due, run_network_pump, run_network_pump_with_trace,
    send_control_event_or_cancel, send_event_or_cancel, send_final_blob_cache_telemetry,
    send_world_event_or_cancel, session_failure_display, start_game_inventory_authority,
    start_game_item_registry, wait_for_login_or_cancel, wait_for_network_work_or_cancel,
    wait_for_send_or_cancel, wrap_readiness_tracked_event, write_network_pump_terminal_marker,
    write_network_pump_transfer_marker,
};

#[path = "close_race_tests.rs"]
mod close_race_tests;
#[path = "disconnect_tests.rs"]
mod disconnect_tests;
#[path = "physics_send_tests.rs"]
mod physics_send_tests;
#[path = "routing_tests.rs"]
mod routing_tests;
#[path = "transfer_tests.rs"]
mod transfer_tests;

fn test_packet() -> protocol::Packet {
    protocol::request_sub_chunk_column(0, 0, 0, -4, 1).unwrap()
}

#[test]
fn readiness_ingress_counter_excludes_transport_only_events() {
    let counter = ReadinessIngressCounter::default();
    let transport_only = WorldEvent::SetTime(SetTimeEvent { time: 6_000 });
    counter.record_produced(&transport_only);
    assert_eq!(counter.pending(), 0);

    let readiness_event = WorldEvent::ChunkRadiusUpdated(16);
    counter.record_produced(&readiness_event);
    assert_eq!(counter.pending(), 1);
    counter.record_consumed(&readiness_event);
    assert_eq!(counter.pending(), 0);

    let mut sequencer = NetworkSequencer::new(7, 0, 42);
    let remote_move = wrap_readiness_tracked_event(
        &mut sequencer,
        &counter,
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 99,
            ..Default::default()
        }),
    );
    assert!(matches!(remote_move.event, WorldEvent::Actor(_)));
    assert_eq!(
        counter.pending(),
        0,
        "remote movement is normalized to actor presentation before classification"
    );

    let local_move = wrap_readiness_tracked_event(
        &mut sequencer,
        &counter,
        WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 42,
            ..Default::default()
        }),
    );
    assert!(matches!(local_move.event, WorldEvent::MovePlayer(_)));
    assert_eq!(counter.pending(), 1);
    counter.record_consumed(&local_move.event);
    assert_eq!(counter.pending(), 0);
}
#[test]
fn blob_cache_semantic_warning_schedule_is_logarithmically_bounded() {
    assert!(bounded_counter_log_due(0, 1));
    assert!(bounded_counter_log_due(1, 2));
    assert!(!bounded_counter_log_due(2, 3));
    assert!(bounded_counter_log_due(3, 4));
    assert!(!bounded_counter_log_due(4, 7));
    assert!(bounded_counter_log_due(7, 8));
    assert!(bounded_counter_log_due(8, 17));
    assert!(!bounded_counter_log_due(17, 17));
    assert!(!bounded_counter_log_due(17, 16));
}

#[test]
fn blob_cache_log_line_exposes_pressure_and_recovery_counters() {
    let path = std::env::temp_dir().join(format!(
        "cinnabar-blob-cache-log-{}-{:?}.txt",
        std::process::id(),
        std::thread::current().id(),
    ));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(std::fs::File::create(&path).unwrap())
        .finish();
    let stats = BlobCacheStats {
        retained_cached_transactions: 101,
        ordinary_ready_events: 102,
        ordinary_ready_bytes: 103,
        recovery_ready_events: 104,
        recovery_ready_bytes: 105,
        redundant_missing_requests: 106,
        abandoned_cached_transactions: 107,
        recovery_requests: 108,
        ordinary_backpressure: 109,
        cached_packet_transaction_pressure: 110,
        cached_packet_pending_pressure: 111,
        cached_packet_staged_pressure: 112,
        cached_packet_reconstruction_pressure: 113,
        cached_packet_ready_pressure: 114,
        ..Default::default()
    };
    tracing::subscriber::with_default(subscriber, || {
        // Another test may have cached this callsite as uninteresting process-wide.
        tracing::callsite::rebuild_interest_cache();
        super::blob_cache_telemetry::emit_blob_cache_telemetry(stats);
    });
    let logged = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let counters = logged
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .filter_map(|(name, value)| value.parse::<u64>().ok().map(|value| (name, value)))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (name, expected) in [
        ("retained_cached_transactions", 101),
        ("ordinary_ready_events", 102),
        ("ordinary_ready_bytes", 103),
        ("recovery_ready_events", 104),
        ("recovery_ready_bytes", 105),
        ("redundant_missing_requests", 106),
        ("abandoned_cached_transactions", 107),
        ("recovery_requests", 108),
        ("ordinary_backpressure", 109),
        ("cached_packet_transaction_pressure", 110),
        ("cached_packet_pending_pressure", 111),
        ("cached_packet_staged_pressure", 112),
        ("cached_packet_reconstruction_pressure", 113),
        ("cached_packet_ready_pressure", 114),
    ] {
        assert_eq!(counters.get(name), Some(&expected), "logged counter {name}");
    }
}

#[test]
fn network_pump_terminal_marker_carries_the_unmasked_error() {
    let mut output = Vec::new();
    write_network_pump_terminal_marker(
        &mut output,
        "receive",
        "socket read failed: \"peer reset\"",
        7,
        None,
    );
    let line = String::from_utf8(output).expect("marker is UTF-8");
    let payload = line
        .trim()
        .strip_prefix("RUST_MCBE_NETWORK_PUMP_TERMINAL=")
        .expect("durable marker prefix");
    let marker: serde_json::Value = serde_json::from_str(payload).expect("marker JSON");

    assert_eq!(marker["schema"], "rust-mcbe-network-pump-terminal-v1");
    assert_eq!(marker["outcome"], "failed");
    assert_eq!(marker["stage"], "receive");
    assert_eq!(marker["message"], "socket read failed: \"peer reset\"");
    assert_eq!(marker["decode_error_count"], 7);
}

type WritePacket<E> = Box<
    dyn FnMut(protocol::Packet) -> std::pin::Pin<Box<dyn Future<Output = Result<(), E>> + Send>>
        + Send,
>;

/// A test write half that hands each packet of a batch, in order, to one callback.
pub(super) struct PacketOutbound<E> {
    write: WritePacket<E>,
    finish_loading: Vec<protocol::Packet>,
}

impl<E: Send + 'static> PacketOutbound<E> {
    pub(super) fn new<F, W>(mut write: W) -> Self
    where
        W: FnMut(protocol::Packet) -> F + Send + 'static,
        F: Future<Output = Result<(), E>> + Send + 'static,
    {
        Self {
            write: Box::new(move |packet| Box::pin(write(packet))),
            finish_loading: Vec::new(),
        }
    }

    pub(super) fn accepting() -> Self {
        Self::new(|_| future::ready(Ok(())))
    }

    pub(super) fn with_finish_loading(mut self, packets: Vec<protocol::Packet>) -> Self {
        self.finish_loading = packets;
        self
    }
}

impl<E: std::fmt::Display + Send + 'static> OutboundSession for PacketOutbound<E> {
    type Error = E;

    async fn send_batch(&mut self, packets: Vec<protocol::Packet>) -> Result<(), E> {
        for packet in packets {
            (self.write)(packet).await?;
        }
        Ok(())
    }

    fn take_finish_loading(&mut self) -> Vec<protocol::Packet> {
        std::mem::take(&mut self.finish_loading)
    }
}

struct ReadyInboundSession {
    inbound: Option<WorldEvent>,
    inbound_selected: Arc<AtomicBool>,
}

struct CachedInboundSession {
    inbound: Option<WorldEvent>,
    stats: BlobCacheStats,
}

struct QueuedInboundSession {
    inbound: VecDeque<WorldEvent>,
    rotations: Arc<AtomicUsize>,
}

struct FailingSendSession;

struct TraceOrderingFailSession {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl NetworkSession for TraceOrderingFailSession {
    type Error = &'static str;
    type Outbound = PacketOutbound<&'static str>;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        future::pending().await
    }

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        let calls = Arc::clone(&self.calls);
        Ok(PacketOutbound::new(move |_| {
            calls.lock().unwrap().push("send");
            future::ready(Err("socket write failed"))
        }))
    }

    fn decode_error_count(&self) -> u64 {
        0
    }

    fn begin_packet_id_trace(&mut self) {
        self.calls.lock().unwrap().push("begin");
    }

    fn cancel_packet_id_trace(&mut self) {
        self.calls.lock().unwrap().push("cancel");
    }

    fn arm_blob_cache_reset_for_fast_transfer(&mut self) {
        self.calls.lock().unwrap().push("rotate");
    }
}

impl NetworkSession for FailingSendSession {
    type Error = &'static str;
    type Outbound = PacketOutbound<&'static str>;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        future::pending().await
    }

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        Ok(PacketOutbound::new(|_| {
            future::ready(Err("socket write failed"))
        }))
    }

    fn decode_error_count(&self) -> u64 {
        0
    }
}

impl NetworkSession for CachedInboundSession {
    type Error = std::convert::Infallible;
    type Outbound = PacketOutbound<&'static str>;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        match self.inbound.take() {
            Some(event) => {
                self.stats.reconstructed_level_chunks += 1;
                Ok(event)
            }
            None => future::pending().await,
        }
    }

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        Ok(PacketOutbound::accepting())
    }

    fn decode_error_count(&self) -> u64 {
        0
    }

    fn blob_cache_enabled(&self) -> bool {
        true
    }

    fn blob_cache_stats(&self) -> BlobCacheStats {
        self.stats
    }
}

impl NetworkSession for ReadyInboundSession {
    type Error = std::convert::Infallible;
    type Outbound = PacketOutbound<&'static str>;

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

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        Ok(PacketOutbound::accepting())
    }

    fn decode_error_count(&self) -> u64 {
        0
    }
}

impl NetworkSession for QueuedInboundSession {
    type Error = std::convert::Infallible;
    type Outbound = PacketOutbound<&'static str>;

    async fn receive_world_event(
        &mut self,
        _current_dimension: i32,
    ) -> Result<WorldEvent, Self::Error> {
        match self.inbound.pop_front() {
            Some(event) => Ok(event),
            None => future::pending().await,
        }
    }

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        Ok(PacketOutbound::accepting())
    }

    fn decode_error_count(&self) -> u64 {
        0
    }

    fn arm_blob_cache_reset_for_fast_transfer(&mut self) {
        self.rotations.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn cache_stats_are_forwarded_after_cached_world_ingress() {
    let initial_stats = BlobCacheStats {
        hashes_classified: 7,
        hits: 3,
        misses: 4,
        admitted_blobs: 4,
        ..BlobCacheStats::default()
    };
    let updated_stats = BlobCacheStats {
        reconstructed_level_chunks: 1,
        ..initial_stats
    };
    let (control_event_tx, mut control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (_commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        CachedInboundSession {
            inbound: Some(WorldEvent::ChunkRadiusUpdated(16)),
            stats: initial_stats,
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        control_event_tx,
        world_event_tx,
        shutdown_rx,
    ));

    let initial = tokio::time::timeout(Duration::from_millis(100), control_events.recv())
        .await
        .expect("initial cache telemetry must be forwarded promptly")
        .expect("control channel must remain open");
    assert!(matches!(
        initial,
        NetworkControlEvent::BlobCacheTelemetry {
            enabled: true,
            stats: observed,
        } if observed == initial_stats
    ));
    let updated = tokio::time::timeout(Duration::from_millis(100), control_events.recv())
        .await
        .expect("updated cache telemetry must follow cached ingress")
        .expect("control channel must remain open");
    assert!(matches!(
        updated,
        NetworkControlEvent::BlobCacheTelemetry {
            enabled: true,
            stats: observed,
        } if observed == updated_stats
    ));

    shutdown.send_replace(true);
    tokio::time::timeout(Duration::from_millis(100), worker)
        .await
        .expect("shutdown must stop the worker")
        .unwrap();
}

#[tokio::test]
async fn final_cache_telemetry_flushes_after_shutdown_is_already_set() {
    let stats = BlobCacheStats {
        hashes_classified: 5,
        hits: 2,
        misses: 3,
        admitted_blobs: 3,
        ..BlobCacheStats::default()
    };
    let (events, mut event_rx) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_events, _world_event_rx) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (_commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(run_network_pump(
        CachedInboundSession {
            inbound: None,
            stats,
        },
        NetworkSequencer::new(7, 0, 42),
        command_rx,
        events,
        world_events,
        shutdown_rx,
    ));
    let initial = event_rx.recv().await.expect("initial cache telemetry");
    assert!(matches!(
        initial,
        NetworkControlEvent::BlobCacheTelemetry { stats: observed, .. } if observed == stats
    ));

    shutdown.send_replace(true);

    let final_event = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
        .await
        .expect("final cache telemetry must ignore shutdown cancellation")
        .expect("control channel must retain final cache telemetry");
    assert!(matches!(
        final_event,
        NetworkControlEvent::BlobCacheTelemetry {
            enabled: true,
            stats: observed,
        } if observed == stats
    ));
    worker.await.expect("network pump stops after final flush");
}

#[tokio::test]
async fn final_cache_telemetry_flush_is_bounded_when_control_queue_stays_full() {
    let session = CachedInboundSession {
        inbound: None,
        stats: BlobCacheStats::default(),
    };
    let (events, _event_rx) = mpsc::channel(1);
    events
        .send(NetworkControlEvent::Stopped {
            decode_error_count: 0,
        })
        .await
        .expect("fill control queue");
    let delivered = tokio::time::timeout(
        Duration::from_secs(1),
        send_final_blob_cache_telemetry(&session, &events),
    )
    .await
    .expect("final telemetry flush must have a fixed deadline");

    assert!(!delivered);
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
    let (world_events, mut world_event_rx) = mpsc::channel(1);
    world_events
        .try_send(SequencedWorldEvent {
            session_generation: 7,
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
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
    ));
}

#[tokio::test]
async fn chat_send_receipt_is_emitted_only_after_the_session_send_completes() {
    let (world_event_tx, _world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    commands
        .try_send(NetworkCommand::Send {
            packet: test_packet(),
            sub_chunk: None,
            chat: Some(super::ChatPacketSend {
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
    commands.try_send(NetworkCommand::FlushFrame).unwrap();
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
        Ok(Some(NetworkControlEvent::ChatPacketSent {
            session: 7,
            sequence: 11,
        }))
    ));
    shutdown.send_replace(true);
    worker.await.unwrap();
}

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
            chat: Some(super::ChatPacketSend {
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
            chat: Some(super::ChatPacketSend {
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
            chat: Some(super::ChatPacketSend {
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
            chat: Some(super::ChatPacketSend {
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
            chat: Some(super::ChatPacketSend {
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
async fn single_worker_acks_ready_command_while_ready_inbound_waits_on_full_world_fifo() {
    let (world_event_tx, world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    for sequence in 1..=WORLD_EVENT_CAPACITY as u64 {
        world_event_tx
            .try_send(WorldIngress::Event(SequencedWorldEvent {
                session_generation: 7,
                sequence,
                event: WorldEvent::ChunkRadiusUpdated(sequence as i32),
            }))
            .unwrap();
    }
    // Model zero main-thread admission by retaining the full receiver without
    // reading from it for the entire assertion window.
    assert_eq!(world_events.len(), WORLD_EVENT_CAPACITY);

    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    for index in 0..COMMAND_CAPACITY - 1 {
        commands
            .try_send(NetworkCommand::Send {
                packet: test_packet(),
                sub_chunk: Some(super::SubChunkRequestSend {
                    chunk: world::ChunkKey::new(0, index as i32, 0),
                    base_sub_chunk_y: -4,
                    count: 1,
                }),
                chat: None,
                physics: None,
                physics_reanchor: None,
                interaction: None,
            })
            .unwrap();
    }
    assert_eq!(commands.capacity(), 1);

    let (control_event_tx, mut control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let inbound_selected = Arc::new(AtomicBool::new(false));
    commands.try_send(NetworkCommand::FlushFrame).unwrap();
    let worker = tokio::spawn(run_network_pump(
        ReadyInboundSession {
            inbound: Some(WorldEvent::ChunkRadiusUpdated(99)),
            inbound_selected: Arc::clone(&inbound_selected),
        },
        NetworkSequencer::new(7, 0, 42),
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
        local_player_unique_id: 1,
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
        weather_cycle_enabled: true,
        rain_level: 0.25,
        lightning_level: 0.75,
    };

    for event in [
        NetworkControlEvent::Bootstrap {
            session_generation: 7,
            world: bootstrap,
            environment,
            custom_blocks: protocol::CustomBlocks::default(),
            inventory: InventoryEvent::Authority(InventoryAuthority::Server),
            item_registry: None,
            player_game_mode: PlayerGameMode::Survival,
            world_default_game_mode: protocol::GameModeUpdate::Explicit(PlayerGameMode::Survival),
            player_game_mode_uses_world_default: false,
            server_authoritative_block_breaking: false,
            rewind_history_size: 20,
            hardcore: false,
            hud_rules: protocol::HudRules::default(),
            packs: (),
            terrain_before_spawn: true,
        },
        NetworkControlEvent::Failed {
            message: "failure".to_owned(),
            decode_error_count: 7,
            server_disconnect: None,
            origin: NetworkFailureOrigin::Startup,
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
                session_generation: 7,
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
        Ok(NetworkControlEvent::Bootstrap {
            session_generation: 7,
            world,
            environment: value,
            inventory: InventoryEvent::Authority(InventoryAuthority::Server),
            player_game_mode: PlayerGameMode::Survival,
            ..
        }) if world == bootstrap && value == environment
    ));
    assert!(matches!(
        control_event_rx.try_recv(),
        Ok(NetworkControlEvent::Failed {
            message,
            decode_error_count: 7,
            server_disconnect: None,
            ..
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
        Ok(WorldIngress::Event(SequencedWorldEvent {
            session_generation: 7,
            sequence: 9,
            event: WorldEvent::ChunkRadiusUpdated(16),
        }))
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
                Some(super::ChatPacketSend {
                    session: 7,
                    sequence: 11,
                    fast_transfer_action: None,
                }),
                None,
            ),
            (
                None,
                None,
                Some(super::SubChunkRequestSend {
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
            chat: Some(super::ChatPacketSend {
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

#[test]
fn saturated_command_queue_preserves_packet_and_shutdown_does_not_join_on_ui_thread() {
    let (commands, _command_rx) = mpsc::channel(COMMAND_CAPACITY);
    for _ in 0..COMMAND_CAPACITY {
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
    }
    let (control_event_tx, control_events) = mpsc::channel(1);
    let (world_event_tx, world_events) = mpsc::channel(1);
    drop(control_event_tx);
    drop(world_event_tx);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    let (physics_reanchor, _physics_reanchor_rx) = watch::channel(0);
    let worker = thread::spawn(|| thread::sleep(Duration::from_millis(250)));
    let mut handle = NetworkHandle {
        session_generation: 0,
        control_events,
        world_events,
        commands,
        pending_latency_reply: std::sync::Mutex::new(None),
        physics_reanchor,
        shutdown,
        thread: Some(worker),
        readiness_ingress: Arc::new(ReadinessIngressCounter::default()),
        experience_gate: Arc::default(),
        unflushed: Default::default(),
    };

    let packet = test_packet();
    let error = handle.send_packet(packet).unwrap_err();
    assert!(matches!(error, PacketSendError::Full(_)));
    let started = Instant::now();
    handle.shutdown();

    assert!(started.elapsed() < Duration::from_millis(100));
    assert!(*handle.shutdown.borrow());
}
