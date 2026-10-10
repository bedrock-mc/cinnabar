//! Retains a remote destination after its transient join intent is consumed.

use std::path::{Path, PathBuf};

use super::MenuRuntime;

#[derive(Debug)]
pub(super) struct RetryTarget {
    address: String,
    auth_cache: Option<PathBuf>,
    account_id: Option<String>,
    featured: bool,
}

impl MenuRuntime {
    /// Returns the selected destination retained across the current join.
    pub(super) fn presence_address(&self) -> Option<&str> {
        self.retry_target
            .as_ref()
            .map(|target| target.address.as_str())
    }

    /// Retains the selected card's featured status without mapping server identities.
    pub(super) fn presence_is_featured(&self) -> bool {
        self.retry_target
            .as_ref()
            .is_some_and(|target| target.featured)
    }

    /// Queues a featured join and keeps its activity classification through adoption and retry.
    pub(super) fn request_featured_connect(&mut self, address: String) {
        self.request_connect(address.clone());
        if self
            .intents
            .join
            .as_ref()
            .is_some_and(|intent| intent.address == address)
            && let Some(target) = self.retry_target.as_mut()
        {
            target.featured = true;
        }
    }

    /// Prevents retrying the old server when a transfer cannot be followed safely.
    pub(crate) fn clear_retry_target(&mut self) {
        self.retry_target = None;
    }

    /// Remembers the actual selected destination, including automatic transfer replacements.
    pub(crate) fn remember_retry_target(
        &mut self,
        address: &str,
        auth_cache: Option<&Path>,
        local_world: bool,
    ) {
        let featured = self
            .retry_target
            .as_ref()
            .is_some_and(|target| target.address == address && target.featured);
        self.retry_target = (!local_world).then(|| RetryTarget {
            address: address.to_owned(),
            auth_cache: auth_cache.map(Path::to_path_buf),
            account_id: self.feeds.account_active_id.clone(),
            featured,
        });
    }

    /// Retry is available only for a failed remote session under the same account.
    pub(super) fn can_reconnect(&self) -> bool {
        self.launcher
            && self.disconnect_message.is_some()
            && !self.is_connecting()
            && !self.local_world_joined
            && !self.account_change_pending()
            && self.retry_target.as_ref().is_some_and(|target| {
                target.account_id == self.feeds.account_active_id
                    && target.auth_cache == self.launcher_auth_cache()
            })
    }

    /// Queues another normal join without replacing its remembered launcher origin.
    pub(super) fn reconnect(&mut self) {
        if !self.can_reconnect() || !self.focus_actions().contains(&super::MenuAction::Reconnect) {
            return;
        }
        let address = self.retry_target.as_ref().unwrap().address.clone();
        self.request_connect(address);
        self.focused = 0;
        self.hovered = None;
    }

    /// Acknowledges the failure and returns to its origin without retaining a retry action.
    pub(super) fn dismiss_disconnect(&mut self) {
        self.disconnect_message = None;
        self.retry_target = None;
        self.show_session_origin(self.screen);
    }
}
