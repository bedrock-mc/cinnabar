//! Bevy ownership and presentation hooks for the session domain.

use super::resource_packs::PackApplication;
use bevy::prelude::{Deref, DerefMut, Resource};
use std::path::PathBuf;

pub use client_session::{
    BatchSendError, NetworkFailureOrigin, PacketSendError, SequencedWorldEvent,
    SessionTransferTarget, WORLD_EVENT_CAPACITY, WorldIngress, session_failure_display,
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
}

/// A Bevy resource retaining the domain's exact command and event queues.
#[derive(Resource, Deref, DerefMut)]
pub struct NetworkHandle(client_session::NetworkHandle<PackApplication>);

impl NetworkHandle {
    /// Supplies an idle session until the launcher starts a join.
    pub(crate) fn disconnected() -> Self {
        Self(client_session::NetworkHandle::disconnected())
    }

    /// Couples the gameplay outbox epoch to the session's socket-write cancellation fence.
    pub(crate) fn movement_ticker(&self) -> crate::movement::MovementTicker {
        crate::movement::MovementTicker::with_epoch_publisher(self.0.physics_epoch_publisher())
    }

    /// Provides isolated queues for app adapter tests.
    #[cfg(test)]
    pub(crate) fn stub() -> (Self, tokio::sync::watch::Receiver<u64>) {
        let (handle, epoch) = client_session::NetworkHandle::stub();
        (Self(handle), epoch)
    }

    /// Keeps a bounded test queue open until the caller drops its guard.
    #[cfg(test)]
    pub(crate) fn with_command_capacity(capacity: usize) -> (Self, Box<dyn std::any::Any>) {
        let (handle, guard) = client_session::NetworkHandle::with_command_capacity(capacity);
        (Self(handle), guard)
    }

    /// Lets an app adapter test publish terminal or bootstrap controls.
    #[cfg(test)]
    pub(crate) fn stub_with_control_sender()
    -> (Self, tokio::sync::mpsc::Sender<NetworkControlEvent>) {
        let (handle, sender) = client_session::NetworkHandle::stub_with_control_sender();
        (Self(handle), sender)
    }

    /// Captures outbound packets through the domain's production FIFO.
    #[cfg(test)]
    pub(crate) fn stub_capturing_packets() -> (Self, client_session::CapturedPackets) {
        let (handle, packets) = client_session::NetworkHandle::stub_capturing_packets();
        (Self(handle), packets)
    }
}

/// Starts the domain worker with presentation preparation at its original bootstrap boundary.
pub fn spawn_network(config: NetworkConfig) -> Result<NetworkHandle, std::io::Error> {
    let actor_artwork = config.actor_artwork;
    client_session::spawn_network(
        client_session::NetworkConfig {
            session_generation: config.session_generation,
            socket_dir: config.socket_dir,
            display_name: config.display_name,
            client_blob_cache: config.client_blob_cache,
            player_skin: config.player_skin.to_client_skin(),
        },
        move |preparation, game_data| {
            let mut packs =
                super::resource_packs::prepare_session_presentation(preparation, game_data)?;
            packs.prepare_actor_artwork(actor_artwork.as_ref());
            Ok(packs)
        },
        client_session::SessionTrace {
            movement_line: crate::movement::pending_trace_line,
            write_movement: crate::movement::write_trace_line,
            packet_trace: emit_packet_trace,
            fast_transfer_action_marker: Some(client_ui::diagnostic_markers::FAST_TRANSFER_ACTION),
        },
    )
    .map(NetworkHandle)
}

/// Attaches the acceptance-owned marker to the transport's serialized observation.
fn emit_packet_trace(trace: &str) {
    crate::acceptance::mutation::write_stdout_marker(
        &mut std::io::stdout().lock(),
        &format!(
            "{}={trace}",
            crate::acceptance::markers::FAST_TRANSFER_PACKET_TRACE
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
