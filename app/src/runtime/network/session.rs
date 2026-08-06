use std::{
    io::Write,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use bevy::prelude::Resource;
use protocol::{
    BlobCacheStats, ClientBlobCache, InventoryEvent, LoginSequence, Packet, PacketIdTraceSnapshot,
    PlayerGameMode, WorldBootstrap, WorldEnvironmentBootstrap, WorldEvent, normalize_authority,
};
use tokio::sync::{mpsc, watch};
use world::ChunkKey;

use crate::{
    acceptance::mutation::write_stdout_marker,
    movement::{MovementTicker, PhysicsSendIdentity},
    ui_runtime::FastTransferAction,
};

pub(crate) const WORLD_EVENT_CAPACITY: usize = 32;
const CONTROL_EVENT_CAPACITY: usize = 64;
const COMMAND_CAPACITY: usize = 64;
const FINAL_CONTROL_FLUSH_TIMEOUT: Duration = Duration::from_millis(250);
const NETWORK_PUMP_TERMINAL_MARKER: &str = "RUST_MCBE_NETWORK_PUMP_TERMINAL";

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub session_generation: u64,
    pub socket_dir: PathBuf,
    pub display_name: String,
    /// Verified blobs outlive a Play session; each login creates a fresh resolver around this cache.
    pub client_blob_cache: ClientBlobCache,
}

#[derive(Debug)]
pub enum NetworkControlEvent {
    Bootstrap {
        session_generation: u64,
        world: WorldBootstrap,
        environment: WorldEnvironmentBootstrap,
        inventory: InventoryEvent,
        player_game_mode: PlayerGameMode,
        world_default_game_mode: PlayerGameMode,
        player_game_mode_uses_world_default: bool,
    },
    SubChunkRequestSent {
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
        sent_at: Instant,
    },
    ChatPacketSent {
        session: u64,
        sequence: u64,
    },
    ChatPacketSendFailed {
        session: u64,
        sequence: u64,
        message: String,
    },
    PhysicsPacketSent {
        identity: PhysicsSendIdentity,
    },
    PhysicsPacketCancelled {
        identity: PhysicsSendIdentity,
        definitely_unsent: bool,
    },
    BlobCacheTelemetry {
        enabled: bool,
        stats: BlobCacheStats,
    },
    Failed {
        message: String,
        decode_error_count: u64,
    },
    Stopped {
        decode_error_count: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequencedWorldEvent {
    pub session_generation: u64,
    pub sequence: u64,
    pub event: WorldEvent,
}

#[derive(Debug, Clone, PartialEq)]
// The FIFO is strictly bounded to WORLD_EVENT_CAPACITY. Keeping the event
// inline avoids adding one heap allocation to every normal world packet just
// to accommodate the rare, small transfer barrier variant.
#[allow(clippy::large_enum_variant)]
pub enum WorldIngress {
    Event(SequencedWorldEvent),
    FastTransferBarrier {
        session_generation: u64,
        sequence: u64,
        action_sequence: u64,
    },
}

#[derive(Debug, Default)]
struct ReadinessIngressCounter {
    produced: AtomicU64,
    consumed: AtomicU64,
}

impl ReadinessIngressCounter {
    fn record_produced(&self, event: &WorldEvent) {
        if readiness_affecting_world_event(event) {
            self.produced.fetch_add(1, Ordering::Release);
        }
    }

    fn record_consumed(&self, event: &WorldEvent) {
        if readiness_affecting_world_event(event) {
            self.consumed.fetch_add(1, Ordering::Release);
        }
    }

    fn progress(&self) -> (u64, u64) {
        (
            self.produced.load(Ordering::Acquire),
            self.consumed.load(Ordering::Acquire),
        )
    }

    fn pending(&self) -> usize {
        let (produced, consumed) = self.progress();
        usize::try_from(produced.saturating_sub(consumed)).unwrap_or(usize::MAX)
    }
}

fn readiness_affecting_world_event(event: &WorldEvent) -> bool {
    matches!(
        event,
        WorldEvent::BiomeDefinitions(_)
            | WorldEvent::LevelChunk(_)
            | WorldEvent::ChunkResync(_)
            | WorldEvent::SubChunkReplyAdmission(_)
            | WorldEvent::SubChunks(_)
            | WorldEvent::BlockUpdates(_)
            | WorldEvent::BlockEntityUpdate(_)
            | WorldEvent::ChunkRadiusUpdated(_)
            | WorldEvent::PublisherUpdate(_)
            | WorldEvent::ChangeDimension(_)
            | WorldEvent::Respawn(_)
            | WorldEvent::MovePlayer(_)
            | WorldEvent::PlayerMovementCorrection(_)
    )
}

fn wrap_readiness_tracked_event(
    sequencer: &mut NetworkSequencer,
    readiness_ingress: &ReadinessIngressCounter,
    event: WorldEvent,
) -> SequencedWorldEvent {
    let sequenced = sequencer.wrap(event);
    readiness_ingress.record_produced(&sequenced.event);
    sequenced
}

#[derive(Debug)]
enum NetworkCommand {
    Send {
        packet: Packet,
        sub_chunk: Option<SubChunkRequestSend>,
        chat: Option<ChatPacketSend>,
        physics: Option<PhysicsSendIdentity>,
        physics_reanchor: Option<watch::Receiver<u64>>,
    },
}

#[derive(Debug, Clone, Copy)]
struct SubChunkRequestSend {
    chunk: ChunkKey,
    base_sub_chunk_y: i32,
    count: usize,
}

#[derive(Debug, Clone, Copy)]
struct ChatPacketSend {
    session: u64,
    sequence: u64,
    fast_transfer_action: Option<FastTransferAction>,
}

#[derive(Debug)]
pub enum PacketSendError {
    Full(Packet),
    Closed(Packet),
}

impl std::fmt::Display for PacketSendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full(_) => formatter.write_str("network command queue is full"),
            Self::Closed(_) => formatter.write_str("network command channel is closed"),
        }
    }
}

