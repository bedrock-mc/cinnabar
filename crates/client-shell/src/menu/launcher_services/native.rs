//! Native attachment shares the original service, account and world retirement owner.

use super::*;
use std::sync::Arc;

impl LauncherServices {
    /// Attaches only after the original publisher has retired.
    pub(super) fn drive_native(
        &mut self,
        commands: &mut Commands,
        menu: &mut MenuRuntime,
        worlds: Option<&mut LocalWorlds>,
    ) {
        let cache = menu.launcher_auth_cache();
        let identity = menu.selected_account_id().map(str::to_owned);
        if let Some(Attachment {
            cache: previous,
            identity: selected,
            account: retained,
        }) = &self.running
            && *previous == cache
            && *selected == identity
            && match (retained, cache.as_deref()) {
                (None, None) => true,
                (Some(retained), Some(path)) => menu
                    .native_catalog
                    .selected_account(path, identity.as_deref())
                    .is_some_and(|selected| Arc::ptr_eq(retained, selected)),
                _ => false,
            }
        {
            return;
        }
        if self.running.is_some() {
            if !self.retire_presence(commands, menu) {
                return;
            }
            self.detach_clients(commands, menu, worlds);
            self.running = None;
            return;
        }
        let execution = match menu.io_execution() {
            Ok(execution) => execution,
            Err(error) => {
                menu.catalog_message = Some(error.to_string());
                return;
            }
        };
        let account = cache.clone().map(|path| menu.native_account_for(path));
        let services = launcher_host::launcher_account::LauncherAccount::native(
            menu.layout.launcher_artwork_dir(),
            account.clone(),
            &pack_runtime::pack_language::active_language_code(),
            execution,
        );
        self.presence_lifetime = Some(services.title_presence_lifetime());
        self.workers = Some(services.worker_lifetime());
        self.running = Some(Attachment {
            cache,
            identity,
            account,
        });
        commands.insert_resource(LauncherAccount(services));
        if let Some(worlds) = worlds
            && let Err(error) = worlds.attach_native(&menu.layout)
        {
            bevy::log::warn!("local worlds unavailable: {error}");
        }
    }
}
