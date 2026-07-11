use std::{
    future::Future,
    path::PathBuf,
    thread::{self, JoinHandle},
    time::Instant,
};

use bevy::prelude::Resource;
use protocol::{LoginSequence, Packet, WorldBootstrap, WorldEvent};
use tokio::sync::{mpsc, watch};
use world::ChunkKey;

const WORLD_EVENT_CAPACITY: usize = 4;
const COMMAND_CAPACITY: usize = 64;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub socket_dir: PathBuf,
    pub display_name: String,
}

#[derive(Debug)]
pub enum NetworkEvent {
    Bootstrap(WorldBootstrap),
    World(SequencedWorldEvent),
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
    events: mpsc::Receiver<NetworkEvent>,
    commands: mpsc::Sender<NetworkCommand>,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

impl NetworkHandle {
    pub fn events_mut(&mut self) -> &mut mpsc::Receiver<NetworkEvent> {
        &mut self.events
    }

    #[must_use]
    pub fn pending_event_count(&self) -> usize {
        self.events.len()
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
    let (event_tx, events) = mpsc::channel(WORLD_EVENT_CAPACITY);
    let (commands, mut command_rx) = mpsc::channel(COMMAND_CAPACITY);
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
                    let _ = event_tx.try_send(NetworkEvent::Failed {
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
                let (mut session, game_data) = match login {
                    Ok(connected) => connected,
                    Err(error) => {
                        let _ = send_event_or_cancel(
                            &event_tx,
                            &mut shutdown_rx,
                            NetworkEvent::Failed {
                                message: error.to_string(),
                                decode_error_count: 0,
                            },
                        )
                        .await;
                        return;
                    }
                };
                let bootstrap = WorldBootstrap::from_game_data(&game_data);
                if !send_event_or_cancel(
                    &event_tx,
                    &mut shutdown_rx,
                    NetworkEvent::Bootstrap(bootstrap),
                )
                .await
                {
                    return;
                }
                let mut sequencer =
                    NetworkSequencer::new(bootstrap.dimension, bootstrap.local_player_runtime_id);

                loop {
                    match wait_for_network_work_or_cancel(
                        session.recv_world_event(sequencer.current_dimension()),
                        command_rx.recv(),
                        &mut shutdown_rx,
                    )
                    .await
                    {
                        NetworkPumpWork::Shutdown => break,
                        NetworkPumpWork::Command(command) => match command {
                            Some(NetworkCommand::Send { packet, sub_chunk }) => {
                                match wait_for_send_or_cancel(
                                    session.send(packet),
                                    &mut shutdown_rx,
                                )
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
                                            if !send_event_or_cancel(
                                                &event_tx,
                                                &mut shutdown_rx,
                                                NetworkEvent::SubChunkRequestSent {
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
                                        let _ = send_event_or_cancel(
                                            &event_tx,
                                            &mut shutdown_rx,
                                            NetworkEvent::Failed {
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
                        NetworkPumpWork::Inbound(event) => match event {
                            Ok(event) => {
                                if !sequencer.should_forward(&event) {
                                    continue;
                                }
                                let event = sequencer.wrap(event);
                                if !send_event_or_cancel(
                                    &event_tx,
                                    &mut shutdown_rx,
                                    NetworkEvent::World(event),
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            Err(error) => {
                                let _ = send_event_or_cancel(
                                    &event_tx,
                                    &mut shutdown_rx,
                                    NetworkEvent::Failed {
                                        message: error.to_string(),
                                        decode_error_count: session.decode_error_count(),
                                    },
                                )
                                .await;
                                return;
                            }
                        },
                    }
                }
                let _ = send_event_or_cancel(
                    &event_tx,
                    &mut shutdown_rx,
                    NetworkEvent::Stopped {
                        decode_error_count: session.decode_error_count(),
                    },
                )
                .await;
            });
        })?;
    Ok(NetworkHandle {
        events,
        commands,
        shutdown,
        thread: Some(thread),
    })
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

async fn wait_for_network_work_or_cancel<I, C>(
    inbound: I,
    command: C,
    shutdown: &mut watch::Receiver<bool>,
) -> NetworkPumpWork<I::Output, C::Output>
where
    I: Future,
    C: Future,
{
    if *shutdown.borrow() {
        return NetworkPumpWork::Shutdown;
    }
    tokio::select! {
        biased;
        _ = wait_for_shutdown(shutdown) => NetworkPumpWork::Shutdown,
        inbound = inbound => NetworkPumpWork::Inbound(inbound),
        command = command => NetworkPumpWork::Command(command),
    }
}

async fn send_event_or_cancel(
    events: &mpsc::Sender<NetworkEvent>,
    shutdown: &mut watch::Receiver<bool>,
    event: NetworkEvent,
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
        future, thread,
        time::{Duration, Instant},
    };

    use protocol::{ChangeDimensionEvent, MovePlayerEvent, WorldEvent};
    use tokio::sync::{mpsc, oneshot, watch};

    use super::{
        COMMAND_CAPACITY, NetworkCommand, NetworkEvent, NetworkHandle, NetworkPumpWork,
        NetworkSequencer, PacketSendError, send_event_or_cancel, wait_for_login_or_cancel,
        wait_for_network_work_or_cancel, wait_for_send_or_cancel,
    };

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

    #[tokio::test]
    async fn saturated_event_queue_is_cancelled_without_waiting_for_capacity() {
        let (events, mut event_rx) = mpsc::channel(1);
        events
            .send(NetworkEvent::Stopped {
                decode_error_count: 1,
            })
            .await
            .unwrap();
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        shutdown.send_replace(true);

        let delivered = send_event_or_cancel(
            &events,
            &mut shutdown_rx,
            NetworkEvent::Stopped {
                decode_error_count: 2,
            },
        )
        .await;

        assert!(!delivered);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(NetworkEvent::Stopped {
                decode_error_count: 1
            })
        ));
        assert!(event_rx.try_recv().is_err());
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
    async fn network_pump_prioritizes_ready_inbound_and_preserves_command_fifo() {
        let (_shutdown, mut shutdown_rx) = watch::channel(false);
        let (commands, mut command_rx) = mpsc::channel(2);
        commands.try_send(10).unwrap();
        commands.try_send(20).unwrap();

        let selected = wait_for_network_work_or_cancel(
            future::ready("inbound"),
            command_rx.recv(),
            &mut shutdown_rx,
        )
        .await;
        assert!(matches!(selected, NetworkPumpWork::Inbound("inbound")));
        assert_eq!(command_rx.len(), 2);

        let first_command = wait_for_network_work_or_cancel(
            future::pending::<&'static str>(),
            command_rx.recv(),
            &mut shutdown_rx,
        )
        .await;
        assert!(matches!(first_command, NetworkPumpWork::Command(Some(10))));
        let second_command = wait_for_network_work_or_cancel(
            future::pending::<&'static str>(),
            command_rx.recv(),
            &mut shutdown_rx,
        )
        .await;
        assert!(matches!(second_command, NetworkPumpWork::Command(Some(20))));
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
        let (event_tx, events) = mpsc::channel(1);
        drop(event_tx);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        let worker = thread::spawn(|| thread::sleep(Duration::from_millis(250)));
        let mut handle = NetworkHandle {
            events,
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
        let (event_tx, events) = mpsc::channel(2);
        let (commands, mut command_rx) = mpsc::channel(2);
        let (shutdown, _shutdown_rx) = watch::channel(false);
        let mut handle = NetworkHandle {
            events,
            commands,
            shutdown,
            thread: None,
        };

        assert_eq!(handle.pending_event_count(), 0);
        assert_eq!(handle.pending_command_count(), 0);
        event_tx
            .try_send(NetworkEvent::Stopped {
                decode_error_count: 0,
            })
            .unwrap();
        assert_eq!(handle.pending_event_count(), 1);
        handle.events_mut().try_recv().unwrap();
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
