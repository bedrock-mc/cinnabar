//! Transfers the exact delivered Gathering selection to the application's session owner.

use super::*;
use launcher_host::native_account::NativeGatheringSelection;

impl MenuRuntime {
    /// Handles allocated rows locally, including stale/missing selection failures without a Go fallback.
    pub(super) fn request_native_gathering(&mut self, address: &str) -> bool {
        if !matches!(target_for(address), bridge::ConnectTarget::Gathering(_)) {
            return false;
        }
        let Some(selection) = self.native_gatherings.get(address).cloned() else {
            self.message =
                Some("That experience is no longer available. Refresh to try again.".into());
            return true;
        };
        let Some(path) = self.launcher_auth_cache() else {
            self.message = Some("Sign in before joining this experience.".into());
            return true;
        };
        if self.account_change_pending() {
            return true;
        }
        let account = self.native_account_for(path.clone());
        if !selection.belongs_to(&account) {
            self.message =
                Some("That experience belongs to a previous account. Refresh to try again.".into());
            return true;
        }
        self.remember_session_origin();
        self.stop_catalog();
        self.remember_retry_target(address, Some(&path), false);
        self.intents.join = None;
        self.intents.native_world = None;
        self.intents.gathering = Some(selection);
        self.show_connecting();
        true
    }

    /// Moves the captured owner; account mutation revokes the pending action before dispatch.
    pub fn take_native_gathering_intent(&mut self) -> Option<NativeGatheringSelection> {
        if self.account_change_pending() || !self.native_sign_in {
            self.intents.gathering = None;
            return None;
        }
        let selection = self.intents.gathering.take()?;
        let path = self.launcher_auth_cache()?;
        let account = self.native_account_for(path);
        selection.belongs_to(&account).then_some(selection)
    }
}
