//! Bevy ownership of the session queues and their end-of-frame flush.

use bevy::app::{App, MainScheduleOrder};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::{Deref, DerefMut, Res, Resource};
use std::path::PathBuf;

/// A Bevy resource retaining the domain's exact command and event queues.
#[derive(Resource, Deref, DerefMut)]
pub struct NetworkHandle<P: Send + Sync + 'static> {
    #[deref]
    inner: client_session::NetworkHandle<P>,
    pending_world_ingress: Option<client_session::WorldIngress>,
    core_socket_dir: Option<PathBuf>, // Set only while a joined session is live.
}

impl<P: Send + Sync + 'static> From<client_session::NetworkHandle<P>> for NetworkHandle<P> {
    fn from(inner: client_session::NetworkHandle<P>) -> Self {
        Self {
            inner,
            pending_world_ingress: None,
            core_socket_dir: None,
        }
    }
}

impl<P: Send + Sync + 'static> NetworkHandle<P> {
    /// Borrows the session FIFO and its blocked head, retained until admission reopens.
    pub fn world_ingress_mut(
        &mut self,
    ) -> (
        &mut tokio::sync::mpsc::Receiver<client_session::WorldIngress>,
        &mut Option<client_session::WorldIngress>,
    ) {
        (
            self.inner.world_events_mut(),
            &mut self.pending_world_ingress,
        )
    }

    /// Counts every unconsumed event, including the head retained outside the channel.
    pub fn pending_event_count(&self) -> usize {
        self.inner
            .pending_event_count()
            .saturating_add(usize::from(self.pending_world_ingress.is_some()))
    }

    /// Adopts the domain queues and the endpoint used by this live session.
    pub fn connected(inner: client_session::NetworkHandle<P>, socket_dir: PathBuf) -> Self {
        Self {
            inner,
            pending_world_ingress: None,
            core_socket_dir: Some(socket_dir),
        }
    }

    /// Returns the control endpoint only while this handle belongs to a live session.
    pub fn core_socket_dir(&self) -> Option<&std::path::Path> {
        self.core_socket_dir.as_deref()
    }

    /// Retires the control endpoint and stops the owned network worker.
    pub fn shutdown(&mut self) {
        self.core_socket_dir = None;
        self.inner.shutdown();
    }

    /// Supplies an idle session until the launcher starts a join.
    pub fn disconnected() -> Self {
        client_session::NetworkHandle::disconnected().into()
    }

    /// Provides isolated queues for app adapter tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub() -> (
        Self,
        tokio::sync::watch::Receiver<simulation_clock::identity::PositionAuthorityGeneration>,
    ) {
        let (handle, epoch) = client_session::NetworkHandle::stub();
        (handle.into(), epoch)
    }

    /// Keeps a bounded test queue open until the caller drops its guard.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_command_capacity(capacity: usize) -> (Self, Box<dyn std::any::Any>) {
        let (handle, guard) = client_session::NetworkHandle::with_command_capacity(capacity);
        (handle.into(), guard)
    }

    /// Lets an app adapter test publish terminal or bootstrap controls.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub_with_control_sender() -> (
        Self,
        tokio::sync::mpsc::Sender<client_session::NetworkControlEvent<P>>,
    ) {
        let (handle, sender) = client_session::NetworkHandle::stub_with_control_sender();
        (handle.into(), sender)
    }

    /// Captures outbound packets through the domain's production FIFO.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub_capturing_packets() -> (Self, client_session::CapturedPackets) {
        let (handle, packets) = client_session::NetworkHandle::stub_capturing_packets();
        (handle.into(), packets)
    }
}

/// Submits the current frame's packet batch before waiting for the next frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkFrameFlush;

/// Registers the session flush after ordinary frame work and before the pacing wait.
pub fn configure_network_frame_flush<P: Send + Sync + 'static>(app: &mut App) {
    render::FrameEnd::install(app);
    app.init_schedule(NetworkFrameFlush);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_before(render::FrameEnd, NetworkFrameFlush);
    app.add_systems(NetworkFrameFlush, flush_network_frame::<P>);
}

/// Flushes packets queued during the frame when a network handle is installed.
fn flush_network_frame<P: Send + Sync + 'static>(network: Option<Res<NetworkHandle<P>>>) {
    if let Some(network) = network {
        network.flush_frame();
    }
}

#[cfg(test)]
#[path = "network/tests.rs"]
mod tests;
