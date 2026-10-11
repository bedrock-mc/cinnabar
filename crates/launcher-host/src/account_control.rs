//! Account feed requests and replies shared by host workers and menu adapters.

use launcher::menu::{
    auth::AuthState,
    view::{
        JoinStage, MenuFriendCard, MenuHome, MenuProfile, MenuRealmCard, MenuServerCard, PingInfo,
        ServerDetails, ServerTrustPrompt,
    },
};

/// Control method names the implementation calls.
#[allow(dead_code, reason = "named for the core-relay control clients")]
pub mod method {
    pub const REALMS_LIST: &str = "realms_list.v1";
    pub const FRIENDS_LIST: &str = "friends_list.v1";
    pub const CONNECT: &str = "connect.v1";
    pub const ACCOUNT_STATUS: &str = "account_status.v1";
    pub const SIGN_OUT: &str = "sign_out.v1";
}

/// An account-surface event pushed by the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountEvent {
    /// The sign-in state changed (device code shown, signed in, signed out).
    #[allow(dead_code, reason = "auth changes arrive as polled status")]
    Auth(AuthState),
    /// The live session ended; the reason shows on the disconnect screen.
    Disconnected { reason: String },
}

/// A request ticket, acceptance flag, and result from the Realm membership worker.
pub type RealmMembershipResponse = (u64, bool, Result<(String, MenuRealmCard), ()>);

/// What the play and sign-in screens need from the core.
pub trait AccountControl {
    /// `account_status.v1`: the current sign-in state, when known.
    fn account_status(&mut self) -> Option<AuthState>;
    /// Changes whenever the account identity or sign-in state retires its data.
    fn account_generation(&mut self) -> Option<u64> {
        None
    }
    /// `realms_list.v1`: joinable realms, or `None` while unavailable.
    fn realms(&mut self) -> Option<Vec<MenuRealmCard>>;
    /// `friends_list.v1`: friend worlds, or `None` while unavailable.
    fn friends(&mut self) -> Option<Vec<MenuFriendCard>>;
    /// `sign_out.v1`; `true` once the core accepted it.
    fn sign_out(&mut self) -> bool;
    /// The next pending account event, if any.
    fn poll_event(&mut self) -> Option<AccountEvent>;
    /// `featured_servers.v1`: cards plus their info-panel details, when fetched.
    fn featured(&mut self) -> Option<Vec<(MenuServerCard, ServerDetails)>> {
        None
    }
    /// Requests live experience counts only while selected experience details are visible.
    fn set_player_counts_visible(&mut self, _visible: bool) {}
    /// `profile.v1`: the signed-in profile, when fetched.
    fn profile(&mut self) -> Option<MenuProfile> {
        None
    }
    /// Requests an immediate profile retry from the feed worker.
    fn refresh_profile(&mut self) {}
    /// `home.v1`: the start screen's service data, when fetched.
    fn home(&mut self) -> Option<MenuHome> {
        None
    }
    /// Reports an inbox interaction through the core messaging session.
    fn report_message(&mut self, _event: bridge::MessageEvent) {}
    /// The server rows `ping.v1` keeps fresh while the launcher shows them.
    fn set_ping_targets(&mut self, _targets: Vec<String>) {}
    /// Pongs from the latest ping round, keyed by address.
    fn pings(&mut self) -> Option<Vec<(String, PingInfo)>> {
        None
    }
    /// The core's stage of preparing the current join; `None` once it hands over.
    fn join_stage(&mut self) -> Option<JoinStage> {
        None
    }
    /// Whether the menu is connecting, which speeds up event polling.
    fn set_joining(&mut self, _joining: bool) {}
    /// The join's pending question whether to trust a NetherNet server.
    fn server_trust(&mut self) -> Option<ServerTrustPrompt> {
        None
    }
    /// Answers trust prompt `id`.
    fn answer_server_trust(&mut self, _id: u64, _trusted: bool) {}
    /// Asks `friends_people.v1` for the account's Xbox friends off the frame.
    fn request_people(&mut self) {}
    /// The friends the last request listed, or `Err` when it failed.
    fn people(&mut self) -> Option<Result<Vec<launcher::menu::invite::Friend>, ()>> {
        None
    }
    /// Sends each friend an invite to the hosted world through `world_invite.v1`, off the frame.
    fn send_invites(&mut self, _xuids: Vec<String>) {}
    /// Queues invitation preview or acceptance on the dedicated account worker.
    fn request_realm_membership(&mut self, _ticket: u64, _code: String, _accept: bool) -> bool {
        false
    }
    /// Cancels the pending preview, closing its control connection.
    fn cancel_realm_membership(&mut self) {}
    /// Delivers one request generation's result.
    fn realm_membership(&mut self) -> Option<RealmMembershipResponse> {
        None
    }
}
