//! Selection lifetime for the native account cached across retries and transfers.

use super::{MenuRuntime, SessionController};
use launcher_host::native_account::NativeAccount;
use std::{io, path::PathBuf, sync::Arc};

pub(super) struct SelectedAccount {
    path: PathBuf,
    identity: Option<String>,
    account: Arc<NativeAccount>,
}

impl SessionController {
    /// Captures startup's saved selection before its original worker verifies credentials.
    /// An absent cache permits offline direct play; errors never select an unsigned fallback.
    pub(crate) fn native_startup_account(
        &mut self,
        menu: &mut MenuRuntime,
        endpoint: Option<&protocol::SessionEndpoint>,
    ) -> io::Result<Option<Arc<NativeAccount>>> {
        let Some(protocol::SessionEndpoint::Native(target)) = endpoint else {
            return Ok(None);
        };
        if !matches!(
            target.transport,
            protocol::NativeTransport::RakNet
                | protocol::NativeTransport::Addressed
                | protocol::NativeTransport::NetherNetLan
        ) {
            return Ok(None);
        }
        if menu.account_operation_pending() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the selected account is changing",
            ));
        }
        let path = menu.layout().auth_cache();
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(self.native_account_for(menu, path))),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the saved account cache is not a file",
            )),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && menu.selected_account_id().is_none() =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Reuses one SDK owner until the selected cache/identity changes, without frame I/O.
    pub(super) fn native_account_for(
        &mut self,
        menu: &mut MenuRuntime,
        path: PathBuf,
    ) -> Arc<NativeAccount> {
        let account = menu.native_account_for(path.clone());
        let identity = menu.selected_account_id().map(str::to_owned);
        if self.native_account.as_ref().is_none_or(|previous| {
            previous.path != path
                || previous.identity != identity
                || !Arc::ptr_eq(&previous.account, &account)
        }) {
            self.native_account = Some(SelectedAccount {
                path,
                identity,
                account,
            });
        }
        Arc::clone(
            &self
                .native_account
                .as_ref()
                .expect("selected native account owner")
                .account,
        )
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
