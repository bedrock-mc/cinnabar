use protocol::identity::FifoSequence;
use render_api::sub_chunk_requests::SubChunkRequestReceipt;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use web_time::Instant;

use bytes::Bytes;
use protocol::{
    BlobCacheStats, CustomBlocks, InventoryEvent, ItemRegistryEvent, Packet, PacketIdTraceSnapshot,
    PlayerGameMode, ServerDisconnectEvent, WorldBootstrap, WorldEnvironmentBootstrap, WorldEvent,
};
use tokio::sync::{mpsc, watch};

use crate::SessionTrace;
use protocol::{FastTransferAction, InteractionPacketGuard, PhysicsSendIdentity};

/// Holds a frame's light burst; terrain admission, not this bound, paces chunk ingress.
pub const WORLD_EVENT_CAPACITY: usize = 256;
/// Bounds queued controls; send receipts wait here until the frame drains them.
pub const CONTROL_EVENT_CAPACITY: usize = 64;
const COMMAND_CAPACITY: usize = 64;
const FINAL_CONTROL_FLUSH_TIMEOUT: Duration = Duration::from_millis(250);
const NETWORK_PUMP_TERMINAL_MARKER: &str = "CINNABAR_NETWORK_PUMP_TERMINAL";

#[cfg(not(target_arch = "wasm32"))]
pub struct NetworkConfig {
    pub session_generation: SessionGeneration,
    pub endpoint: protocol::SessionEndpoint,
    /// One application-owned acquisition future, polled on this session's runtime and cancelled with it.
    #[cfg(feature = "native-networking")]
    native_preparation: Option<NativeConnectionPreparation>,
    pub display_name: String,
    /// The user storage owner's opaque persisted login identifier.
    pub client_random_id: protocol::ClientRandomId,
    /// Verified blobs outlive a Play session; each login creates a fresh resolver around this cache.
    pub client_blob_cache: protocol::ClientBlobCache,
    /// The client's own skin, uploaded in the ClientData login payload.
    pub player_skin: protocol::ClientSkin,
    /// The player's language, input mode and GUI scale, reported at login.
    pub login_settings: protocol::LoginSettings,
    /// Records this session's full packet capture when set.
    pub packet_capture: Option<packet_log::PacketCaptureRequest>,
}

#[cfg(not(target_arch = "wasm32"))]
mod config;

/// One application-owned preparation that resolves its endpoint and moves the connection proof into Login.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub type NativeConnectionPreparation = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<
                    (
                        protocol::NativeConnectionTarget,
                        protocol::NativeAuthentication,
                    ),
                    std::io::Error,
                >,
            > + Send,
    >,
>;

/// Prepared facts returned by the pack owner at the join bootstrap boundary.
#[derive(Debug)]
pub struct PreparedSession<P> {
    pub custom_blocks: CustomBlocks,
    pub packs_applied: bool,
    pub packs: P,
}

/// Which transport leg or lifecycle stage produced a session failure.
///
/// Only [`NetworkFailureOrigin::Receive`] represents a remote-initiated
/// termination of an active play session; every other origin is owned by the
/// local process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkFailureOrigin {
    /// The active session's inbound transport failed: the server disconnected
    /// or the upstream read terminated mid-session.
    Receive,
    /// The outbound transport failed while writing a packet.
    Send,
    /// The session failed before the play pump started (runtime/login).
    Startup,
}

/// The bounded server-directed transfer target carried by terminal events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTransferTarget {
    pub host: String,
    pub port: u16,
}

