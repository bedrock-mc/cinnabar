use std::{
    future::Future,
    path::PathBuf,
    thread::{self, JoinHandle},
    time::Instant,
};

use bevy::prelude::Resource;
use protocol::{
    BlobCacheStats, ClientBlobCache, LoginSequence, Packet, WorldBootstrap,
    WorldEnvironmentBootstrap, WorldEvent,
};
use tokio::sync::{mpsc, watch};
use world::ChunkKey;

pub(crate) const WORLD_EVENT_CAPACITY: usize = 32;
const CONTROL_EVENT_CAPACITY: usize = 64;
const COMMAND_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub socket_dir: PathBuf,
    pub display_name: String,
    /// Verified blobs outlive a Play session; each login creates a fresh resolver around this cache.
    pub client_blob_cache: ClientBlobCache,
}

#[derive(Debug)]
pub enum NetworkControlEvent {
    Bootstrap {
        world: WorldBootstrap,
        environment: WorldEnvironmentBootstrap,
    },
    SubChunkRequestSent {
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
        sent_at: Instant,
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
    pub sequence: u64,
    pub event: WorldEvent,
}

#[derive(Debug)]
enum NetworkCommand {
    Send {
        packet: Packet,
        sub_chunk: Option<SubChunkRequestSend>,
    },
}

#[derive(Debug, Clone, Copy)]
struct SubChunkRequestSend {
    chunk: ChunkKey,
    base_sub_chunk_y: i32,
    count: usize,
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
    world_events: mpsc::Receiver<SequencedWorldEvent>,
    commands: mpsc::Sender<NetworkCommand>,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

impl NetworkHandle {
    pub fn control_events_mut(&mut self) -> &mut mpsc::Receiver<NetworkControlEvent> {
        &mut self.control_events
    }

    pub fn world_events_mut(&mut self) -> &mut mpsc::Receiver<SequencedWorldEvent> {
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

    pub fn send_packet(&self, packet: Packet) -> Result<(), PacketSendError> {
        self.send_packet_with_confirmation(packet, None)
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
        )
    }

