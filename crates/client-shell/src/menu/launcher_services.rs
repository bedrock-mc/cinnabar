//! One selected native account-service lifetime and its original retirement fences.

use super::{MenuRuntime, launcher_account::LauncherAccount};
use crate::local_worlds::LocalWorlds;
use bevy::prelude::{Commands, Resource};
use std::path::PathBuf;

/// Owns the selected services until their original publisher confirms retirement.
#[derive(Default, Resource)]
pub struct LauncherServices {
    running: Option<Attachment>,
    retirement_blocked: bool,
    presence_lifetime: Option<launcher_host::launcher_account::TitlePresenceLifetime>,
    workers: Option<launcher_host::launcher_account::AccountWorkerLifetime>,
    /// Covers session cleanup independently of this account service's worker group.
    session_workers: Option<tokio_util::task::TaskTracker>,
}

struct Attachment {
    cache: Option<PathBuf>,
    identity: Option<String>,
    account: Option<std::sync::Arc<launcher_host::native_account::NativeAccount>>,
}

impl LauncherServices {
    /// Retains the application's original session completion group before account admission.
    pub fn set_session_workers(&mut self, workers: tokio_util::task::TaskTracker) {
        self.session_workers = Some(workers);
    }

    /// Retires original readers and publication before admitting account storage or replacement.
    pub(super) fn drive(
        &mut self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
        idle: bool,
        mut worlds: Option<&mut LocalWorlds>,
    ) {
        if !self.retirement_complete(commands, menu) {
            return;
        }
        let sign_in_queued = menu.feeds.account_adding && menu.auth_restart_requested;
        if idle
            && (menu.accounts.operation.is_some() || sign_in_queued)
            && menu.auth_cleanup_complete()
        {
            if !self.retire_presence(commands, menu) {
                return;
            }
            if self
                .session_workers
                .as_ref()
                .is_some_and(|workers| !workers.is_empty())
            {
                return;
            }
            if self.running.take().is_some() {
                self.detach_clients(commands, menu, worlds.as_deref_mut());
            }
            if menu.accounts.operation.is_some() {
                menu.start_account_operation();
            } else {
                menu.start_sign_in_after_retirement();
            }
            return;
        }
        if menu.accounts.work.is_some() {
            return;
        }
        if idle && !menu.sign_in_in_flight() {
            self.drive_native(commands, menu, worlds);
        }
    }
}

mod native;
mod presence;

#[cfg(test)]
mod dead_child_tests;
#[cfg(test)]
mod retirement_tests;

#[cfg(all(test, feature = "native-networking"))]
mod sign_in_tests;
