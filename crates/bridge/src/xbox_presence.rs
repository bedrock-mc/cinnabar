//! Client activity reported to the account core without Xbox credentials.
use crate::{BridgeError, account::call};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Committed world state shared by the account core and native publisher.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct XboxPresenceState {
    pub in_world: bool,
    pub game_mode: i32,
    pub realm: bool,
    pub experience: bool,
}

impl XboxPresenceState {
    /// Selects the shared product activity, with experience taking precedence over Realm modes.
    pub fn activity_id(&self) -> String {
        use crate::contract::*;
        if !self.in_world {
            return PRESENCE_ACTIVITY_MENUS.to_owned();
        }
        if self.experience {
            return PRESENCE_ACTIVITY_EXPERIENCE.to_owned();
        }
        let mode = match self.game_mode {
            PRESENCE_GAME_MODE_CREATIVE => PRESENCE_ACTIVITY_CREATIVE,
            PRESENCE_GAME_MODE_ADVENTURE => PRESENCE_ACTIVITY_ADVENTURE,
            _ => PRESENCE_ACTIVITY_SURVIVAL,
        };
        if self.realm {
            format!("{PRESENCE_ACTIVITY_REALM_PREFIX}{mode}")
        } else {
            mode.to_owned()
        }
    }
}

#[derive(Deserialize)]
struct Accepted {}

/// Queues the newest activity in the core; this does not wait for Xbox Live.
pub async fn report_xbox_presence(
    socket_dir: &Path,
    state: &XboxPresenceState,
) -> Result<(), BridgeError> {
    call::<Accepted, _>(socket_dir, crate::contract::METHOD_PRESENCE, Some(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_activity_matches_the_shared_go_policy_cases() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../data/presence-activities.json")).unwrap();
        assert_eq!(cases.len(), 40);
        for case in cases {
            let state = XboxPresenceState {
                in_world: case["in_world"].as_bool().unwrap(),
                game_mode: case["game_mode"].as_i64().unwrap() as i32,
                realm: case["realm"].as_bool().unwrap(),
                experience: case["experience"].as_bool().unwrap(),
            };
            assert_eq!(state.activity_id(), case["activity"].as_str().unwrap());
        }
    }

    #[test]
    fn menus_have_no_world_or_experience_identity() {
        let state = XboxPresenceState::default();
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!({"in_world": false, "game_mode": 0, "realm": false, "experience": false})
        );
    }
}
