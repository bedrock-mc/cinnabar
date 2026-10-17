//! Account and join progress data shared by local RPC and relay control.
use crate::contract::{AuthState, ConnectStage};
use serde::{Deserialize, Serialize};

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
