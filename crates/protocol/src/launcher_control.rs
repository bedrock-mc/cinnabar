//! Launcher control client (realms, friends, connect, account, events), re-exported so the app
//! reaches the bridge through this facade.

pub use bridge::{
    Account, Artwork, AuthState, BridgeError, ConnectProgress, ConnectStage, ConnectTarget, Events,
    FeaturedGame, FeaturedServer, Friend, Home, Inbox, LiveEvent, Message, MessageButton,
    MessageEvent, MessageImage, Person, Profile, ProfileAchievement, ProfileAchievements,
    ProfileStatistics, Realm, RealmMembership, ServerDisconnect, ServerPing, ServerTrustPrompt,
    TransferPending, account_status, answer_server_trust, connect_target, control_endpoint_path,
    home, list_featured_servers, list_featured_servers_with_counts, list_friends, list_people,
    list_realms, ping_servers, poll_events, profile, realm_membership, report_message_event,
    sign_out,
};
pub use bridge::{PacketDelayLease, RelayedPosition, packet_delay_with_position, set_packet_delay};
pub use bridge::{XboxPresenceState, report_xbox_presence};
