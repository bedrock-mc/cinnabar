//! Bevy ownership and presentation hooks for the session domain.

use crate::runtime::network::resource_packs::PackApplication;
#[cfg(not(target_arch = "wasm32"))]
use client_runtime::network::NetworkHandle;
#[cfg(not(target_arch = "wasm32"))]
use std::ffi::OsStr;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

#[cfg(feature = "native-networking")]
pub(crate) mod friend;
#[cfg(feature = "native-networking")]
pub(crate) mod local_world;
mod preparation;
pub(crate) use preparation::Preparation;

pub type NetworkControlEvent = client_session::NetworkControlEvent<PackApplication>;

/// Startup snapshot supplied by application composition.
#[cfg(not(target_arch = "wasm32"))]
pub struct NetworkConfig {
    /// Immutable world and collision inputs captured for this join.
    pub(crate) registry_base: super::session_registries::RegistryBase,
    pub session_generation: simulation_clock::identity::SessionGeneration,
    pub endpoint: protocol::SessionEndpoint,
    /// Cached account owner for an explicitly authenticated native join.
    #[cfg(feature = "native-networking")]
    pub native_account: Option<std::sync::Arc<launcher_host::native_account::NativeAccount>>,
    /// The original selected friend owner and bounded cleanup reservation.
    #[cfg(feature = "native-networking")]
    pub native_friend: Option<friend::FriendConnectionRequest>,
    /// Original account-bound featured selection; allocation is explicit and never inferred from its card.
    #[cfg(feature = "native-networking")]
    pub native_gathering: Option<launcher_host::native_account::NativeGatheringSelection>,
    /// Original local process selection and its optional selected account.
    #[cfg(feature = "native-networking")]
    pub native_local_world: Option<local_world::WorldConnectionRequest>,
    pub display_name: String,
    /// The user storage owner's opaque persisted login identifier.
    pub client_random_id: protocol::ClientRandomId,
    pub client_blob_cache: protocol::ClientBlobCache,
    pub player_skin: client_shell::player_skin::LocalPlayerSkin,
    /// The player's language, input mode and GUI scale, reported at login.
    pub login_settings: protocol::LoginSettings,
    /// Rechecked against live artwork before presentation publication.
    pub actor_artwork: Option<render_model::ActorArtworkPages>,
    /// The carrier catalog initial server UI resolves against on the worker.
    pub ui_catalog: Option<std::sync::Arc<json_ui::Catalog>>,
    /// Where an opted-in packet capture goes by default.
    pub log_dir: PathBuf,
    /// The Record network session setting as the session starts.
    pub record_session: bool,
}

/// Starts the domain worker with presentation preparation at its original bootstrap boundary.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_network(
    config: NetworkConfig,
    runtime: tokio::runtime::Handle,
    tasks: tokio_util::task::TaskTracker,
    packet_relay: Option<std::sync::Arc<dyn protocol::PacketRelay>>,
) -> Result<NetworkHandle<PackApplication>, std::io::Error> {
    if bridge::join_profile::JoinProfile::current().id().is_none() {
        bridge::join_profile::JoinProfile::begin();
    }
    let socket_dir = config.endpoint.core_directory().map(Path::to_path_buf);
    #[cfg(feature = "native-networking")]
    if matches!(&config.endpoint, protocol::SessionEndpoint::Native(target)
        if launcher_host::native_account::is_realm_target(&target.address)
            || protocol::NativeRawTarget::recognizes(&target.address)
            || matches!(launcher_host::launcher_core::target_for(&target.address), bridge::ConnectTarget::Gathering(_)))
        && config.native_account.is_none()
        && config.native_gathering.is_none()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "joining this service requires a verified native account",
        ));
    }
    let player_skin = config.player_skin.to_client_skin();
    #[cfg(feature = "native-networking")]
    if usize::from(config.native_account.is_some())
        + usize::from(config.native_friend.is_some())
        + usize::from(config.native_gathering.is_some())
        + usize::from(config.native_local_world.is_some())
        > 1
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "native connection has conflicting account owners",
        ));
    }
    #[cfg(feature = "native-networking")]
    let friend = config.native_friend.map(|request| {
        request.into_companion(
            protocol::login_client_data(
                &config.display_name,
                config.client_random_id,
                Some(&player_skin),
                &config.login_settings,
            ),
            config.client_random_id,
        )
    });
    #[cfg(feature = "native-networking")]
    let local_world = config.native_local_world.map(|request| {
        request.into_companion(
            protocol::login_client_data(
                &config.display_name,
                config.client_random_id,
                Some(&player_skin),
                &config.login_settings,
            ),
            config.client_random_id,
        )
    });
    #[cfg(feature = "native-networking")]
    let native_preparation = if config.native_account.is_some() || config.native_gathering.is_some()
    {
        if !matches!(&config.endpoint, protocol::SessionEndpoint::Native(target) if matches!(target.transport, protocol::NativeTransport::RakNet | protocol::NativeTransport::Addressed | protocol::NativeTransport::NetherNetLan))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "selected native transport does not support account preparation",
            ));
        }
        let protocol::SessionEndpoint::Native(target) = &config.endpoint else {
            unreachable!("checked native endpoint")
        };
        let target = target.clone();
        let skin = player_skin.clone();
        let settings = config.login_settings.clone();
        let display_name = config.display_name.clone();
        let client_random_id = config.client_random_id;
        Some(Box::pin(async move {
            let claims = protocol::login_client_data(
                &display_name,
                client_random_id,
                Some(&skin),
                &settings,
            );
            if let Some(selection) = config.native_gathering {
                selection.prepare(claims, client_random_id).await
            } else {
                config
                    .native_account
                    .expect("selected account preparation")
                    .prepare_connection(target, claims, client_random_id)
                    .await
            }
        }) as client_session::NativeConnectionPreparation)
    } else {
        None
    };
    let preparation = Preparation {
        registry_base: config.registry_base,
        session_generation: config.session_generation,
        actor_artwork: config.actor_artwork,
        ui_catalog: config.ui_catalog,
        physical_memory_bytes: launcher_host::memory::physical_bytes(),
    };
    let mut domain = client_session::NetworkConfig::new(
        config.session_generation,
        config.endpoint,
        config.display_name,
        config.client_random_id,
        config.client_blob_cache,
        player_skin,
        config.login_settings,
    );
    domain.packet_capture = packet_capture_request(
        std::env::var_os(client_session::markers::PACKET_CAPTURE).as_deref(),
        config.record_session,
        &config.log_dir,
        unix_time_ms(),
    );
    #[cfg(feature = "native-networking")]
    let domain = domain.with_native_preparation(native_preparation);
    let prepare =
        move |handoff, game_data: &protocol::GameData, cancelled: &(dyn Fn() -> bool + Sync)| {
            preparation.run(handoff, game_data, cancelled)
        };
    let trace = client_session::SessionTrace {
        movement_line: gameplay::movement::pending_trace_line,
        write_movement: gameplay::movement::write_trace_line,
        packet_trace: emit_packet_trace,
        fast_transfer_action_marker: Some(client_ui::diagnostic_markers::FAST_TRANSFER_ACTION),
    };
    #[cfg(feature = "native-networking")]
    let result = match (friend, local_world) {
        (Some(companion), None) => client_session::spawn_network_with_companion(
            domain,
            runtime,
            tasks,
            prepare,
            trace,
            packet_relay,
            companion,
        ),
        (None, Some(companion)) => client_session::spawn_network_with_companion(
            domain,
            runtime,
            tasks,
            prepare,
            trace,
            packet_relay,
            companion,
        ),
        (None, None) => {
            client_session::spawn_network(domain, runtime, tasks, prepare, trace, packet_relay)
        }
        (Some(_), Some(_)) => unreachable!("checked exclusive native connection owners"),
    };
    #[cfg(not(feature = "native-networking"))]
    let result =
        client_session::spawn_network(domain, runtime, tasks, prepare, trace, packet_relay);
    result.map(|inner| match socket_dir {
        Some(directory) => NetworkHandle::connected(inner, directory),
        None => NetworkHandle::native(inner),
    })
}

