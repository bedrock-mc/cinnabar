//! Client activity reported to the account core without Xbox credentials.
use crate::{BridgeError, account::call};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Committed world state; the Go core selects and publishes its Xbox activity ID.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct XboxPresenceState {
    pub in_world: bool,
    pub game_mode: i32,
    pub realm: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub experience: String,
}

#[derive(Deserialize)]
struct Accepted {}

/// Queues the newest activity in the core; this does not wait for Xbox Live.
pub async fn report_xbox_presence(
    socket_dir: &Path,
    state: &XboxPresenceState,
) -> Result<(), BridgeError> {
    call::<Accepted, _>(socket_dir, "presence.v1", Some(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_have_no_world_or_experience_identity() {
        let state = XboxPresenceState::default();
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!({"in_world": false, "game_mode": 0, "realm": false})
        );
    }
}
