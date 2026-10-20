//! Developer-only selection of the same opaque rows retained by the native catalog.

use super::*;
use developer_control::protocol::{NativeFriendChoice, NativeFriendRow, NativeFriendsReport};

const STALE: &str = "The native friend catalog changed. Read it again before joining.";
const UNAVAILABLE: &str = "The native friend catalog is unavailable for the selected account.";

impl MenuRuntime {
    /// Reports current rows or queues their original selection without enabling ordinary UI joins.
    pub fn developer_native_friends(
        &mut self,
        join: Option<NativeFriendChoice>,
    ) -> Result<NativeFriendsReport, String> {
        if !self.native_sign_in
            || self.fixture_active()
            || self.presentation_accounts
            || self.account_change_pending()
        {
            return Err(UNAVAILABLE.into());
        }
        let selected = self.native_catalog.selected.as_ref().ok_or(UNAVAILABLE)?;
        if selected.identity.as_deref() != self.selected_account_id()
            || Some(&selected.path) != self.launcher_auth_cache().as_ref()
        {
            return Err(UNAVAILABLE.into());
        }
        let queued = join.is_some();
        if let Some(choice) = join {
            if choice.revision != self.native_catalog.epoch {
                return Err(STALE.into());
            }
            let row = self
                .native_catalog
                .friend_worlds
                .get(choice.index)
                .ok_or(STALE)?;
            if row.card().xuid != choice.expected_xuid {
                return Err(STALE.into());
            }
            if self.is_connecting()
                || self.native_catalog.join.is_some()
                || self.intents.gathering.is_some()
                || self.intents.native_world.is_some()
            {
                return Err("A join is already queued or in progress.".into());
            }
            let selection = self
                .native_friend_selection(choice.index)
                .ok_or("That row has no current native friend selection.")?;
            self.queue_native_friend(selection);
        }
        Ok(NativeFriendsReport {
            revision: self.native_catalog.epoch,
            worlds: self
                .native_catalog
                .friend_worlds
                .iter()
                .enumerate()
                .map(|(index, world)| NativeFriendRow {
                    index,
                    xuid: world.card().xuid.clone(),
                    world_name: world.card().world_name.clone(),
                    selectable: world.selection().is_some(),
                })
                .collect(),
            queued,
        })
    }
}

#[cfg(test)]
mod tests;
