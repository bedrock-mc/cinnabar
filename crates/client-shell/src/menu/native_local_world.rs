//! Original local-process selection passed to the existing native session worker.

use super::*;
use launcher_host::{local_worlds::NativeLocalWorldSelection, native_account::NativeAccount};
use std::sync::Arc;

/// One user-opened world and the account selected when its actual process became ready.
pub struct NativeLocalWorldIntent {
    selection: NativeLocalWorldSelection,
    account: Option<Arc<NativeAccount>>,
    retirement: crate::local_worlds::NativeLocalWorldRetirement,
    path: Option<PathBuf>,
    identity: Option<String>,
}

impl std::fmt::Debug for NativeLocalWorldIntent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("NativeLocalWorldIntent")
    }
}

impl NativeLocalWorldIntent {
    /// Transfers the exact original process and optional account; display names never locate it.
    pub fn into_parts(
        self,
    ) -> (
        NativeLocalWorldSelection,
        Option<Arc<NativeAccount>>,
        crate::local_worlds::NativeLocalWorldRetirement,
    ) {
        (self.selection, self.account, self.retirement)
    }
}

impl MenuRuntime {
    /// Selects native local play without the Core connection/report acknowledgement path.
    pub(super) fn request_native_local_world(
        &mut self,
        worlds: &mut crate::local_worlds::LocalWorlds,
        id: &str,
        name: String,
    ) {
        self.request_local_world_join(name, false);
        self.intents.join = None;
        let result = (|| {
            let selection = worlds.native_selection(id)?;
            let retirement = worlds.native_retirement(selection.clone())?;
            if self.account_change_pending() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "local world account selection is changing",
                ));
            }
            let path = self.launcher_auth_cache();
            let identity = self.selected_account_id().map(str::to_owned);
            if identity.is_some() && path.is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "selected local world account is unavailable",
                ));
            }
            let account = path
                .as_ref()
                .map(|path| self.native_account_for(path.clone()));
            Ok(NativeLocalWorldIntent {
                selection,
                account,
                retirement,
                path,
                identity,
            })
        })();
        match result {
            Ok(intent) => self.intents.native_world = Some(intent),
            Err(error) => {
                worlds.leave_world();
                self.cancel_join();
                self.show_join_failure(error.to_string());
            }
        }
    }

    /// Revokes a pending action if account selection or its original process has changed.
    pub fn take_native_local_world_intent(&mut self) -> Option<NativeLocalWorldIntent> {
        let intent = self.intents.native_world.take()?;
        if !self.native_sign_in
            || self.account_change_pending()
            || intent.path != self.launcher_auth_cache()
            || intent.identity.as_deref() != self.selected_account_id()
            || intent.selection.validate().is_err()
        {
            self.cancel_join();
            self.show_join_failure("Local world selection changed before joining.".into());
            return None;
        }
        if let (Some(path), Some(account)) = (&intent.path, &intent.account)
            && !Arc::ptr_eq(account, &self.native_account_for(path.clone()))
        {
            self.cancel_join();
            self.show_join_failure("Local world account changed before joining.".into());
            return None;
        }
        Some(intent)
    }
}
