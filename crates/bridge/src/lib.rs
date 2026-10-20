//! Local stream bridge between the Rust client and Go core.

#[cfg(not(target_arch = "wasm32"))]
mod account;
mod account_data;
mod player_identity;
pub use account_data::{
    Account, Artwork, ConnectProgress, MessageEvent, Person, ProfileAchievement,
    ProfileAchievements, ProfileStatistics,
};
pub use player_identity::{valid_cinnabar_uuid, valid_cinnabar_xuid};
#[cfg(not(target_arch = "wasm32"))]
mod cinnabar_api;
#[cfg(not(target_arch = "wasm32"))]
pub use cinnabar_api::{
    CinnabarApiConfig, CinnabarLookup, configure_cinnabar_api, lookup_cinnabar_players,
    sync_cinnabar_players,
};
/// Shared values and wire enums for the Rust and Go bridge.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod contract;
pub use contract::{
    AuthState, Backend, CinnabarRelease, ConnectStage, ConnectTarget, GameMode, Lifecycle,
    MAX_FRAME_LEN, PackAcquisition, PackApplication, PackDownstreamOutcome, PackOffer, Transport,
    WorldState,
};
#[cfg(not(target_arch = "wasm32"))]
mod endpoint;
mod error;
mod framed;
pub mod join_profile;
pub mod latency_trace;
#[cfg(not(target_arch = "wasm32"))]
mod packet_delay;
#[cfg(not(target_arch = "wasm32"))]
mod status;
pub mod tasks;
#[cfg(not(target_arch = "wasm32"))]
mod worlds;
#[cfg(not(target_arch = "wasm32"))]
mod xbox_presence;
#[cfg(not(target_arch = "wasm32"))]
pub use xbox_presence::{XboxPresenceState, report_xbox_presence};

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

/// Connects to the core's session endpoint published in `socket_dir`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn connect_session(socket_dir: &Path) -> anyhow::Result<(FramedReader, FrameQueue)> {
    let stream = endpoint::connect(socket_dir, endpoint::EndpointKind::Session).await?;
    Ok(crate::framed::queued(stream, MAX_FRAME_LEN))
}

#[cfg(not(target_arch = "wasm32"))]
pub use account::{
    Events, FeaturedGame, FeaturedServer, Friend, Home, Inbox, LiveEvent, Message, MessageButton,
    MessageImage, Profile, Realm, RealmMembership, ServerDisconnect, ServerPing, ServerTrustPrompt,
    account_status, answer_server_trust, connect_target, home, list_featured_servers,
    list_featured_servers_with_counts, list_friends, list_people, list_realms, ping_servers,
    poll_events, profile, realm_membership, report_message_event, sign_out,
};
#[cfg(not(target_arch = "wasm32"))]
pub use endpoint::listener::SessionListener;
pub use error::BridgeError;
pub use framed::BridgeCodec;
#[cfg(not(target_arch = "wasm32"))]
pub use framed::FramedStream;
pub use framed::{FrameQueue, FrameRead, FrameWritten, FramedReader};
#[cfg(not(target_arch = "wasm32"))]
pub use packet_delay::{
    PacketDelayLease, RelayedPosition, packet_delay_with_position, set_packet_delay,
};
#[cfg(not(target_arch = "wasm32"))]
pub use status::{
    NetworkConnection, PackAdmission, StatusV1, TransferPending, read_status,
    report_pack_application,
};
#[cfg(not(target_arch = "wasm32"))]
pub use worlds::{LocalTarget, LocalWorld, LocalWorldInfo, invite_to_world, set_local_world};

/// Returns the platform session endpoint used for the logical socket directory.
#[must_use]
#[cfg(not(target_arch = "wasm32"))]
pub fn session_endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    endpoint::endpoint_path(socket_dir, endpoint::EndpointKind::Session)
}

/// Returns the platform control endpoint used for the logical socket directory.
#[must_use]
#[cfg(not(target_arch = "wasm32"))]
pub fn control_endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    endpoint::endpoint_path(socket_dir, endpoint::EndpointKind::Control)
}

/// How long a freshly spawned core has to publish its bridge endpoint.
pub const CORE_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Returns every endpoint the core publishes in `socket_dir`, for cleanup after a lost core.
#[must_use]
#[cfg(not(target_arch = "wasm32"))]
pub fn core_endpoint_paths(socket_dir: &Path) -> [PathBuf; 2] {
    [
        session_endpoint_path(socket_dir),
        control_endpoint_path(socket_dir),
    ]
}

/// Applies the bridge transport's platform-specific endpoint readiness rule.
#[cfg(not(target_arch = "wasm32"))]
pub fn bridge_endpoint_exists(directory: &Path) -> bool {
    let endpoint = session_endpoint_path(directory);
    if cfg!(windows) {
        endpoint.is_file()
    } else {
        endpoint.exists()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::path::Path;

    use bytes::Bytes;
    use futures::Stream;

    use super::{BridgeError, FrameQueue, FramedReader, connect_session};

    fn assert_transport<R, W>()
    where
        R: Stream<Item = Result<Bytes, BridgeError>> + Unpin + Send,
        W: Clone + Send + Sync,
    {
    }

    #[test]
    fn public_transport_contract_is_stable() {
        assert_transport::<FramedReader, FrameQueue>();
        let _ = connect_session;
    }

    #[tokio::test]
    async fn connect_session_rejects_empty_directory() {
        let error = match connect_session(Path::new("")).await {
            Ok(_) => panic!("empty socket directory must fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error.downcast_ref::<BridgeError>(),
            Some(BridgeError::InvalidEndpoint { .. })
        ));
    }
}