impl std::error::Error for PacketSendError {}

impl PacketSendError {
    #[must_use]
    pub fn into_packet(self) -> Packet {
        match self {
            Self::Full(packet) | Self::Closed(packet) => packet,
        }
    }

    #[must_use]
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Closed(_))
    }
}

#[derive(Resource)]
pub struct NetworkHandle {
    control_events: mpsc::Receiver<NetworkControlEvent>,
    world_events: mpsc::Receiver<WorldIngress>,
    commands: mpsc::Sender<NetworkCommand>,
    physics_reanchor: watch::Sender<u64>,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
    readiness_ingress: Arc<ReadinessIngressCounter>,
}

impl NetworkHandle {
    /// A live Bevy app still owns a network resource while it is sitting at
    /// the launcher. Empty channels keep every production system total and
    /// let the menu start without a Go bridge; a later menu connection can
    /// replace this resource with a real session.
    pub(crate) fn disconnected() -> Self {
        empty_network_channels().0
    }

    #[cfg(test)]
    pub(crate) fn stub() -> (Self, watch::Receiver<u64>) {
        empty_network_channels()
    }
    #[cfg(test)]
    pub(crate) fn shutdown_requested(&self) -> bool {
        *self.shutdown.borrow()
    }

    pub(crate) fn movement_ticker(&self) -> MovementTicker {
        MovementTicker::with_epoch_publisher(self.physics_reanchor.clone())
    }

    pub fn control_events_mut(&mut self) -> &mut mpsc::Receiver<NetworkControlEvent> {
        &mut self.control_events
    }

    pub fn world_events_mut(&mut self) -> &mut mpsc::Receiver<WorldIngress> {
        &mut self.world_events
    }

    #[must_use]
    pub fn pending_event_count(&self) -> usize {
        self.control_events
            .len()
            .saturating_add(self.world_events.len())
    }

