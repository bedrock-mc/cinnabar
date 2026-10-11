//! Bevy ownership of the session queues and their end-of-frame flush.

use bevy::app::{App, Last, MainScheduleOrder};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::{Deref, DerefMut, Res, Resource};
#[cfg(feature = "local-mods")]
use std::path::PathBuf;

/// A Bevy resource retaining the domain's exact command and event queues.
#[derive(Resource, Deref, DerefMut)]
pub struct NetworkHandle<P: Send + Sync + 'static> {
    #[deref]
    inner: client_session::NetworkHandle<P>,
    #[cfg(feature = "local-mods")]
    core_socket_dir: Option<PathBuf>, // Set only while a joined session is live.
}

impl<P: Send + Sync + 'static> From<client_session::NetworkHandle<P>> for NetworkHandle<P> {
    fn from(inner: client_session::NetworkHandle<P>) -> Self {
        Self {
            inner,
            #[cfg(feature = "local-mods")]
            core_socket_dir: None,
        }
    }
}

impl<P: Send + Sync + 'static> NetworkHandle<P> {
    /// Adopts the domain queues and the endpoint used by this live session.
    #[cfg(feature = "local-mods")]
    pub fn connected(inner: client_session::NetworkHandle<P>, socket_dir: PathBuf) -> Self {
        Self {
            inner,
            core_socket_dir: Some(socket_dir),
        }
    }

    #[cfg(feature = "local-mods")]
    /// Returns the control endpoint only while this handle belongs to a live session.
    pub fn core_socket_dir(&self) -> Option<&std::path::Path> {
        self.core_socket_dir.as_deref()
    }

    /// Retires the control endpoint and stops the owned network worker.
    pub fn shutdown(&mut self) {
        #[cfg(feature = "local-mods")]
        {
            self.core_socket_dir = None;
        }
        self.inner.shutdown();
    }

    /// Supplies an idle session until the launcher starts a join.
    pub fn disconnected() -> Self {
        client_session::NetworkHandle::disconnected().into()
    }

    /// Provides isolated queues for app adapter tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub() -> (Self, tokio::sync::watch::Receiver<u64>) {
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

/// Runs once per frame after `Last`, where vanilla flushes its batched peer at the end of its
/// update: every packet the frame queued leaves in one batch.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkFrameFlush;

/// Registers the session flush after all ordinary frame schedules.
pub fn configure_network_frame_flush<P: Send + Sync + 'static>(app: &mut App) {
    app.init_schedule(NetworkFrameFlush);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(Last, NetworkFrameFlush);
    app.add_systems(NetworkFrameFlush, flush_network_frame::<P>);
}

/// Flushes packets queued during the frame when a network handle is installed.
fn flush_network_frame<P: Send + Sync + 'static>(network: Option<Res<NetworkHandle<P>>>) {
    if let Some(network) = network {
        network.flush_frame();
    }
}