/// The session's capture: [`client_session::markers::PACKET_CAPTURE`] decides when set
/// non-empty, even to `0`; otherwise the Record network session setting records into
/// `log_dir`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn packet_capture_request(
    environment: Option<&OsStr>,
    record_setting: bool,
    log_dir: &Path,
    started_unix_ms: u64,
) -> Option<client_session::packet_log::PacketCaptureRequest> {
    let value = environment
        .filter(|value| !value.is_empty())
        .or(record_setting.then_some(OsStr::new("1")));
    client_session::packet_log::PacketCaptureRequest::from_setting(value, log_dir, started_unix_ms)
}

/// Whether new sessions record a capture, and whether
/// [`client_session::markers::PACKET_CAPTURE`] decided it rather than the setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaptureChoice {
    pub(crate) on: bool,
    pub(crate) by_variable: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Display for CaptureChoice {
    /// `on` or `off`, followed by the variable's name in parentheses when it decided.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(if self.on { "on" } else { "off" })?;
        if self.by_variable {
            write!(formatter, " ({})", client_session::markers::PACKET_CAPTURE)?;
        }
        Ok(())
    }
}

/// Whether new sessions record a capture, by the same precedence as
/// [`packet_capture_request`].
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn packet_capture_choice(
    environment: Option<&OsStr>,
    record_setting: bool,
) -> CaptureChoice {
    match environment.filter(|value| !value.is_empty()) {
        Some(value) => CaptureChoice {
            on: value != "0",
            by_variable: true,
        },
        None => CaptureChoice {
            on: record_setting,
            by_variable: false,
        },
    }
}

/// Wall-clock milliseconds since the Unix epoch, as a capture header records its start.
#[cfg(not(target_arch = "wasm32"))]
fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
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

    #[test]
    fn the_capture_variable_beats_the_record_setting() {
        let logs = Path::new("logs");
        let in_logs = Some(client_session::packet_log::PacketCaptureRequest {
            path: logs.join(format!(
                "packets-7.{}",
                client_session::packet_log::CAPTURE_EXTENSION
            )),
            started_unix_ms: 7,
            in_log_dir: true,
        });
        assert_eq!(packet_capture_request(None, true, logs, 7), in_logs);
        assert_eq!(
            packet_capture_request(Some(OsStr::new("")), true, logs, 7),
            in_logs
        );
        assert_eq!(packet_capture_request(None, false, logs, 7), None);
        assert_eq!(
            packet_capture_request(Some(OsStr::new("0")), true, logs, 7),
            None
        );
        assert_eq!(
            packet_capture_request(Some(OsStr::new("run.cnbrcap")), false, logs, 7)
                .map(|request| request.path),
            Some(PathBuf::from("run.cnbrcap"))
        );
        let label = |environment: Option<&str>, setting| {
            packet_capture_choice(environment.map(OsStr::new), setting).to_string()
        };
        let variable = client_session::markers::PACKET_CAPTURE;
        assert_eq!(label(None, false), "off");
        assert_eq!(label(None, true), "on");
        assert_eq!(label(Some(""), true), "on");
        assert_eq!(label(Some("0"), true), format!("off ({variable})"));
        assert_eq!(label(Some("1"), false), format!("on ({variable})"));
    }
}
