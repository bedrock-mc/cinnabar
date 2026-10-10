//! Local stream bridge between the Rust client and Go core.

mod account;
mod endpoint;
mod error;
mod framed;
mod packet_delay;
mod session;
mod status;
mod store;
mod worlds;
mod xbox_presence;
pub use xbox_presence::{XboxPresenceState, report_xbox_presence};

use std::path::Path;

pub use account::{
    Account, Artwork, AuthState, ConnectProgress, ConnectStage, ConnectTarget, Events,
    FeaturedGame, FeaturedServer, Friend, Home, Inbox, LiveEvent, Message, MessageButton,
    MessageEvent, MessageImage, Person, Profile, ProfileAchievement, ProfileAchievements,
    ProfileStatistics, Realm, RealmMembership, ServerDisconnect, ServerPing, ServerTrustPrompt,
    account_status, answer_server_trust, connect_target, home, list_featured_servers,
    list_featured_servers_with_counts, list_friends, list_people, list_realms, ping_servers,
    poll_events, profile, realm_membership, report_message_event, sign_out,
};
pub use error::BridgeError;
pub(crate) use framed::FramedStream;
pub use framed::{FrameQueue, FramedReader};
pub use packet_delay::{
    PacketDelayLease, RelayedPosition, packet_delay_with_position, set_packet_delay,
};
pub use session::{
    ConnectRequest, CoreMessage, HandoffPack, HandoffPackReceiver, PackContentKey,
    SessionDisconnect, SessionHandoff, SessionIdentity, SessionTransfer, batch_frame_body,
    batch_frame_from_bedrock, connect_session, decode_core_message, encode_batch, encode_connect,
};
pub use status::{
    Lifecycle, PackAcquisition, PackAdmission, PackApplication, PackDownstreamOutcome, PackOffer,
    StatusV1, TransferPending, read_status, report_pack_application,
};
pub use store::{
    ConfirmedPurchase, PendingPurchase, PurchaseOutcome, PurchaseStatus, StoreBalance,
    StoreEntitlements, StoreOffer, StoreOfferDetail, StorePage, StorePrice, StoreRating, StoreRow,
    StoreRowMore, StoreSearch, StoreSearchResults, store_balance, store_entitlements, store_home,
    store_offer, store_purchase, store_row_more, store_search,
};
pub use worlds::{
    Backend, CODE_EULA_REQUIRED, Difficulty, GameMode, Generator, NewWorld, Prefs, PrefsUpdate,
    Setup, SetupState, UnavailableReason, World, WorldState, WorldStatus, WorldUpdate,
    accept_bds_eula, close_world, create_world, delete_world, invite_to_world, list_worlds,
    local_worlds_prefs, open_world, open_world_with, set_world_paused, update_world, world_status,
};

/// Returns the platform endpoint used for the logical socket directory.
#[must_use]
pub fn endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    endpoint::endpoint_path(socket_dir, endpoint::EndpointKind::Game)
}

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

/// Largest payload accepted by the local bridge framing protocol.
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

/// Connects to the local Go core game endpoint published in `socket_dir`.
///
/// A spawned writer task owns the write half, so sends never wait on a read in progress.
pub async fn connect(socket_dir: &Path) -> anyhow::Result<(FramedReader, FrameQueue)> {
    let stream = endpoint::connect(socket_dir, endpoint::EndpointKind::Game).await?;
    Ok(framed::queued(stream, MAX_FRAME_LEN))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bytes::Bytes;
    use futures::Stream;

    use super::{BridgeError, FrameQueue, FramedReader, connect};

    fn assert_transport<R, W>()
    where
        R: Stream<Item = Result<Bytes, BridgeError>> + Unpin + Send,
        W: Clone + Send + Sync,
    {
    }

    #[test]
    fn public_transport_contract_is_stable() {
        assert_transport::<FramedReader, FrameQueue>();
        let _ = connect;
    }

    #[tokio::test]
    async fn connect_preserves_bridge_error_in_anyhow_result() {
        let error = match connect(Path::new("")).await {
            Ok(_) => panic!("empty socket directory must fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error.downcast_ref::<BridgeError>(),
            Some(BridgeError::InvalidEndpoint { .. })
        ));
    }
}
