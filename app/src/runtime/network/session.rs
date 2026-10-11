//! Bevy ownership and presentation hooks for the session domain.

use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use std::path::PathBuf;

pub type NetworkControlEvent = client_session::NetworkControlEvent<PackApplication>;

/// Startup snapshot supplied by application composition.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub session_generation: u64,
    pub socket_dir: PathBuf,
    pub display_name: String,
    /// The user storage owner's opaque persisted login identifier.
    pub client_random_id: protocol::ClientRandomId,
    pub client_blob_cache: protocol::ClientBlobCache,
    pub player_skin: client_shell::player_skin::LocalPlayerSkin,
    /// The player's language, input mode and GUI scale, reported at login.
    pub login_settings: protocol::LoginSettings,
    /// Rechecked against live artwork before presentation publication.
    pub actor_artwork: Option<render::ActorArtworkPages>,
    /// The carrier catalog initial server UI resolves against on the worker.
    pub ui_catalog: Option<std::sync::Arc<json_ui::Catalog>>,
}

/// Starts the domain worker with presentation preparation at its original bootstrap boundary.
pub fn spawn_network(
    config: NetworkConfig,
) -> Result<NetworkHandle<PackApplication>, std::io::Error> {
    #[cfg(feature = "local-mods")]
    let socket_dir = config.socket_dir.clone();
    let actor_artwork = config.actor_artwork;
    let ui_catalog = config.ui_catalog;
    let physical_memory_bytes = launcher_host::memory::physical_bytes();
    client_session::spawn_network(
        client_session::NetworkConfig {
            session_generation: config.session_generation,
            socket_dir: config.socket_dir,
            display_name: config.display_name,
            client_random_id: config.client_random_id,
            client_blob_cache: config.client_blob_cache,
            player_skin: config.player_skin.to_client_skin(),
            login_settings: config.login_settings,
        },
        move |handoff, game_data, cancelled| {
            let preparation =
                pack_runtime::prepare_session_packs(handoff, game_data, physical_memory_bytes);
            if cancelled() {
                return None;
            }
            super::resource_packs::prepare_join(
                &preparation,
                game_data,
                cancelled,
                super::resource_packs::JoinBases {
                    actor_artwork: actor_artwork.as_ref(),
                    ui_catalog: ui_catalog.as_ref(),
                },
            )
            .map(|result| {
                result
                    .map(|packs| client_session::PreparedSession {
                        custom_blocks: preparation.inputs.blocks.clone(),
                        packs_applied: preparation.has_applied_packs(),
                        packs,
                    })
                    .map_err(|error| error.to_string())
            })
        },
        client_session::SessionTrace {
            movement_line: gameplay::movement::pending_trace_line,
            write_movement: gameplay::movement::write_trace_line,
            packet_trace: emit_packet_trace,
            fast_transfer_action_marker: Some(client_ui::diagnostic_markers::FAST_TRANSFER_ACTION),
        },
    )
    .map(|inner| {
        #[cfg(feature = "local-mods")]
        {
            NetworkHandle::connected(inner, socket_dir)
        }
        #[cfg(not(feature = "local-mods"))]
        {
            inner.into()
        }
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
    use client_session::{NetworkFailureOrigin, SessionTransferTarget};
    use protocol::{InventoryAuthority, InventoryEvent};
    use std::sync::Arc;
    use tokio::sync::mpsc;
    #[path = "language.rs"]
    mod language;
    #[path = "mining_mode.rs"]
    mod mining_mode;
}