    #[must_use]
    pub fn pending_command_count(&self) -> usize {
        self.commands
            .max_capacity()
            .saturating_sub(self.commands.capacity())
    }

    #[must_use]
    pub(crate) fn pending_readiness_event_count(&self) -> usize {
        self.readiness_ingress.pending()
    }

    #[must_use]
    pub(crate) fn readiness_ingress_progress(&self) -> (u64, u64) {
        self.readiness_ingress.progress()
    }

    pub(crate) fn record_readiness_event_consumed(&self, event: &WorldEvent) {
        self.readiness_ingress.record_consumed(event);
    }

    #[cfg(test)]
    pub fn send_packet(&self, packet: Packet) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, None, None, None, None)
    }

    pub(crate) fn send_physics_packet(
        &self,
        identity: PhysicsSendIdentity,
        packet: Packet,
    ) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(
            packet,
            None,
            None,
            Some(identity),
            Some(self.physics_reanchor.subscribe()),
        )
    }

    pub(crate) fn send_hotbar_packet(&self, packet: Packet) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, None, None, None, None)
    }

    pub fn send_chat_packet(
        &self,
        session: u64,
        sequence: u64,
        fast_transfer_action: Option<FastTransferAction>,
        packet: Packet,
    ) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(
            packet,
            None,
            Some(ChatPacketSend {
                session,
                sequence,
                fast_transfer_action,
            }),
            None,
            None,
        )
    }

    pub fn send_sub_chunk_request(
        &self,
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
        packet: Packet,
    ) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(
            packet,
            Some(SubChunkRequestSend {
                chunk,
                base_sub_chunk_y,
                count,
            }),
            None,
            None,
            None,
        )
    }

    fn send_packet_with_confirmation(
        &self,
        packet: Packet,
        sub_chunk: Option<SubChunkRequestSend>,
        chat: Option<ChatPacketSend>,
        physics: Option<PhysicsSendIdentity>,
        physics_reanchor: Option<watch::Receiver<u64>>,
    ) -> Result<(), PacketSendError> {
        self.commands
            .try_send(NetworkCommand::Send {
                packet,
                sub_chunk,
                chat,
                physics,
                physics_reanchor,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(NetworkCommand::Send { packet, .. }) => {
                    PacketSendError::Full(packet)
                }
                mpsc::error::TrySendError::Closed(NetworkCommand::Send { packet, .. }) => {
                    PacketSendError::Closed(packet)
                }
            })
    }

    pub fn shutdown(&mut self) {
        self.shutdown.send_replace(true);
        self.release_thread();
    }

    fn release_thread(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.is_finished() {
            let _ = thread.join();
            return;
        }
        // Joining can wait on socket teardown or a slow transport. Keep that
        // wait off Bevy's UI thread while still reaping the worker normally.
        let _ = thread::Builder::new()
            .name("bedrock-network-reaper".to_owned())
            .spawn(move || {
                let _ = thread.join();
            });
    }
}

fn empty_network_channels() -> (NetworkHandle, watch::Receiver<u64>) {
    let (_control_event_tx, control_events) = mpsc::channel(1);
    let (_world_event_tx, world_events) = mpsc::channel(1);
    let (commands, _command_rx) = mpsc::channel(1);
    let (physics_reanchor, physics_reanchor_rx) = watch::channel(0);
    let (shutdown, _shutdown_rx) = watch::channel(false);
    (
        NetworkHandle {
            control_events,
            world_events,
            commands,
            physics_reanchor,
            shutdown,
            thread: None,
            readiness_ingress: Arc::new(ReadinessIngressCounter::default()),
        },
        physics_reanchor_rx,
    )
}

impl Drop for NetworkHandle {
    fn drop(&mut self) {
        self.shutdown.send_replace(true);
        self.release_thread();
    }
}

