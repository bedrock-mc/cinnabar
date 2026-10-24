//! The selected account and its original Realm invitation operation.
use super::*;

#[derive(Clone, Default)]
pub(in crate::launcher_account) struct Source(
    pub(in crate::launcher_account) Option<Arc<crate::native_account::NativeAccount>>,
);

impl Source {
    /// Compares the captured account without acquiring credentials or blocking presentation.
    pub(super) fn same_account(&self, current: &Self) -> bool {
        match (&self.0, &current.0) {
            (Some(first), Some(second)) => Arc::ptr_eq(first, second),
            (None, None) => true,
            _ => false,
        }
    }
}

pub(super) struct Prepared(Box<crate::native_account::PreparedRealmMembership>);
pub(super) struct Loaded(crate::native_account::RealmMembershipResult);
pub(super) struct Reply {
    pub(super) code: String,
    pub(super) card: MenuRealmCard,
}

impl Source {
    /// Validates the invitation before acquiring the selected account's original lease.
    pub(super) async fn prepare(&self, input: &str) -> Result<Prepared, ()> {
        let code = launcher::menu::realm_membership::invite_code(input)
            .ok_or(())?
            .to_owned();
        Arc::clone(self.0.as_ref().ok_or(())?)
            .prepare_realm_membership(code)
            .await
            .map(|prepared| Prepared(Box::new(prepared)))
            .map_err(|_| ())
    }
}

impl Prepared {
    /// A possible acceptance retains its original bounded observation through cancellation.
    pub(super) fn observes_acceptance(&self, accept: bool) -> bool {
        accept
    }
    /// Uses the originally prepared service operation exactly once.
    pub(super) async fn run(self, accept: bool) -> Result<Loaded, ()> {
        (*self.0).run(accept).await.map(Loaded).map_err(|_| ())
    }
}

impl Loaded {
    /// Releases the reply only while the original account remains current.
    pub(super) async fn into_current(self) -> Result<Reply, ()> {
        self.0
            .into_current()
            .await
            .map(|(code, card)| Reply { code, card })
            .ok_or(())
    }
}
