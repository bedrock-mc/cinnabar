use std::future::Future;

use protocol::WorldEvent;
use tokio::sync::{mpsc, watch};

use super::{
    InboundWorldEvent, NetworkControlEvent, NetworkSession, SequencedWorldEvent, WorldIngress,
};

pub(super) enum WorldSideWork<'a, E> {
    Event(Result<Box<InboundWorldEvent>, E>),
    Capacity(Result<mpsc::Permit<'a, WorldIngress>, mpsc::error::SendError<()>>),
}

pub(super) async fn wait_for_world_side_work<'a, S: NetworkSession>(
    session: &mut S,
    current_dimension: i32,
    world_event_tx: &'a mpsc::Sender<WorldIngress>,
    has_pending_world_event: bool,
) -> WorldSideWork<'a, S::Error> {
    if has_pending_world_event {
        WorldSideWork::Capacity(world_event_tx.reserve().await)
    } else {
        WorldSideWork::Event(
            session
                .receive_world_ingress(current_dimension)
                .await
                .map(Box::new),
        )
    }
}

pub(super) async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

pub(super) async fn wait_for_login_or_cancel<F>(
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

pub(super) async fn wait_for_send_or_cancel<F>(
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

pub(super) enum NetworkPumpWork<I, C> {
    Shutdown,
    Inbound(I),
    Command(C),
}

#[derive(Clone, Copy)]
pub(super) enum NetworkPumpPreference {
    Inbound,
    Command,
}

pub(super) async fn wait_for_network_work_or_cancel<I, C>(
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

pub(super) async fn send_control_event_or_cancel(
    events: &mpsc::Sender<NetworkControlEvent>,
    shutdown: &mut watch::Receiver<bool>,
    event: NetworkControlEvent,
) -> bool {
    send_event_or_cancel(events, shutdown, event).await
}

#[cfg(test)]
pub(super) async fn send_world_event_or_cancel(
    events: &mpsc::Sender<WorldIngress>,
    shutdown: &mut watch::Receiver<bool>,
    event: SequencedWorldEvent,
) -> bool {
    send_event_or_cancel(events, shutdown, WorldIngress::Event(event)).await
}

pub(super) async fn send_event_or_cancel<T>(
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
pub(super) struct NetworkSequencer {
    session_generation: u64,
    next_sequence: u64,
    current_dimension: i32,
    local_player_runtime_id: u64,
}

impl NetworkSequencer {
    pub(super) fn take_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }

    pub(super) const fn session_generation(&self) -> u64 {
        self.session_generation
    }
    pub(super) const fn new(
        session_generation: u64,
        current_dimension: i32,
        local_player_runtime_id: u64,
    ) -> Self {
        Self {
            session_generation,
            next_sequence: 1,
            current_dimension,
            local_player_runtime_id,
        }
    }

    pub(super) fn wrap_fast_transfer_barrier(&mut self, action_sequence: u64) -> WorldIngress {
        let sequence = self.take_sequence();
        WorldIngress::FastTransferBarrier {
            session_generation: self.session_generation,
            sequence,
            action_sequence,
        }
    }

    pub(super) const fn current_dimension(self) -> i32 {
        self.current_dimension
    }

    pub(super) fn wrap(&mut self, event: WorldEvent) -> SequencedWorldEvent {
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
        let sequence = self.take_sequence();
        if let WorldEvent::ChangeDimension(change) = &event {
            self.current_dimension = change.dimension;
        }
        SequencedWorldEvent {
            session_generation: self.session_generation,
            sequence,
            event,
        }
    }
}