pub fn spawn_network(config: NetworkConfig) -> Result<NetworkHandle, std::io::Error> {
    let session_generation = config.session_generation;
    let (control_event_tx, control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_event_tx, world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (physics_reanchor, _physics_reanchor_rx) = watch::channel(0);
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let readiness_ingress = Arc::new(ReadinessIngressCounter::default());
    let network_readiness_ingress = Arc::clone(&readiness_ingress);
    let thread = thread::Builder::new()
        .name("bedrock-network".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = control_event_tx.try_send(NetworkControlEvent::Failed {
                        message: format!("failed to create network runtime: {error}"),
                        decode_error_count: 0,
                    });
                    return;
                }
            };
            runtime.block_on(async move {
                let Some(login) = wait_for_login_or_cancel(
                    LoginSequence::connect_with_blob_cache(
                        &config.socket_dir,
                        &config.display_name,
                        config.client_blob_cache.clone(),
                    ),
                    &mut shutdown_rx,
                )
                .await
                else {
                    return;
                };
                let (session, game_data) = match login {
                    Ok(connected) => connected,
                    Err(error) => {
                        let _ = send_control_event_or_cancel(
                            &control_event_tx,
                            &mut shutdown_rx,
                            NetworkControlEvent::Failed {
                                message: error.to_string(),
                                decode_error_count: 0,
                            },
                        )
                        .await;
                        return;
                    }
                };
                let bootstrap = WorldBootstrap::from_game_data(&game_data);
                let environment = WorldEnvironmentBootstrap::from_game_data(&game_data);
                let inventory = start_game_inventory_authority(&game_data);
                let player_game_mode = PlayerGameMode::from_game_data(&game_data);
                let world_default_game_mode =
                    PlayerGameMode::world_default_from_game_data(&game_data);
                let player_game_mode_uses_world_default =
                    PlayerGameMode::bootstrap_uses_world_default(&game_data);
                if !send_control_event_or_cancel(
                    &control_event_tx,
                    &mut shutdown_rx,
                    NetworkControlEvent::Bootstrap {
                        session_generation,
                        world: bootstrap,
                        environment,
                        inventory,
                        player_game_mode,
                        world_default_game_mode,
                        player_game_mode_uses_world_default,
                    },
                )
                .await
                {
                    return;
                }
                let sequencer = NetworkSequencer::new(
                    session_generation,
                    bootstrap.dimension,
                    bootstrap.local_player_runtime_id,
                );
                run_network_pump_with_readiness_ingress(
                    session,
                    sequencer,
                    command_rx,
                    control_event_tx,
                    world_event_tx,
                    shutdown_rx,
                    network_readiness_ingress,
                )
                .await;
            });
        })?;
    Ok(NetworkHandle {
        control_events,
        world_events,
        commands,
        physics_reanchor,
        shutdown,
        thread: Some(thread),
        readiness_ingress,
    })
}

fn start_game_inventory_authority(game_data: &protocol::GameData) -> InventoryEvent {
    normalize_authority(game_data.start_game.enable_item_stack_net_manager)
}

trait NetworkSession: Send {
    type Error: std::fmt::Display + Send;

    fn receive_world_event(
        &mut self,
        current_dimension: i32,
    ) -> impl Future<Output = Result<WorldEvent, Self::Error>> + Send;

    fn send_packet(
        &mut self,
        packet: Packet,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn decode_error_count(&self) -> u64;

    fn blob_cache_enabled(&self) -> bool {
        false
    }

    fn blob_cache_stats(&self) -> BlobCacheStats {
        BlobCacheStats::default()
    }

    fn begin_packet_id_trace(&mut self) {}

    fn cancel_packet_id_trace(&mut self) {}

    fn arm_blob_cache_reset_for_fast_transfer(&mut self) {}

    fn drain_packet_id_trace(&mut self) -> Option<PacketIdTraceSnapshot> {
        None
    }
}

impl NetworkSession for protocol::PlaySession {
    type Error = protocol::ProtocolError;

    fn receive_world_event(
        &mut self,
        current_dimension: i32,
    ) -> impl Future<Output = Result<WorldEvent, Self::Error>> + Send {
        self.recv_world_event(current_dimension)
    }

