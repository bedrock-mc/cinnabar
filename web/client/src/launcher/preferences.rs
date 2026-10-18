//! A bounded browser preference document; account credentials never belong here.

use ::launcher::menu::{MAX_SERVER_NAME_BYTES, view::SavedServer};
use serde::{Deserialize, Serialize};

/// Maximum serialized UTF-8 bytes accepted by the browser storage owner.
pub const MAX_PREFERENCE_BYTES: usize = 32 * 1024;
/// Maximum saved addressed servers in the browser menu.
pub const MAX_SERVERS: usize = 32;

/// The small durable subset of menu state supported by the browser host.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub gui_scale_offset: i8,
    pub servers: Vec<SavedServer>,
}

impl Preferences {
    /// Rejects oversized or invalid stored state before exposing it to the game menu.
    pub fn decode(json: &str) -> Result<Self, &'static str> {
        if json.len() > MAX_PREFERENCE_BYTES {
            return Err("Browser preferences exceed the storage limit.");
        }
        let preferences: Self =
            serde_json::from_str(json).map_err(|_| "Browser preferences could not be read.")?;
        preferences.validate()?;
        Ok(preferences)
    }

    /// Validates before writing, so rejected changes leave the last saved document intact.
    pub fn encode(&self) -> Result<String, &'static str> {
        self.validate()?;
        let json =
            serde_json::to_string(self).map_err(|_| "Browser preferences could not be saved.")?;
        if json.len() > MAX_PREFERENCE_BYTES {
            return Err("Browser preferences exceed the storage limit.");
        }
        Ok(json)
    }

    /// Checks the saved-server count and each bounded addressed-server row.
    fn validate(&self) -> Result<(), &'static str> {
        if self.servers.len() > MAX_SERVERS {
            return Err("Browser preferences contain unsupported settings.");
        }
        for server in &self.servers {
            if server.name.trim().is_empty()
                || server.name.len() > MAX_SERVER_NAME_BYTES
                || server.name.chars().any(char::is_control)
            {
                return Err("Enter a valid server name.");
            }
            super::addressed_target(&server.address)?;
        }
        Ok(())
    }
}