    fn send_packet_with_confirmation(
        &self,
        packet: Packet,
        sub_chunk: Option<SubChunkRequestSend>,
    ) -> Result<(), PacketSendError> {
        self.commands
            .try_send(NetworkCommand::Send { packet, sub_chunk })
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

impl Drop for NetworkHandle {
    fn drop(&mut self) {
        self.shutdown.send_replace(true);
        self.release_thread();
    }
}

pub fn spawn_network(config: NetworkConfig) -> Result<NetworkHandle, std::io::Error> {
    let (control_event_tx, control_events) = mpsc::channel(CONTROL_EVENT_CAPACITY);
    let (world_event_tx, world_events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (shutdown, mut shutdown_rx) = watch::channel(false);
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
                if !send_control_event_or_cancel(
                    &control_event_tx,
                    &mut shutdown_rx,
                    NetworkControlEvent::Bootstrap {
                        world: bootstrap,
                        environment,
                    },
                )
                .await
                {
                    return;
                }
                let sequencer =
                    NetworkSequencer::new(bootstrap.dimension, bootstrap.local_player_runtime_id);
                run_network_pump(
                    session,
                    sequencer,
                    command_rx,
                    control_event_tx,
                    world_event_tx,
                    shutdown_rx,
                )
                .await;
            });
        })?;
    Ok(NetworkHandle {
        control_events,
        world_events,
        commands,
        shutdown,
        thread: Some(thread),
    })
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
}

async fn run_network_pump<S: NetworkSession>(
    mut session: S,
    mut sequencer: NetworkSequencer,
    mut command_rx: mpsc::Receiver<NetworkCommand>,
    control_event_tx: mpsc::Sender<NetworkControlEvent>,
    world_event_tx: mpsc::Sender<SequencedWorldEvent>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut pump_preference = NetworkPumpPreference::Inbound;
    let mut pending_world_event = None;
    let mut last_blob_cache_stats = None;
    if session.blob_cache_enabled() {
        let stats = session.blob_cache_stats();
        emit_blob_cache_telemetry(stats);
        if !send_control_event_or_cancel(
            &control_event_tx,
            &mut shutdown_rx,
            NetworkControlEvent::BlobCacheTelemetry {
                enabled: true,
                stats,
            },
        )
        .await
        {
            return;
        }
        last_blob_cache_stats = Some(stats);
    }

    loop {
        match wait_for_network_work_or_cancel(
            wait_for_world_side_work(
                &mut session,
                sequencer.current_dimension(),
                &world_event_tx,
                pending_world_event.is_some(),
            ),
            command_rx.recv(),
            &mut shutdown_rx,
            &mut pump_preference,
        )
        .await
        {
            NetworkPumpWork::Shutdown => break,
            NetworkPumpWork::Command(command) => match command {
                Some(NetworkCommand::Send { packet, sub_chunk }) => {
                    match wait_for_send_or_cancel(session.send_packet(packet), &mut shutdown_rx)
                        .await
                    {
                        None => {
                            if *shutdown_rx.borrow() {
                                break;
                            }
                        }
                        Some(Ok(())) => {
                            if let Some(sub_chunk) = sub_chunk {
                                let sent_at = Instant::now();
                                if !send_control_event_or_cancel(
                                    &control_event_tx,
                                    &mut shutdown_rx,
                                    NetworkControlEvent::SubChunkRequestSent {
                                        chunk: sub_chunk.chunk,
                                        base_sub_chunk_y: sub_chunk.base_sub_chunk_y,
                                        count: sub_chunk.count,
                                        sent_at,
                                    },
                                )
                                .await
                                {
                                    return;
                                }
                            }
                        }
                        Some(Err(error)) => {
                            send_final_blob_cache_telemetry(
                                &session,
                                &control_event_tx,
                                &mut shutdown_rx,
                            )
                            .await;
                            let _ = send_control_event_or_cancel(
                                &control_event_tx,
                                &mut shutdown_rx,
                                NetworkControlEvent::Failed {
                                    message: error.to_string(),
                                    decode_error_count: session.decode_error_count(),
                                },
                            )
                            .await;
                            return;
                        }
                    }
                }
                None => break,
            },
            NetworkPumpWork::Inbound(WorldSideWork::Capacity(Ok(permit))) => {
                permit.send(
                    pending_world_event
                        .take()
                        .expect("world capacity is reserved only for a pending event"),
                );
            }
            NetworkPumpWork::Inbound(WorldSideWork::Capacity(Err(_))) => return,
            NetworkPumpWork::Inbound(WorldSideWork::Event(Ok(event))) => {
                try_emit_blob_cache_telemetry(
                    &session,
                    &control_event_tx,
                    &mut last_blob_cache_stats,
                );
                pending_world_event = Some(sequencer.wrap(event));
            }
            NetworkPumpWork::Inbound(WorldSideWork::Event(Err(error))) => {
                send_final_blob_cache_telemetry(&session, &control_event_tx, &mut shutdown_rx)
                    .await;
                let _ = send_control_event_or_cancel(
                    &control_event_tx,
                    &mut shutdown_rx,
                    NetworkControlEvent::Failed {
                        message: error.to_string(),
                        decode_error_count: session.decode_error_count(),
                    },
                )
                .await;
                return;
            }
        }
    }

    send_final_blob_cache_telemetry(&session, &control_event_tx, &mut shutdown_rx).await;
    let _ = send_control_event_or_cancel(
        &control_event_tx,
        &mut shutdown_rx,
        NetworkControlEvent::Stopped {
            decode_error_count: session.decode_error_count(),
        },
    )
    .await;
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
        emit_blob_cache_telemetry(stats);
        *last_stats = Some(stats);
    }
}

async fn send_final_blob_cache_telemetry<S: NetworkSession>(
    session: &S,
    control_event_tx: &mpsc::Sender<NetworkControlEvent>,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    if !session.blob_cache_enabled() {
        return;
    }
    let stats = session.blob_cache_stats();
    emit_blob_cache_telemetry(stats);
    let _ = send_control_event_or_cancel(
        control_event_tx,
        shutdown_rx,
        NetworkControlEvent::BlobCacheTelemetry {
            enabled: true,
            stats,
        },
    )
    .await;
}

fn emit_blob_cache_telemetry(stats: BlobCacheStats) {
    bevy::log::info!(
        target: "bedrock_client::blob_cache",
        hashes_classified = stats.hashes_classified,
        hits = stats.hits,
        misses = stats.misses,
        admitted_blobs = stats.admitted_blobs,
        rejected_blobs = stats.rejected_blobs,
        evictions = stats.evictions,
        pending_transactions = stats.pending_transactions,
        pending_bytes = stats.pending_bytes,
        pending_resets = stats.pending_resets,
        reconstructed_level_chunks = stats.reconstructed_level_chunks,
        reconstructed_sub_chunks = stats.reconstructed_sub_chunks,
        "client blob cache counters"
    );
}

enum WorldSideWork<'a, E> {
    Event(Result<WorldEvent, E>),
    Capacity(Result<mpsc::Permit<'a, SequencedWorldEvent>, mpsc::error::SendError<()>>),
}

async fn wait_for_world_side_work<'a, S: NetworkSession>(
    session: &mut S,
    current_dimension: i32,
    world_event_tx: &'a mpsc::Sender<SequencedWorldEvent>,
    has_pending_world_event: bool,
) -> WorldSideWork<'a, S::Error> {
    if has_pending_world_event {
        WorldSideWork::Capacity(world_event_tx.reserve().await)
    } else {
        WorldSideWork::Event(session.receive_world_event(current_dimension).await)
    }
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