#[derive(Debug)]
pub enum NetworkControlEvent<P = ()> {
    Bootstrap {
        session_generation: SessionGeneration,
        world: WorldBootstrap,
        environment: WorldEnvironmentBootstrap,
        custom_blocks: CustomBlocks,
        inventory: InventoryEvent,
        item_registry: Option<ItemRegistryEvent>,
        player_game_mode: PlayerGameMode,
        world_default_game_mode: protocol::GameModeUpdate,
        player_game_mode_uses_world_default: bool,
        server_authoritative_block_breaking: bool,
        /// StartGame `RewindHistorySize`, raw.
        rewind_history_size: i32,
        hardcore: bool,
        hud_rules: protocol::HudRules,
        death_rules: protocol::DeathRules,
        crafting_rules: protocol::CraftingRules,
        packs: P,
        /// Whether the server sent terrain before spawn; Dragonfly sends none until initialized.
        terrain_before_spawn: bool,
        /// StartGame's hashed world identity, absent when no level ID was supplied.
        level_identity: Option<u64>,
    },
    SubChunkRequestSent {
        receipt: SubChunkRequestReceipt,
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
        server_disconnect: Option<ServerDisconnectEvent>,
        origin: NetworkFailureOrigin,
    },
    /// The server directed the client to a new target, ending this session.
    ///
    /// Like [`NetworkControlEvent::Failed`] this is terminal: the pump stops
    /// after emitting it and no `Stopped` record follows. The wire's
    /// `reload_world` hint is recorded in the pump's durable transferred
    /// marker; it does not change the app-side handoff.
    Transferred {
        target: SessionTransferTarget,
        decode_error_count: u64,
    },
    Stopped {
        decode_error_count: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequencedWorldEvent {
    pub session_generation: SessionGeneration,
    pub sequence: FifoSequence,
    pub event: WorldEvent,
}

#[derive(Debug, Clone, PartialEq)]
enum InboundWorldEvent {
    Event(WorldEvent),
    LevelChunk {
        event: protocol::LevelChunkEvent,
        payload: Bytes,
    },
}

#[derive(Debug, Clone, PartialEq)]
// The FIFO is strictly bounded to WORLD_EVENT_CAPACITY. Keeping the event
// inline avoids adding one heap allocation to every normal world packet just
// to accommodate the rare, small transfer barrier variant.
#[allow(clippy::large_enum_variant)]
pub enum WorldIngress {
    Event(SequencedWorldEvent),
    LevelChunk {
        session_generation: SessionGeneration,
        sequence: FifoSequence,
        event: protocol::LevelChunkEvent,
        payload: Bytes,
    },
    FastTransferBarrier {
        session_generation: SessionGeneration,
        sequence: FifoSequence,
        action_sequence: u64,
    },
}

impl WorldIngress {
    /// The session generation and FIFO sequence this ingress carries.
    #[must_use]
    pub fn identity(&self) -> (u64, u64) {
        match self {
            Self::Event(event) => (event.session_generation.get(), event.sequence.get()),
            Self::LevelChunk {
                session_generation,
                sequence,
                ..
            }
            | Self::FastTransferBarrier {
                session_generation,
                sequence,
                ..
            } => (session_generation.get(), sequence.get()),
        }
    }
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

fn wrap_inbound_world_event(
    sequencer: &mut NetworkSequencer,
    readiness_ingress: &ReadinessIngressCounter,
    event: InboundWorldEvent,
) -> WorldIngress {
    match event {
        InboundWorldEvent::Event(event) => WorldIngress::Event(wrap_readiness_tracked_event(
            sequencer,
            readiness_ingress,
            event,
        )),
        InboundWorldEvent::LevelChunk { event, payload } => {
            let sequence = sequencer.take_sequence();
            readiness_ingress.produced.fetch_add(1, Ordering::Release);
            WorldIngress::LevelChunk {
                session_generation: sequencer.session_generation(),
                sequence,
                event,
                payload,
            }
        }
    }
}

// Keep frequent packet sends inline; completion is a single command per session.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum NetworkCommand {
    FinishLoading,
    /// Ends the frame: everything queued since the previous flush leaves as one batch.
    FlushFrame {
        cache_opportunity: bool,
    },
    Send {
        packet: Packet,
        sub_chunk: Option<SubChunkRequestReceipt>,
        chat: Option<ChatPacketSend>,
        physics: Option<PhysicsSendIdentity>,
        physics_reanchor: Option<watch::Receiver<PositionAuthorityGeneration>>,
        interaction: Option<InteractionPacketGuard>,
    },
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

pub use protocol::BatchSendError;

pub struct NetworkHandle<P = ()> {
    session_generation: u64,
    control_events: mpsc::Receiver<NetworkControlEvent<P>>,
    world_events: mpsc::Receiver<WorldIngress>,
    commands: mpsc::Sender<NetworkCommand>,
    pending_latency_reply: Mutex<Option<Packet>>,
    physics_reanchor: watch::Sender<PositionAuthorityGeneration>,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
    readiness_ingress: Arc<ReadinessIngressCounter>,
    experience_gate: Arc<experience::ExperienceGate>,
    unflushed: Mutex<frame_flush::FrameFlush>,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    packet_delay: packet_delay::Control,
    connection_observation: observation::Observation,
}

impl<P> NetworkHandle<P> {
    /// Queues the readiness boundary after the loading screen can close.
    pub fn finish_loading(&self) -> bool {
        let mut frame = self.unflushed.lock().expect("outbound frame lock");
        let Ok(mut permits) = frame.reserve(&self.commands, 1) else {
            return false;
        };
        permits
            .next()
            .expect("one reserved command")
            .send(NetworkCommand::FinishLoading);
        true
    }

    /// Ends the frame's batch without waiting for capacity or another frame.
    /// A clean frame submits only a retained simulation opportunity.
    pub fn flush_frame(&self) {
        let _ = self.flush_latency_reply();
        self.unflushed
            .lock()
            .expect("outbound frame lock")
            .flush(&self.commands);
    }

    /// Records normal simulation progress independently of player movement and packet traffic.
    pub fn advance_simulation(&self, elapsed: Duration) {
        self.unflushed
            .lock()
            .expect("outbound frame lock")
            .advance_simulation(elapsed);
    }

    /// A live Bevy app still owns a network resource while it is sitting at
    /// the launcher. Empty channels keep every production system total and
    /// let the menu start without a Go bridge; a later menu connection can
    /// replace this resource with a real session.
    pub fn disconnected() -> Self {
        empty_network_channels().0
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn stub() -> (Self, watch::Receiver<PositionAuthorityGeneration>) {
        empty_network_channels()
    }
    /// A test handle with `capacity` payload slots plus its frame delimiter; the guard keeps it open.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_command_capacity(capacity: usize) -> (Self, Box<dyn std::any::Any>) {
        let (mut handle, _) = empty_network_channels();
        let (commands, receiver) = frame_flush::command_channel(capacity);
        handle.commands = commands;
        (handle, Box::new(receiver))
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn shutdown_requested(&self) -> bool {
        *self.shutdown.borrow()
    }

    /// Shares the transport cancellation epoch with the gameplay prediction owner.
    pub fn physics_epoch_publisher(&self) -> watch::Sender<PositionAuthorityGeneration> {
        self.physics_reanchor.clone()
    }

    pub fn control_events_mut(&mut self) -> &mut mpsc::Receiver<NetworkControlEvent<P>> {
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
        let pending_reply = usize::from(
            self.pending_latency_reply
                .lock()
                .expect("latency reply lock")
                .is_some(),
        );
        let frame = self.unflushed.lock().expect("outbound frame lock");
        self.commands
            .max_capacity()
            .saturating_sub(self.commands.capacity())
            .saturating_sub(frame.reserved_slots())
            .saturating_add(pending_reply)
    }

    #[must_use]
    pub fn pending_readiness_event_count(&self) -> usize {
        self.readiness_ingress.pending()
    }

    #[must_use]
    pub fn readiness_ingress_progress(&self) -> (u64, u64) {
        self.readiness_ingress.progress()
    }

    pub fn record_readiness_event_consumed(&self, event: &WorldEvent) {
        self.readiness_ingress.record_consumed(event);
    }

    pub fn record_level_chunk_consumed(&self) {
        self.readiness_ingress
            .consumed
            .fetch_add(1, Ordering::Release);
    }

    pub fn send_physics_packet(
        &self,
        identity: PhysicsSendIdentity,
        packet: Packet,
        interaction: Option<InteractionPacketGuard>,
    ) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(
            packet,
            None,
            None,
            Some(identity),
            Some(self.physics_reanchor.subscribe()),
            interaction,
        )
    }

    pub fn send_hotbar_packet(&self, packet: Packet) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, None, None, None, None, None)
    }

    /// Queues an unguarded movement-side packet such as a prediction sync.
    pub fn send_movement_packet(&self, packet: Packet) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, None, None, None, None, None)
    }

    /// Queues a transfer acknowledgement without invalidating it on a physics reanchor.
    pub fn send_dimension_packet(&self, packet: Packet) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, None, None, None, None, None)
    }

    /// Queues an inventory, swing or interaction packet ahead of this frame's movement.
    pub fn send_inventory_packet(&self, packet: Packet) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, None, None, None, None, None)
    }

    /// Queues standalone packets together or not at all, so a swing never leaves without its
    /// transaction.
    pub fn send_inventory_packets(&self, packets: Vec<Packet>) -> Result<(), BatchSendError> {
        if packets.is_empty() {
            return Ok(());
        }
        self.flush_latency_reply()?;
        let mut frame = self.unflushed.lock().expect("outbound frame lock");
        let permits = frame.reserve(&self.commands, packets.len())?;
        for (permit, packet) in permits.zip(packets) {
            permit.send(NetworkCommand::Send {
                packet,
                sub_chunk: None,
                chat: None,
                physics: None,
                physics_reanchor: None,
                interaction: None,
            });
        }
        Ok(())
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
            None,
        )
    }

    /// Queues one terrain request with the membership echoed after its successful write.
    pub fn send_sub_chunk_request(
        &self,
        receipt: SubChunkRequestReceipt,
        packet: Packet,
    ) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, Some(receipt), None, None, None, None)
    }

    fn send_packet_with_confirmation(
        &self,
        packet: Packet,
        sub_chunk: Option<SubChunkRequestReceipt>,
        chat: Option<ChatPacketSend>,
        physics: Option<PhysicsSendIdentity>,
        physics_reanchor: Option<watch::Receiver<PositionAuthorityGeneration>>,
        interaction: Option<InteractionPacketGuard>,
    ) -> Result<(), PacketSendError> {
        if let Err(error) = self.flush_latency_reply() {
            return Err(match error {
                BatchSendError::Full => PacketSendError::Full(packet),
                BatchSendError::Closed => PacketSendError::Closed(packet),
            });
        }
        let mut frame = self.unflushed.lock().expect("outbound frame lock");
        let permit = match frame.reserve(&self.commands, 1) {
            Ok(mut permits) => permits.next().expect("one reserved command"),
            Err(BatchSendError::Full) => return Err(PacketSendError::Full(packet)),
            Err(BatchSendError::Closed) => return Err(PacketSendError::Closed(packet)),
        };
        permit.send(NetworkCommand::Send {
            packet,
            sub_chunk,
            chat,
            physics,
            physics_reanchor,
            interaction,
        });
        Ok(())
    }

    pub fn shutdown(&mut self) {
        *self
            .pending_latency_reply
            .lock()
            .expect("latency reply lock") = None;
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        self.packet_delay.clear();
        self.shutdown.send_replace(true);
        self.connection_observation.clear();
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

fn empty_network_channels<P>() -> (
    NetworkHandle<P>,
    watch::Receiver<PositionAuthorityGeneration>,
) {
    let (_control_event_tx, control_events) = mpsc::channel(1);
    let (_world_event_tx, world_events) = mpsc::channel(1);
    let (commands, _command_rx) = mpsc::channel(1);
    let (physics_reanchor, physics_reanchor_rx) =
        watch::channel(PositionAuthorityGeneration::default());
    let (shutdown, _shutdown_rx) = watch::channel(false);
    (
        NetworkHandle {
            session_generation: 0,
            control_events,
            world_events,
            commands,
            pending_latency_reply: Mutex::new(None),
            physics_reanchor,
            shutdown,
            thread: None,
            readiness_ingress: Arc::new(ReadinessIngressCounter::default()),
            experience_gate: Arc::default(),
            connection_observation: Default::default(),
            unflushed: Default::default(),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            packet_delay: Default::default(),
        },
        physics_reanchor_rx,
    )
}

impl<P> Drop for NetworkHandle<P> {
    fn drop(&mut self) {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        self.packet_delay.clear();
        self.shutdown.send_replace(true);
        self.connection_observation.clear();
        self.release_thread();
    }
}

pub(crate) mod execution;
mod observation;
mod start;
pub use execution::{NetworkExecutor, SessionRead};
#[cfg(not(target_arch = "wasm32"))]
pub use start::spawn_network;
pub use start::{PreparedLogin, prepare_network};
#[cfg(not(target_arch = "wasm32"))]
pub use start::{SessionCompanion, spawn_network_with_companion};

/// The inbound half of a play session; its write half runs on its own task.
trait NetworkSession: execution::SessionOwner {
    type Error: std::fmt::Display + Send;
    type Outbound: OutboundSession;

    /// Detaches the write half before the pump starts.
    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error>;

    /// Completes connection teardown before the network runtime stops.
    fn close(&mut self) -> impl execution::SessionFuture<Output = ()> {
        std::future::ready(())
    }

    fn receive_world_event(
        &mut self,
        current_dimension: i32,
    ) -> impl execution::SessionFuture<Output = Result<WorldEvent, Self::Error>>;

    fn receive_world_ingress(
        &mut self,
        current_dimension: i32,
    ) -> impl execution::SessionFuture<Output = Result<InboundWorldEvent, Self::Error>> {
        async move {
            self.receive_world_event(current_dimension)
                .await
                .map(InboundWorldEvent::Event)
        }
    }

    fn decode_error_count(&self) -> u64;

    /// Buffers bounded raw input without decoding world events while their consumer is full.
    fn read_ahead(&mut self) -> impl execution::SessionFuture<Output = ()> {
        std::future::pending()
    }

    /// Reads inbound until it ends, so a Transfer or Disconnect the core sent before closing is
    /// retained when a write fails first; the caller bounds the wait.
    fn drain_to_boundary(&mut self) -> impl execution::SessionFuture<Output = ()> {
        async move { while self.receive_world_ingress(0).await.is_ok() {} }
    }

    fn take_server_disconnect(&mut self) -> Option<ServerDisconnectEvent> {
        None
    }

    fn take_server_transfer(&mut self) -> Option<protocol::ServerTransferEvent> {
        None
    }

    fn blob_cache_enabled(&self) -> bool {
        false
    }

    fn blob_cache_stats(&self) -> BlobCacheStats {
        BlobCacheStats::default()
    }

    fn begin_packet_id_trace(&mut self) {}

    /// The latency-trace connection, core frame and in-frame index of the packet received last.
    fn latency_trace_position(&self) -> (u64, u64, u32) {
        (0, 0, 0)
    }

    fn cancel_packet_id_trace(&mut self) {}

    fn arm_blob_cache_reset_for_fast_transfer(&mut self) {}

    fn drain_packet_id_trace(&mut self) -> Option<PacketIdTraceSnapshot> {
        None
    }
}

impl<R: execution::SessionRead> NetworkSession for protocol::PlaySession<R> {
    type Error = protocol::ProtocolError;
    type Outbound = protocol::PlayOutbound;

    fn close(&mut self) -> impl execution::SessionFuture<Output = ()> {
        async move {
            if let Err(error) = protocol::PlaySession::close(self).await {
                tracing::debug!(%error, "connection teardown ended before graceful close");
            }
        }
    }

    fn outbound(&mut self) -> Result<Self::Outbound, Self::Error> {
        protocol::PlaySession::outbound(self)
    }

    fn receive_world_event(
        &mut self,
        current_dimension: i32,
    ) -> impl execution::SessionFuture<Output = Result<WorldEvent, Self::Error>> {
        self.recv_world_event(current_dimension)
    }

    fn receive_world_ingress(
        &mut self,
        current_dimension: i32,
    ) -> impl execution::SessionFuture<Output = Result<InboundWorldEvent, Self::Error>> {
        self.recv_world_event_mapped(
            current_dimension,
            InboundWorldEvent::Event,
            |event, payload| InboundWorldEvent::LevelChunk { event, payload },
        )
    }

    fn decode_error_count(&self) -> u64 {
        protocol::PlaySession::decode_error_count(self)
    }

    /// Reads into the play session's bounded raw transport buffer.
    fn read_ahead(&mut self) -> impl execution::SessionFuture<Output = ()> {
        protocol::PlaySession::read_ahead(self)
    }

    fn drain_to_boundary(&mut self) -> impl execution::SessionFuture<Output = ()> {
        protocol::PlaySession::drain_to_boundary(self)
    }

    fn take_server_disconnect(&mut self) -> Option<ServerDisconnectEvent> {
        protocol::PlaySession::take_server_disconnect(self)
    }

    fn take_server_transfer(&mut self) -> Option<protocol::ServerTransferEvent> {
        protocol::PlaySession::take_server_transfer(self)
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

    fn latency_trace_position(&self) -> (u64, u64, u32) {
        protocol::PlaySession::latency_trace_position(self)
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

pub fn session_failure_display(
    transport_message: &str,
    server_disconnect: Option<&ServerDisconnectEvent>,
) -> String {
    match server_disconnect.and_then(disconnect_display_reason) {
        Some(reason) => format!("server disconnected: {reason} ({transport_message})"),
        None => format!("network session failed: {transport_message}"),
    }
}

fn emit_network_pump_terminal_marker(
    stage: &'static str,
    message: &str,
    decode_errors: u64,
    server_disconnect: Option<&ServerDisconnectEvent>,
) {
    let mut stdout = std::io::stdout().lock();
    write_network_pump_terminal_marker(
        &mut stdout,
        stage,
        message,
        decode_errors,
        server_disconnect,
    );
    let _ = stdout.flush();
}

/// Emits the durable transferred-session record so live evidence attributes
/// the session end to the server's transfer instead of a transport failure.
fn emit_network_pump_transfer_marker(
    target: &SessionTransferTarget,
    reload_world: bool,
    decode_errors: u64,
) {
    let mut stdout = std::io::stdout().lock();
    write_network_pump_transfer_marker(&mut stdout, target, reload_world, decode_errors);
    let _ = stdout.flush();
}

fn write_network_pump_terminal_marker(
    writer: &mut impl Write,
    stage: &'static str,
    message: &str,
    decode_errors: u64,
    server_disconnect: Option<&ServerDisconnectEvent>,
) {
    let mut marker = serde_json::json!({
        "schema": "rust-mcbe-network-pump-terminal-v1",
        "outcome": "failed",
        "stage": stage,
        "message": message,
        "decode_error_count": decode_errors,
    });
    if let Some(disconnect) = server_disconnect {
        marker["server_disconnect"] = serde_json::json!({
            "reason": disconnect.reason,
            "message": disconnect.message,
            "filtered_message": disconnect.filtered_message,
        });
    }
    let _ = writeln!(writer, "{NETWORK_PUMP_TERMINAL_MARKER}={marker}");
}

fn write_network_pump_transfer_marker(
    writer: &mut impl Write,
    target: &SessionTransferTarget,
    reload_world: bool,
    decode_errors: u64,
) {
    let marker = serde_json::json!({
        "schema": "rust-mcbe-network-pump-terminal-v1",
        "outcome": "transferred",
        "target": { "host": target.host, "port": target.port },
        "reload_world": reload_world,
        "decode_error_count": decode_errors,
    });
    let _ = writeln!(writer, "{NETWORK_PUMP_TERMINAL_MARKER}={marker}");
}

/// Writes a transport observation immediately without making I/O failure fatal.
fn write_stdout_marker(writer: &mut impl Write, marker: &str) {
    let _ = writeln!(writer, "{marker}");
    let _ = writer.flush();
}

fn emit_packet_id_trace<S: NetworkSession>(session: &mut S, observation: &SessionTrace) {
    let Some(trace) = session.drain_packet_id_trace() else {
        return;
    };
    let marker = serde_json::json!({
        "schema": "rust-mcbe-fast-transfer-packet-trace-v1",
        "packet_ids": trace.packet_ids,
        "overflow": trace.overflow,
        "timed_out": trace.timed_out,
    });
    (observation.packet_trace)(&marker.to_string());
}

mod blob_cache_telemetry;
#[cfg(test)]
use blob_cache_telemetry::bounded_counter_log_due;
use blob_cache_telemetry::{
    emit_blob_cache_telemetry, send_final_blob_cache_telemetry, try_emit_blob_cache_telemetry,
};
mod bootstrap;
mod experience;
mod forms;
mod frame_flush;
pub(crate) mod handle_state;
mod latency_reply;
mod outbound;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod packet_delay;
#[cfg(not(target_arch = "wasm32"))]
pub mod packet_log;
use bootstrap::{send_startup_failure, start_game_inventory_authority, start_game_item_registry};
use outbound::*;
mod pump;
use pump::*;
mod pump_runtime;
use pump_runtime::*;
mod terminal;
use terminal::TerminalOwner;
mod disconnect_display;
use disconnect_display::disconnect_display_reason;
#[cfg(test)]
mod tests;
