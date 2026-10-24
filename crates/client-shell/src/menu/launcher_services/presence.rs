//! Original title-publisher retirement and account-service detachment.
use super::*;

impl LauncherServices {
    /// Keeps replacements blocked until the original requested retirement is confirmed.
    pub(super) fn retirement_complete(
        &mut self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
    ) -> bool {
        !self.retirement_blocked
            && (self
                .presence_lifetime
                .as_ref()
                .is_none_or(|life| !life.retirement_requested())
                || self.retire_presence(commands, menu))
    }

    /// Cancels account readers before the original publisher needs their shared lease for cleanup.
    /// A failed removal or lost worker permanently holds this slot; it never resets authority.
    pub(super) fn retire_presence(
        &mut self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
    ) -> bool {
        use launcher_host::launcher_account::TitlePresenceRetirement;
        commands.remove_resource::<LauncherAccount>();
        let catalog_finished = menu.native_catalog.retirement_complete();
        menu.accounts.skip_control = true;
        if let Some(lifetime) = &self.presence_lifetime {
            lifetime.retire();
            match lifetime.retirement() {
                None => return false,
                Some(TitlePresenceRetirement::Complete) if lifetime.worker_finished() => {}
                Some(TitlePresenceRetirement::Complete) => return false,
                Some(TitlePresenceRetirement::Unconfirmed) => {
                    self.retirement_blocked = true;
                    bevy::log::warn!(
                        "native title presence cleanup unconfirmed; account replacement remains blocked"
                    );
                    return false;
                }
            }
        }
        if !catalog_finished
            || self
                .workers
                .as_ref()
                .is_some_and(|workers| !workers.is_empty())
        {
            return false;
        }
        self.presence_lifetime = None;
        self.workers = None;
        true
    }

    /// Detaches readers before a replacement account service can attach.
    pub(super) fn detach_clients(
        &self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
        worlds: Option<&mut LocalWorlds>,
    ) {
        commands.remove_resource::<LauncherAccount>();
        menu.close_realm_membership();
        if let Some(worlds) = worlds {
            worlds.detach();
        }
        menu.control_auth = None;
        menu.accounts.skip_control = true;
        menu.feeds.profile = Default::default();
    }
}
