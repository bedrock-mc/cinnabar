//! Account and service data shared by native ownership and menu presentation.

use crate::contract::{AuthState, ConnectStage};
use serde::{Deserialize, Serialize};

/// Remote HTTPS artwork and a local copy once one is cached; `path` is set only after the requested artwork is cached.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Artwork {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub path: String,
}

/// The four Xbox title statistics vanilla requests for a player profile.
/// Numeric strings preserve the service's precision; absent values are unavailable.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ProfileStatistics {
    pub minutes_played: Option<String>,
    pub blocks_broken: Option<String>,
    pub mobs_defeated: Option<String>,
    pub distance_travelled: Option<String>,
}

/// The account's achievement summary for the authenticated Minecraft title.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ProfileAchievements {
    pub unlocked: u32,
    pub total: u32,
    pub current_gamerscore: Option<i64>,
    pub max_gamerscore: Option<i64>,
    pub entries: Vec<ProfileAchievement>,
}

/// Xbox achievement data; game-specific suggested order remains optional.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ProfileAchievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub image: Artwork,
    pub gamerscore: Option<i64>,
    pub locked: bool,
    #[serde(default)]
    pub date_unlocked: String,
    pub suggested_order: Option<u32>,
}

/// A messaging report: `event_type` is Click, Dismiss, Delete, Impression, ControlImpression or ReadAll.
#[derive(Clone, Debug, Default, Serialize, Eq, PartialEq)]
pub struct MessageEvent {
    pub event_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instance_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub report_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub button_id: String,
}

/// One Xbox friend of the signed-in account; `xuid` addresses their game invite.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Person {
    pub xuid: String,
    pub gamertag: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub gamerpic: Artwork,
}

/// Sign-in state plus the fields that belong to it; `reason` never carries secrets or paths.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Account {
    pub state: AuthState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamertag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The active stage of preparing a join, and its pack download counts.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ConnectProgress {
    pub stage: ConnectStage,
    /// Packs being downloaded (cache hits excluded) and those finished.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub packs_done: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub packs_total: u32,
    /// Across all packs; the total grows as each pack's download begins.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub received_bytes: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub total_bytes: u64,
}

/// Omits zero counters in the same compact form for consumers.
fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

/// One Realm the account can join; `target` is its stable join id.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Realm {
    pub name: String,
    pub state: String,
    pub target: String,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub motd: String,
    #[serde(default)]
    pub world_type: String,
    #[serde(default)]
    pub online_players: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub days_left: i32,
    #[serde(default)]
    pub expired: bool,
    /// Joined as a member rather than owned.
    #[serde(default)]
    pub member: bool,
}

/// One activity a featured server or gathering advertises.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct FeaturedGame {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image: Artwork,
}

/// A featured server with the details the play screen's info panel shows.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct FeaturedServer {
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub news_title: String,
    #[serde(default)]
    pub news: String,
    #[serde(default)]
    pub logo: Artwork,
    #[serde(default)]
    pub background: Artwork,
    #[serde(default)]
    pub screenshots: Vec<Artwork>,
    #[serde(default)]
    pub games: Vec<FeaturedGame>,
    /// The experience's service count, absent until it is available.
    #[serde(default)]
    pub player_count: Option<i64>,
}

/// The signed-in account as the start and profile screens show it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Profile {
    #[serde(default)]
    pub gamertag: String,
    #[serde(default)]
    pub xuid: String,
    #[serde(default)]
    pub gamerpic: Artwork,
    #[serde(default)]
    pub avatar: Artwork,
    #[serde(default)]
    pub avatar_error: bool,
    #[serde(default)]
    pub featured_screenshot: Artwork,
    #[serde(default)]
    pub featured_screenshot_error: bool,
    #[serde(default)]
    pub real_name: String,
    #[serde(default)]
    pub presence_text: String,
    /// Absent when the lookup failed, so the UI never shows a fabricated zero.
    #[serde(default)]
    pub gamerscore: Option<i64>,
    #[serde(default)]
    pub friends: Option<u32>,
    #[serde(default)]
    pub followers: Option<u32>,
    #[serde(default)]
    pub statistics: Option<ProfileStatistics>,
    #[serde(default)]
    pub achievements: Option<ProfileAchievements>,
}

/// The start screen's service data: messaging surfaces, inbox counts,
/// treatments, the pending Realms invite count, live events and the persona head.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Home {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub inbox: Inbox,
    #[serde(default)]
    pub treatments: Vec<String>,
    #[serde(default)]
    pub realm_invites: u32,
    #[serde(default)]
    pub live_events: Vec<LiveEvent>,
    #[serde(default)]
    pub persona_head: Artwork,
}

/// One player-messaging message; `surface` places it (`PlayButton`, `InboxMessage`, ...).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Message {
    #[serde(default)]
    pub colors: std::collections::BTreeMap<String, [u8; 3]>,
    #[serde(default)]
    pub received: String,
    #[serde(default)]
    pub sender: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub instance_id: String,
    #[serde(default)]
    pub report_id: String,
    #[serde(default)]
    pub surface: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub sub_title: String,
    #[serde(default)]
    pub banner: String,
    #[serde(default)]
    pub images: Vec<MessageImage>,
    #[serde(default)]
    pub buttons: Vec<MessageButton>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct MessageImage {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct MessageButton {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub action: String,
}

/// Per-category service totals can include messages outside the loaded page.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct InboxCategory {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub unread: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Inbox {
    #[serde(default)]
    pub categories: Vec<InboxCategory>,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub unread: u32,
}

/// A live gathering with its active segment's start-screen UI.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct LiveEvent {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub start_unix: i64,
    #[serde(default)]
    pub end_unix: i64,
    #[serde(default)]
    pub route_to_servers: bool,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub button_text: String,
    #[serde(default)]
    pub caption_text: String,
    #[serde(default)]
    pub caption_countdown: bool,
    #[serde(default)]
    pub badge: Artwork,
    #[serde(default)]
    pub event_image: Artwork,
}

/// One server's RakNet pong; `online` is false when it did not answer.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct ServerPing {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub players: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub ping_ms: u32,
    #[serde(default)]
    pub motd: String,
}

/// One friend's joinable world; `xuid` identifies it for [`crate::ConnectTarget::Friend`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Friend {
    pub gamertag: String,
    pub xuid: String,
    pub world_name: String,
    pub members: u32,
    pub max_members: u32,
    #[serde(default)]
    pub handle_id: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

/// A verified or explicitly accepted Realm invitation.
#[derive(Clone, Debug, Deserialize)]
pub struct RealmMembership {
    pub code: String,
    pub realm: Realm,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn featured_player_counts_preserve_missing_zero_and_signed_values() {
        for (payload, expected) in [
            (r#"{}"#, None),
            (r#"{"player_count":null}"#, None),
            (r#"{"player_count":0}"#, Some(0)),
            (r#"{"player_count":-1}"#, Some(-1)),
            (r#"{"player_count":12345}"#, Some(12_345)),
        ] {
            let server: FeaturedServer = serde_json::from_str(payload).expect("featured server");
            assert_eq!(server.player_count, expected);
        }
    }
}
