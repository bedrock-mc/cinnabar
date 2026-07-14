use std::{
    future::Future,
    path::PathBuf,
    thread::{self, JoinHandle},
    time::Instant,
};

use bevy::prelude::Resource;
use protocol::{LoginSequence, Packet, WorldBootstrap, WorldEnvironmentBootstrap, WorldEvent};
use tokio::sync::{mpsc, watch};
use world::ChunkKey;

pub(crate) const WORLD_EVENT_CAPACITY: usize = 32;
const CONTROL_EVENT_CAPACITY: usize = 64;
const COMMAND_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub socket_dir: PathBuf,
    pub display_name: String,
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

    #[cfg(test)]
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
                    LoginSequence::connect(&config.socket_dir, &config.display_name),
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
                if sequencer.should_forward(&event) {
                    pending_world_event = Some(sequencer.wrap(event));
                }
            }
            NetworkPumpWork::Inbound(WorldSideWork::Event(Err(error))) => {
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

    let _ = send_control_event_or_cancel(
        &control_event_tx,
        &mut shutdown_rx,
        NetworkControlEvent::Stopped {
            decode_error_count: session.decode_error_count(),
        },
    )
    .await;
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

    fn should_forward(self, event: &WorldEvent) -> bool {
        !matches!(
            event,
            WorldEvent::MovePlayer(movement)
                if movement.runtime_id != self.local_player_runtime_id
        )
    }

    fn wrap(&mut self, event: WorldEvent) -> SequencedWorldEvent {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        if let WorldEvent::ChangeDimension(change) = &event {
            self.current_dimension = change.dimension;
        }
        SequencedWorldEvent { sequence, event }
    }
}

#[cfg(test)]
mod tests {
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
        COMMAND_CAPACITY, CONTROL_EVENT_CAPACITY, NetworkCommand, NetworkControlEvent,
        NetworkHandle, NetworkPumpPreference, NetworkPumpWork, NetworkSequencer, NetworkSession,
        PacketSendError, SequencedWorldEvent, WORLD_EVENT_CAPACITY, run_network_pump,
        send_control_event_or_cancel, send_event_or_cancel, send_world_event_or_cancel,
        wait_for_login_or_cancel, wait_for_network_work_or_cancel, wait_for_send_or_cancel,
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
    fn only_the_local_players_movement_reaches_the_camera_stream() {
        let sequencer = NetworkSequencer::new(0, 42);
        let movement = |runtime_id| {
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id,
                position: [1.0, 64.0, 2.0],
                pitch: 5.0,
                yaw: 90.0,
            })
        };

        assert!(sequencer.should_forward(&movement(42)));
        assert!(!sequencer.should_forward(&movement(7)));
    }

    #[test]
    fn server_authoritative_correction_bypasses_foreign_player_runtime_filter() {
        let sequencer = NetworkSequencer::new(0, 42);
        let correction = WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
            position: [27.5, 111.0, 91.5],
            delta: [0.0; 3],
            pitch: -15.0,
            yaw: 90.0,
            on_ground: true,
            tick: 55,
        });

        assert!(sequencer.should_forward(&correction));
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

        let acknowledgement = tokio::time::timeout(
            Duration::from_millis(100),
            control_events.recv(),
        )
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
            day_cycle_stop_time: 18_000,
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
}
