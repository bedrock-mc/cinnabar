//! Shared service data, bounded browser/gateway framing and local-server wire contracts.

mod account_data;
mod cinnabar_api;
mod error;
mod framed;
mod packet_delay;
mod player_identity;
mod status;
mod worlds;
mod xbox_presence;

pub use account_data::{
    Account, Artwork, ConnectProgress, FeaturedGame, FeaturedServer, Friend, Home, Inbox,
    InboxCategory, LiveEvent, Message, MessageButton, MessageEvent, MessageImage, Person, Profile,
    ProfileAchievement, ProfileAchievements, ProfileStatistics, Realm, RealmMembership, ServerPing,
};
pub use cinnabar_api::{CinnabarApiConfig, CinnabarLookup};
pub use error::BridgeError;
pub use framed::{BridgeCodec, FrameQueue, FrameRead, FrameWritten, FramedReader};
pub use packet_delay::{PacketDelayLease, RelayedPosition};
pub use player_identity::{valid_cinnabar_uuid, valid_cinnabar_xuid};
pub use status::NetworkConnection;
pub use worlds::{LocalTarget, LocalWorld, LocalWorldInfo};
pub use xbox_presence::XboxPresenceState;

/// Shared product policy and wire enums.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub mod contract;
pub mod gateway_egress;
#[cfg(not(target_arch = "wasm32"))]
pub mod host_world;
pub mod join_profile;
pub mod latency_trace;
pub mod relay_control;
pub mod tasks;
pub use contract::{
    AuthState, Backend, CinnabarRelease, ConnectStage, ConnectTarget, GameMode, Lifecycle,
    MAX_FRAME_LEN, PackAcquisition, PackApplication, PackDownstreamOutcome, PackOffer, Transport,
    WorldState,
};
