//! Local stream bridge between the Rust client and Go core.

mod account;
/// Shared frame limits and session message kinds for the Rust and Go bridge.
pub mod contract;
pub use contract::MAX_FRAME_LEN;
#[cfg_attr(target_arch = "wasm32", path = "endpoint_web.rs")]
mod endpoint;
mod error;
mod framed;
mod packet_delay;
#[cfg(target_arch = "wasm32")]
mod relay_control;
#[cfg(target_arch = "wasm32")]
pub use relay_control::RelayControl;
#[cfg_attr(
    not(target_arch = "wasm32"),
    allow(dead_code, reason = "only browsers run a relay control socket")
)]
mod relay_events;
mod status;
mod store;
mod worlds;
mod xbox_presence;
pub use xbox_presence::{XboxPresenceState, report_xbox_presence};

use std::fmt;
use std::path::{Path, PathBuf};

/// Connects to the core's session endpoint: one a local core publishes, or a browser's relay.
pub async fn connect_session(core: &CoreEndpoint) -> anyhow::Result<(FramedReader, FrameQueue)> {
    let stream = match core {
        CoreEndpoint::SocketDir(socket_dir) => {
            endpoint::connect(socket_dir, endpoint::EndpointKind::Session).await?
        }
        CoreEndpoint::Relay(relay) => endpoint::connect_relay(relay).await?,
    };
    Ok(crate::framed::queued(stream, MAX_FRAME_LEN))
}

pub use account::{
    Account, Artwork, AuthState, ConnectProgress, ConnectStage, ConnectTarget, Events,
    FeaturedGame, FeaturedServer, Friend, Home, Inbox, LiveEvent, Message, MessageButton,
    MessageEvent, MessageImage, Person, Profile, ProfileAchievement, ProfileAchievements,
    ProfileStatistics, Realm, RealmMembership, ServerDisconnect, ServerPing, ServerTrustPrompt,
    account_status, answer_server_trust, connect_target, home, list_featured_servers,
    list_featured_servers_with_counts, list_friends, list_people, list_realms, ping_servers,
    poll_events, profile, realm_membership, report_message_event, sign_out,
};
#[cfg(not(target_arch = "wasm32"))]
pub use endpoint::listener::SessionListener;
pub use error::BridgeError;
pub use framed::FramedStream;
pub use framed::{FrameQueue, FramedReader};
pub use packet_delay::{
    PacketDelayLease, RelayedPosition, packet_delay_with_position, set_packet_delay,
};
pub use status::{
    Lifecycle, NetworkConnection, PackAcquisition, PackAdmission, PackApplication,
    PackDownstreamOutcome, PackOffer, StatusV1, TransferPending, read_status,
    report_pack_application,
};
pub use store::{
    ConfirmedPurchase, PendingPurchase, PurchaseOutcome, PurchaseStatus, StoreBalance,
    StoreEntitlements, StoreOffer, StoreOfferDetail, StorePage, StorePrice, StoreRating, StoreRow,
    StoreRowMore, StoreSearch, StoreSearchResults, store_balance, store_entitlements, store_home,
    store_offer, store_purchase, store_row_more, store_search,
};
pub use worlds::{
    Backend, GameMode, LocalTarget, LocalWorld, LocalWorldInfo, Transport, WorldState,
    invite_to_world, set_local_world,
};

/// Returns the platform session endpoint used for the logical socket directory.
#[must_use]
pub fn session_endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    endpoint::endpoint_path(socket_dir, endpoint::EndpointKind::Session)
}

/// Returns the platform control endpoint used for the logical socket directory.
#[must_use]
pub fn control_endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    endpoint::endpoint_path(socket_dir, endpoint::EndpointKind::Control)
}

/// Where the client reaches its core's session endpoint. Browsers reach only a relay and native
/// builds only a local core; connecting to the other fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreEndpoint {
    /// A local core publishing its endpoints in this socket directory.
    SocketDir(PathBuf),
    /// A web relay running this browser's sessions.
    Relay(RelaySession),
}

/// A browser's session on a web relay.
#[derive(Clone, PartialEq, Eq)]
pub struct RelaySession {
    /// The relay's base WebSocket URL, such as `wss://play.example.com`.
    pub url: String,
    /// The session the relay's control endpoint issued; it is the browser's only credential.
    pub session_id: String,
}

/// Issues the relay session each browser join opens. A browser's control socket holds it and
/// replaces it when the relay forgets an idle session, so joins ask for it as they start.
pub trait RelaySessions: Send + Sync {
    /// The session the next join opens, once the relay has issued one.
    fn session(&self) -> Option<RelaySession>;
}

impl fmt::Debug for RelaySession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelaySession")
            .field("url", &self.url)
            .field("session_id", &"<redacted>")
            .finish()
    }
}

/// How long a freshly spawned core has to publish its bridge endpoint.
pub const CORE_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Returns every endpoint the core publishes in `socket_dir`, for cleanup after a lost core.
#[must_use]
pub fn core_endpoint_paths(socket_dir: &Path) -> [PathBuf; 2] {
    [
        session_endpoint_path(socket_dir),
        control_endpoint_path(socket_dir),
    ]
}

/// Applies the bridge transport's platform-specific endpoint readiness rule.
pub fn bridge_endpoint_exists(directory: &Path) -> bool {
    let endpoint = session_endpoint_path(directory);
    if cfg!(windows) {
        endpoint.is_file()
    } else {
        endpoint.exists()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bytes::Bytes;
    use futures::Stream;

    use super::{BridgeError, CoreEndpoint, FrameQueue, FramedReader, connect_session};

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
        let error = match connect_session(&CoreEndpoint::SocketDir(PathBuf::new())).await {
            Ok(_) => panic!("empty socket directory must fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error.downcast_ref::<BridgeError>(),
            Some(BridgeError::InvalidEndpoint { .. })
        ));
    }
}