    fn send_packet(
        &mut self,
        packet: Packet,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.send(packet)
    }

    fn decode_error_count(&self) -> u64 {
        protocol::PlaySession::decode_error_count(self)
    }

    fn blob_cache_enabled(&self) -> bool {
        protocol::PlaySession::blob_cache_enabled(self)
    }

    fn blob_cache_stats(&self) -> BlobCacheStats {
        protocol::PlaySession::blob_cache_stats(self)
    }

    fn begin_packet_id_trace(&mut self) {
        protocol::PlaySession::begin_packet_id_trace(self);
    }

    fn cancel_packet_id_trace(&mut self) {
        protocol::PlaySession::cancel_packet_id_trace(self);
    }

    fn arm_blob_cache_reset_for_fast_transfer(&mut self) {
        protocol::PlaySession::arm_blob_cache_reset_for_fast_transfer(self);
    }

    fn drain_packet_id_trace(&mut self) -> Option<PacketIdTraceSnapshot> {
        protocol::PlaySession::drain_packet_id_trace(self)
    }
}

fn try_emit_blob_cache_telemetry<S: NetworkSession>(
    session: &S,
    control_event_tx: &mpsc::Sender<NetworkControlEvent>,
    last_stats: &mut Option<BlobCacheStats>,
) {
    if !session.blob_cache_enabled() {
        return;
    }
    let stats = session.blob_cache_stats();
    if *last_stats == Some(stats) {
        return;
    }
    if control_event_tx
        .try_send(NetworkControlEvent::BlobCacheTelemetry {
            enabled: true,
            stats,
        })
        .is_ok()
    {
        emit_bounded_blob_cache_warning(last_stats.unwrap_or_default(), stats);
        emit_blob_cache_telemetry(stats);
        *last_stats = Some(stats);
    }
}

async fn send_final_blob_cache_telemetry<S: NetworkSession>(
    session: &S,
    control_event_tx: &mpsc::Sender<NetworkControlEvent>,
) -> bool {
    if !session.blob_cache_enabled() {
        return true;
    }
    let stats = session.blob_cache_stats();
    emit_blob_cache_telemetry(stats);
    matches!(
        tokio::time::timeout(
            FINAL_CONTROL_FLUSH_TIMEOUT,
            control_event_tx.send(NetworkControlEvent::BlobCacheTelemetry {
                enabled: true,
                stats,
            }),
        )
        .await,
        Ok(Ok(()))
    )
}

fn emit_blob_cache_telemetry(stats: BlobCacheStats) {
    bevy::log::info!(
        target: "bedrock_client::blob_cache",
        hashes_classified = stats.hashes_classified,
        hits = stats.hits,
        misses = stats.misses,
        redundant_missing_requests = stats.redundant_missing_requests,
        admitted_blobs = stats.admitted_blobs,
        rejected_blobs = stats.rejected_blobs,
        evictions = stats.evictions,
        pending_transactions = stats.pending_transactions,
        pending_bytes = stats.pending_bytes,
        retained_cached_transactions = stats.retained_cached_transactions,
        ordinary_ready_events = stats.ordinary_ready_events,
        ordinary_ready_bytes = stats.ordinary_ready_bytes,
        recovery_ready_events = stats.recovery_ready_events,
        recovery_ready_bytes = stats.recovery_ready_bytes,
        pending_resets = stats.pending_resets,
        skipped_packets = stats.skipped_packets,
        skipped_world_events = stats.skipped_world_events,
        skipped_cached_packets = stats.skipped_cached_packets,
        skipped_miss_responses = stats.skipped_miss_responses,
        empty_miss_responses = stats.empty_miss_responses,
        cached_packet_semantic_shape = stats.cached_packet_semantic_shape,
        cached_packet_transaction_pressure = stats.cached_packet_transaction_pressure,
        cached_packet_pending_pressure = stats.cached_packet_pending_pressure,
        cached_packet_staged_pressure = stats.cached_packet_staged_pressure,
        cached_packet_reconstruction_pressure = stats.cached_packet_reconstruction_pressure,
        cached_packet_ready_pressure = stats.cached_packet_ready_pressure,
        miss_response_unsolicited = stats.miss_response_unsolicited,
        miss_response_integrity_rejection = stats.miss_response_integrity_rejection,
        miss_response_cache_pressure = stats.miss_response_cache_pressure,
        abandoned_cached_transactions = stats.abandoned_cached_transactions,
        recovery_requests = stats.recovery_requests,
        ordinary_backpressure = stats.ordinary_backpressure,
        reconstructed_level_chunks = stats.reconstructed_level_chunks,
        reconstructed_sub_chunks = stats.reconstructed_sub_chunks,
        "client blob cache counters"
    );
}

fn emit_bounded_blob_cache_warning(previous: BlobCacheStats, current: BlobCacheStats) {
    let cached_packet_due = bounded_counter_log_due(
        previous.skipped_cached_packets,
        current.skipped_cached_packets,
    );
    let miss_response_due = bounded_counter_log_due(
        previous.skipped_miss_responses,
        current.skipped_miss_responses,
    );
    if cached_packet_due || miss_response_due {
        bevy::log::warn!(
            target: "bedrock_client::blob_cache",
            skipped_cached_packets = current.skipped_cached_packets,
            skipped_miss_responses = current.skipped_miss_responses,
            cached_packet_semantic_shape = current.cached_packet_semantic_shape,
            cached_packet_transaction_pressure = current.cached_packet_transaction_pressure,
            cached_packet_pending_pressure = current.cached_packet_pending_pressure,
            cached_packet_staged_pressure = current.cached_packet_staged_pressure,
            cached_packet_reconstruction_pressure = current.cached_packet_reconstruction_pressure,
            cached_packet_ready_pressure = current.cached_packet_ready_pressure,
            miss_response_unsolicited = current.miss_response_unsolicited,
            miss_response_integrity_rejection = current.miss_response_integrity_rejection,
            miss_response_cache_pressure = current.miss_response_cache_pressure,
            "skipped semantically invalid client blob-cache packet"
        );
    }
}

fn bounded_counter_log_due(previous: u64, current: u64) -> bool {
    current != 0 && current > previous && (previous == 0 || current.ilog2() > previous.ilog2())
}

fn emit_network_pump_terminal_marker(stage: &'static str, message: &str, decode_errors: u64) {
    let mut stdout = std::io::stdout().lock();
    write_network_pump_terminal_marker(&mut stdout, stage, message, decode_errors);
    let _ = stdout.flush();
}

fn write_network_pump_terminal_marker(
    writer: &mut impl Write,
    stage: &'static str,
    message: &str,
    decode_errors: u64,
) {
    let marker = serde_json::json!({
        "schema": "rust-mcbe-network-pump-terminal-v1",
        "outcome": "failed",
        "stage": stage,
        "message": message,
        "decode_error_count": decode_errors,
    });
    let _ = writeln!(writer, "{NETWORK_PUMP_TERMINAL_MARKER}={marker}");
}

fn emit_packet_id_trace<S: NetworkSession>(session: &mut S) {
    let Some(trace) = session.drain_packet_id_trace() else {
        return;
    };
    let marker = serde_json::json!({
        "schema": "rust-mcbe-fast-transfer-packet-trace-v1",
        "packet_ids": trace.packet_ids,
        "overflow": trace.overflow,
        "timed_out": trace.timed_out,
    });
    write_stdout_marker(
        &mut std::io::stdout().lock(),
        &format!(
            "{}={marker}",
            crate::acceptance::markers::FAST_TRANSFER_PACKET_TRACE
        ),
    );
}

mod pump;
use pump::*;
mod pump_runtime;
use pump_runtime::*;

#[cfg(test)]
mod tests;
