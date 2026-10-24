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

/// Reads raw core frames while world admission is full, retaining the pending FIFO head.
pub(super) async fn wait_for_world_side_work<'a, S: NetworkSession>(
    session: &mut S,
    current_dimension: i32,
    world_event_tx: &'a mpsc::Sender<WorldIngress>,
    has_pending_world_event: bool,
) -> WorldSideWork<'a, S::Error> {
    if has_pending_world_event {
        let capacity = world_event_tx.reserve();
        tokio::pin!(capacity);
        let permit = tokio::select! {
            biased;
            permit = &mut capacity => permit,
            () = session.read_ahead() => capacity.await,
        };
        WorldSideWork::Capacity(permit)
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

pub(super) enum NetworkPumpWork<I, H> {
    Shutdown,
    Inbound(I),
    Hook(H),
}

/// Outbound hooks win over inbound work, so a trace is armed before its batch is written.
pub(super) async fn wait_for_network_work_or_cancel<I, H>(
    inbound: I,
    hook: H,
    shutdown: &mut watch::Receiver<bool>,
) -> NetworkPumpWork<I::Output, H::Output>
where
    I: Future,
    H: Future,
{
    if *shutdown.borrow() {
        return NetworkPumpWork::Shutdown;
    }
    tokio::select! {
        biased;
        _ = wait_for_shutdown(shutdown) => NetworkPumpWork::Shutdown,
        hook = hook => NetworkPumpWork::Hook(hook),
        inbound = inbound => NetworkPumpWork::Inbound(inbound),
    }
}

pub(super) async fn send_control_event_or_cancel<P>(
    events: &mpsc::Sender<NetworkControlEvent<P>>,
    shutdown: &mut watch::Receiver<bool>,
    event: NetworkControlEvent<P>,
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
    session_generation: simulation_clock::identity::SessionGeneration,
    next_sequence: protocol::identity::FifoSequence,
    current_dimension: i32,
    local_player_runtime_id: u64,
}

impl NetworkSequencer {
    pub(super) fn take_sequence(&mut self) -> protocol::identity::FifoSequence {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.next();
        sequence
    }

    pub(super) const fn session_generation(&self) -> simulation_clock::identity::SessionGeneration {
        self.session_generation
    }
    pub(super) const fn new(
        session_generation: simulation_clock::identity::SessionGeneration,
        current_dimension: i32,
        local_player_runtime_id: u64,
    ) -> Self {
        Self {
            session_generation,
            next_sequence: protocol::identity::FifoSequence::new(1),
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
                    force_local_move: false,
                    player_mode: Some(movement.mode),
                    source_tick: Some(movement.source_tick),
                    interpolation: Default::default(),
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

/// Records which core frame and slot produced `ingress`, once it is sequenced.
pub(super) fn trace_decoded((conn, frame, index): (u64, u64, u32), ingress: &WorldIngress) {
    if let Some(trace) = bridge::latency_trace::active() {
        let (session, fifo) = ingress.identity();
        trace.record(
            bridge::latency_trace::kind::DECODED,
            &[
                conn as i64,
                frame as i64,
                i64::from(index),
                session as i64,
                fifo as i64,
                trace.now(),
            ],
        );
    }
}

/// Publishes `ingress` into the app's world channel, recording its entry at the time publishing
/// began, so the app can never drain it before its recorded enqueue time.
pub(super) fn publish_traced(permit: mpsc::Permit<'_, WorldIngress>, ingress: WorldIngress) {
    let Some(trace) = bridge::latency_trace::active() else {
        permit.send(ingress);
        return;
    };
    let (session, fifo) = ingress.identity();
    let _ = trace.record_admitted(
        bridge::latency_trace::kind::ENQUEUED,
        &[session as i64, fifo as i64],
        || {
            permit.send(ingress);
            Ok::<_, std::convert::Infallible>(())
        },
    );
}
