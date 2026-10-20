//! An explicit invitation operation retaining the selected account's original lease.

use super::super::{read_identity::ReadIdentity, *};
use launcher::menu::view::MenuRealmCard;

/// One verified account acquired before the worker commits explicit acceptance intent.
pub(crate) struct PreparedRealmMembership {
    operation: Operation,
    identity: ReadIdentity,
    code: String,
}

/// A presentation result, not a transport or reusable membership authorization.
pub(crate) struct RealmMembershipResult {
    identity: ReadIdentity,
    code: String,
    realm: MenuRealmCard,
}

impl RealmMembershipResult {
    /// Waits through account contention under the worker's original cutoff, without repeating the service call.
    pub(crate) async fn into_current(self) -> Option<(String, MenuRealmCard)> {
        self.identity
            .wait_current()
            .await
            .then_some((self.code, self.realm))
    }
}

impl NativeAccount {
    /// Acquires the existing proof and credential lease before a cancel-safe membership preparation ends.
    pub(crate) async fn prepare_realm_membership(
        self: Arc<Self>,
        code: String,
    ) -> io::Result<PreparedRealmMembership> {
        let mut operation = self.acquire_verified().await?;
        operation.promote()?;
        let identity = ReadIdentity::capture(&operation);
        Ok(PreparedRealmMembership {
            operation,
            identity,
            code,
        })
    }
}

impl PreparedRealmMembership {
    /// Uses the existing service owner once; a caller must observe a possible acceptance write.
    /// Credential rotations persist on error, cancellation and success through the same Operation.
    pub(crate) async fn run(mut self, accept: bool) -> io::Result<RealmMembershipResult> {
        let response = {
            let mut realms = self
                .operation
                .minecraft()?
                .realms()
                .await
                .map_err(super::realm_error)?;
            if accept {
                realms.accept_invite(&self.code).await
            } else {
                realms.preview_invite(&self.code).await
            }
        };
        self.operation.persist()?;
        let mut realm = super::card(response.map_err(super::realm_error)?);
        // The existing preview card is actionable only after the UI's separate explicit acceptance.
        realm.member = true;
        drop(self.operation);
        Ok(RealmMembershipResult {
            identity: self.identity,
            code: self.code,
            realm,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        task::{Context, Waker},
    };

    /// Prepared intent owns the actual protected lease; a canceled second preparation cannot release it.
    #[tokio::test]
    async fn native_realm_membership_preparation_retains_the_original_account_lease() {
        let (_directory, account) = super::super::super::profile::tests::account();
        let prepared = Arc::clone(&account)
            .prepare_realm_membership("fixture".into())
            .await
            .unwrap();
        assert_eq!(
            account.store.try_native_lease().err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        let mut next = Box::pin(Arc::clone(&account).prepare_realm_membership("another".into()));
        assert!(
            next.as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        drop(next);
        assert!(account.state.lock().unwrap().busy);
        drop(prepared);
        assert!(!account.state.lock().unwrap().busy);
        let _lease = account.store.try_native_lease().unwrap();
        assert!(matches!(
            account.state.lock().unwrap().cached.as_ref().unwrap().owner,
            Owner::Minecraft(_)
        ));
    }

    /// A completed presentation survives another real operation on its unchanged account.
    /// The card is synthetic; this exercises final publication, without a Realms service request.
    #[tokio::test(start_paused = true)]
    async fn native_realm_membership_result_survives_same_account_contention() {
        let (_directory, account) = super::super::super::profile::tests::account();
        let PreparedRealmMembership {
            mut operation,
            identity,
            code,
        } = Arc::clone(&account)
            .prepare_realm_membership("fixture".into())
            .await
            .unwrap();
        operation.persist().unwrap();
        drop(operation);
        let result = RealmMembershipResult {
            identity,
            code,
            realm: serde_json::from_value(serde_json::json!({
                "name":"Confirmed", "state":"OPEN", "member":true
            }))
            .unwrap(),
        };
        let contender = Arc::clone(&account).acquire_verified().await.unwrap();
        let mut publishing = Box::pin(result.into_current());
        assert!(
            publishing
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending(),
            "ordinary account contention discarded the completed membership result"
        );
        drop(contender);
        let (code, realm) = publishing
            .await
            .expect("the original identity is still current");
        assert_eq!(code, "fixture");
        assert_eq!(realm.name, "Confirmed");
        assert!(realm.member);
    }

    /// A busy owner may return a different identity; waiting must still reject that result.
    #[tokio::test(start_paused = true)]
    async fn native_realm_membership_publication_rechecks_identity_after_contention() {
        let (_directory, account) = super::super::super::profile::tests::account();
        let mut original = Arc::clone(&account).acquire_verified().await.unwrap();
        let identity = ReadIdentity::capture(&original);
        original.persist().unwrap();
        drop(original);
        let mut contender = account.begin().unwrap();
        let mut publishing = Box::pin(identity.wait_current());
        assert!(
            publishing
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending(),
            "an acquired owner has temporarily taken the cached identity"
        );
        contender
            .cached
            .as_mut()
            .unwrap()
            .identity
            .as_mut()
            .unwrap()
            .xuid = "456".into();
        drop(contender);
        assert!(!publishing.await);
    }

    /// The caller's original cutoff still bounds a real account wait without canceling its owner.
    #[tokio::test(start_paused = true)]
    async fn native_realm_membership_publication_keeps_the_original_cutoff() {
        let (_directory, account) = super::super::super::profile::tests::account();
        let mut original = Arc::clone(&account).acquire_verified().await.unwrap();
        let identity = ReadIdentity::capture(&original);
        original.persist().unwrap();
        drop(original);
        let contender = account.begin().unwrap();
        let deadline = tokio::time::Instant::now() + LEASE_RETRY;
        assert!(
            tokio::time::timeout_at(deadline, identity.wait_current())
                .await
                .is_err()
        );
        assert!(account.state.lock().unwrap().busy);
        drop(contender);
        assert!(identity.current());
    }
}
