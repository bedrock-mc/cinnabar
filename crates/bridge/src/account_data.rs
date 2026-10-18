//! Account data shared by menu presentation and native RPCs.

use crate::contract::{AuthState, ConnectStage};
use serde::{Deserialize, Serialize};

/// Remote HTTPS artwork and a local copy once one is cached; the core fills `path` only for art it renders.
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

/// The core's stage of preparing a join, and its pack download counts.
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

/// Omits zero counters in the same form as the Go control owner.
fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}
