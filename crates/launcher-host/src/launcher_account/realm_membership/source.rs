//! The membership UI's selected existing account, captured before queueing a request.

use super::*;

#[derive(Clone, Default)]
pub(in crate::launcher_account) enum Source {
    #[default]
    Core,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(Option<Arc<crate::native_account::NativeAccount>>),
}

pub(super) enum Prepared {
    Core(String),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(Box<crate::native_account::PreparedRealmMembership>),
}

pub(super) enum Loaded {
    Core(bridge::RealmMembership),
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(crate::native_account::RealmMembershipResult),
}

pub(super) struct Reply {
    pub(super) code: String,
    pub(super) card: MenuRealmCard,
    pub(super) catalog: Option<Realm>,
}

impl Source {
    /// Validates the input before credentials and retains the same native lease for explicit intent.
    pub(super) async fn prepare(&self, input: &str) -> Result<Prepared, ()> {
        let code = launcher::menu::realm_membership::invite_code(input)
            .ok_or(())?
            .to_owned();
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if let Self::Native(account) = self {
            return match account {
                Some(account) => Arc::clone(account)
                    .prepare_realm_membership(code)
                    .await
                    .map(|prepared| Prepared::Native(Box::new(prepared)))
                    .map_err(|_| ()),
                None => Err(()),
            };
        }
        Ok(Prepared::Core(code))
    }

    /// Matches the concrete selected account without service I/O on the launcher frame.
    pub(super) fn same_account(&self, current: &Self) -> bool {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        match (self, current) {
            (Self::Core, Self::Core) => true,
            (Self::Native(first), Self::Native(second)) => match (first, second) {
                (Some(first), Some(second)) => Arc::ptr_eq(first, second),
                (None, None) => true,
                _ => false,
            },
            _ => false,
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        {
            let (Self::Core, Self::Core) = (self, current);
            true
        }
    }
}

impl Prepared {
    /// Native acceptance may have applied and must not be canceled and silently submitted again.
    pub(super) fn observes_acceptance(&self, accept: bool) -> bool {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        {
            accept && matches!(self, Self::Native(_))
        }
        #[cfg(not(all(feature = "native-networking", not(target_arch = "wasm32"))))]
        {
            let _ = accept;
            false
        }
    }

    /// Invokes only the selected existing service method; neither outcome starts a game connection.
    pub(super) async fn run(
        self,
        socket_dir: Option<&std::path::Path>,
        accept: bool,
    ) -> Result<Loaded, ()> {
        match self {
            Self::Core(code) => bridge::realm_membership(
                super::super::workers::require_core_directory(socket_dir).map_err(|_| ())?,
                &code,
                accept,
            )
            .await
            .map(Loaded::Core)
            .map_err(|_| ()),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(prepared) => (*prepared)
                .run(accept)
                .await
                .map(Loaded::Native)
                .map_err(|_| ()),
        }
    }
}

impl Loaded {
    /// Releases presentation only while the original verified native account remains current.
    pub(super) async fn into_current(self) -> Result<Reply, ()> {
        match self {
            Self::Core(result) => Ok(Reply {
                card: realm_card(&result.realm),
                code: result.code,
                catalog: Some(result.realm),
            }),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(result) => result
                .into_current()
                .await
                .map(|(code, card)| Reply {
                    code,
                    card,
                    catalog: None,
                })
                .ok_or(()),
        }
    }
}