async fn wait_for_login_or_cancel<F>(
    login: F,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<F::Output>
where
    F: Future,
{
    if *shutdown.borrow() {
        return None;
    }
    tokio::select! {
        biased;
        _ = wait_for_shutdown(shutdown) => None,
        result = login => Some(result),
    }
}

async fn wait_for_send_or_cancel<F>(
    send: F,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<F::Output>
where
    F: Future,
{
    if *shutdown.borrow() {
        return None;
    }
    tokio::select! {
        biased;
        _ = wait_for_shutdown(shutdown) => None,
        result = send => Some(result),
    }
}

enum NetworkPumpWork<I, C> {
    Shutdown,
    Inbound(I),
    Command(C),
}

#[derive(Clone, Copy)]
enum NetworkPumpPreference {
    Inbound,
    Command,
}

async fn wait_for_network_work_or_cancel<I, C>(
    inbound: I,
    command: C,
    shutdown: &mut watch::Receiver<bool>,
    preference: &mut NetworkPumpPreference,
) -> NetworkPumpWork<I::Output, C::Output>
where
    I: Future,
    C: Future,
{
    if *shutdown.borrow() {
        return NetworkPumpWork::Shutdown;
    }
    let work = match preference {
        NetworkPumpPreference::Inbound => tokio::select! {
            biased;
            _ = wait_for_shutdown(shutdown) => NetworkPumpWork::Shutdown,
            inbound = inbound => NetworkPumpWork::Inbound(inbound),
            command = command => NetworkPumpWork::Command(command),
        },
        NetworkPumpPreference::Command => tokio::select! {
            biased;
            _ = wait_for_shutdown(shutdown) => NetworkPumpWork::Shutdown,
            command = command => NetworkPumpWork::Command(command),
            inbound = inbound => NetworkPumpWork::Inbound(inbound),
        },
    };
    match &work {
        NetworkPumpWork::Shutdown => {}
        NetworkPumpWork::Inbound(_) => *preference = NetworkPumpPreference::Command,
        NetworkPumpWork::Command(_) => *preference = NetworkPumpPreference::Inbound,
    }
    work
}

async fn send_control_event_or_cancel(
    events: &mpsc::Sender<NetworkControlEvent>,
    shutdown: &mut watch::Receiver<bool>,
    event: NetworkControlEvent,
) -> bool {
    send_event_or_cancel(events, shutdown, event).await
}

#[cfg(test)]
async fn send_world_event_or_cancel(
    events: &mpsc::Sender<SequencedWorldEvent>,
    shutdown: &mut watch::Receiver<bool>,
    event: SequencedWorldEvent,
) -> bool {
    send_event_or_cancel(events, shutdown, event).await
}

async fn send_event_or_cancel<T>(
    events: &mpsc::Sender<T>,
    shutdown: &mut watch::Receiver<bool>,
    event: T,
) -> bool {
    if *shutdown.borrow() {
        return false;
    }
    tokio::select! {
        biased;
        _ = wait_for_shutdown(shutdown) => false,
        result = events.send(event) => result.is_ok(),
    }
}

#[derive(Debug, Clone, Copy)]
struct NetworkSequencer {
    next_sequence: u64,
    current_dimension: i32,
    local_player_runtime_id: u64,
}

impl NetworkSequencer {
    const fn new(current_dimension: i32, local_player_runtime_id: u64) -> Self {
        Self {
            next_sequence: 1,
            current_dimension,
            local_player_runtime_id,
        }
    }

    const fn current_dimension(self) -> i32 {
        self.current_dimension
    }

    fn wrap(&mut self, event: WorldEvent) -> SequencedWorldEvent {
        let event = match event {
            WorldEvent::MovePlayer(movement)
                if movement.runtime_id != self.local_player_runtime_id =>
            {
                WorldEvent::Actor(protocol::ActorEvent::Move(protocol::ActorMoveEvent {
                    dimension: self.current_dimension,
                    runtime_id: movement.runtime_id,
                    position: movement.position.map(Some),
                    position_origin: protocol::ActorPositionOrigin::NetworkOffset,
                    pitch: Some(movement.pitch),
                    yaw: Some(movement.yaw),
                    head_yaw: Some(movement.head_yaw),
                    on_ground: Some(movement.on_ground),
                    teleported: movement.teleported,
                    player_mode: Some(movement.mode),
                    source_tick: Some(movement.source_tick),
                }))
            }
            event => event,
        };
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        if let WorldEvent::ChangeDimension(change) = &event {
            self.current_dimension = change.dimension;
        }
        SequencedWorldEvent { sequence, event }
    }
}

#[cfg(test)]
mod tests;
