//! Bevy ownership and presentation hooks for the session domain.

use super::resource_packs::PackApplication;
use bevy::app::{App, Last, MainScheduleOrder};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::{Deref, DerefMut, Res, Resource};
use std::path::PathBuf;

pub use client_session::{
    BatchSendError, NetworkFailureOrigin, PacketSendError, SequencedWorldEvent,
    SessionTransferTarget, WorldIngress, session_failure_display,
};
pub type NetworkControlEvent = client_session::NetworkControlEvent<PackApplication>;

/// Startup snapshot supplied by application composition.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub session_generation: u64,
    pub socket_dir: PathBuf,
    pub display_name: String,
    pub client_blob_cache: protocol::ClientBlobCache,
    pub player_skin: crate::player_skin::LocalPlayerSkin,
    /// Rechecked against live artwork before presentation publication.
    pub actor_artwork: Option<render::ActorArtworkPages>,
    /// The carrier catalog initial server UI resolves against on the worker.
    pub ui_catalog: Option<std::sync::Arc<json_ui::Catalog>>,
}

/// A Bevy resource retaining the domain's exact command and event queues.
#[derive(Resource, Deref, DerefMut)]
pub struct NetworkHandle {
    #[deref]
    inner: client_session::NetworkHandle<PackApplication>,
    #[cfg(feature = "local-mods")]
    core_socket_dir: Option<PathBuf>, // Set only while a joined session is live.
}

impl From<client_session::NetworkHandle<PackApplication>> for NetworkHandle {
    fn from(inner: client_session::NetworkHandle<PackApplication>) -> Self {
        Self {
            inner,
            #[cfg(feature = "local-mods")]
            core_socket_dir: None,
        }
    }
}

impl NetworkHandle {
    #[cfg(feature = "local-mods")]
    pub(crate) fn core_socket_dir(&self) -> Option<&std::path::Path> {
        self.core_socket_dir.as_deref()
    }

    pub fn shutdown(&mut self) {
        #[cfg(feature = "local-mods")]
        {
            self.core_socket_dir = None;
        }
        self.inner.shutdown();
    }

    /// Supplies an idle session until the launcher starts a join.
    pub(crate) fn disconnected() -> Self {
        client_session::NetworkHandle::disconnected().into()
    }

    /// Couples the gameplay outbox epoch to the session's socket-write cancellation fence.
    pub(crate) fn movement_ticker(&self) -> crate::movement::MovementTicker {
        crate::movement::MovementTicker::with_epoch_publisher(self.inner.physics_epoch_publisher())
    }

    /// Provides isolated queues for app adapter tests.
    #[cfg(test)]
    pub(crate) fn stub() -> (Self, tokio::sync::watch::Receiver<u64>) {
        let (handle, epoch) = client_session::NetworkHandle::stub();
        (handle.into(), epoch)
    }

    /// Keeps a bounded test queue open until the caller drops its guard.
    #[cfg(test)]
    pub(crate) fn with_command_capacity(capacity: usize) -> (Self, Box<dyn std::any::Any>) {
        let (handle, guard) = client_session::NetworkHandle::with_command_capacity(capacity);
        (handle.into(), guard)
    }

    /// Lets an app adapter test publish terminal or bootstrap controls.
    #[cfg(test)]
    pub(crate) fn stub_with_control_sender()
    -> (Self, tokio::sync::mpsc::Sender<NetworkControlEvent>) {
        let (handle, sender) = client_session::NetworkHandle::stub_with_control_sender();
        (handle.into(), sender)
    }

    /// Captures outbound packets through the domain's production FIFO.
    #[cfg(test)]
    pub(crate) fn stub_capturing_packets() -> (Self, client_session::CapturedPackets) {
        let (handle, packets) = client_session::NetworkHandle::stub_capturing_packets();
        (handle.into(), packets)
    }
}

/// Runs once per frame after `Last`, where vanilla flushes its batched peer at the end of its
/// update: every packet the frame queued leaves in one batch.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NetworkFrameFlush;

pub(crate) fn configure_network_frame_flush(app: &mut App) {
    app.init_schedule(NetworkFrameFlush);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(Last, NetworkFrameFlush);
    app.add_systems(NetworkFrameFlush, flush_network_frame);
}

fn flush_network_frame(network: Option<Res<NetworkHandle>>) {
    if let Some(network) = network {
        network.flush_frame();
    }
}

/// Starts the domain worker with presentation preparation at its original bootstrap boundary.
pub fn spawn_network(config: NetworkConfig) -> Result<NetworkHandle, std::io::Error> {
    #[cfg(feature = "local-mods")]
    let socket_dir = config.socket_dir.clone();
    let actor_artwork = config.actor_artwork;
    let ui_catalog = config.ui_catalog;
    client_session::spawn_network(
        client_session::NetworkConfig {
            session_generation: config.session_generation,
            socket_dir: config.socket_dir,
            display_name: config.display_name,
            client_blob_cache: config.client_blob_cache,
            player_skin: config.player_skin.to_client_skin(),
            resource_pack_store: super::resource_packs::compile_cache().map(|cache| {
                std::sync::Arc::new(cache.clone())
                    as std::sync::Arc<dyn protocol::ResourcePackStore>
            }),
            physical_memory_bytes: crate::global_resources::memory::physical_bytes(),
        },
        move |preparation, game_data, cancelled| {
            super::resource_packs::prepare_join(
                preparation,
                game_data,
                cancelled,
                super::resource_packs::JoinBases {
                    actor_artwork: actor_artwork.as_ref(),
                    ui_catalog: ui_catalog.as_ref(),
                },
            )
        },
        client_session::SessionTrace {
            movement_line: gameplay::movement::pending_trace_line,
            write_movement: gameplay::movement::write_trace_line,
            packet_trace: emit_packet_trace,
            fast_transfer_action_marker: Some(client_ui::diagnostic_markers::FAST_TRANSFER_ACTION),
        },
    )
    .map(|inner| NetworkHandle {
        inner,
        #[cfg(feature = "local-mods")]
        core_socket_dir: Some(socket_dir),
    })
}

/// Attaches the acceptance-owned marker to the transport's serialized observation.
fn emit_packet_trace(trace: &str) {
    diagnostics::write_stdout_marker(
        &mut diagnostics::console::stdout(),
        &format!(
            "{}={trace}",
            diagnostics::markers::FAST_TRANSFER_PACKET_TRACE
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{InventoryAuthority, InventoryEvent};
    use std::sync::Arc;
    use tokio::sync::mpsc;
    #[path = "language.rs"]
    mod language;
    #[path = "mining_mode.rs"]
    mod mining_mode;
}
