//! Leased credential acquisition and verified selected-account admission.

use super::*;

impl NativeAccount {
    /// Validates the selected saved account without minting a Minecraft service credential.
    /// Cancellation retains any SDK-visible OAuth rotation through the operation's existing Drop owner.
    pub async fn validate(self: Arc<Self>) -> io::Result<launcher::accounts::AccountProfile> {
        self.validate_selected(None).await
    }

    /// Keeps an interactive saved-account continuation on its original leased generation.
    pub(crate) async fn validate_generation(
        self: Arc<Self>,
        generation: &str,
    ) -> io::Result<launcher::accounts::AccountProfile> {
        self.validate_selected(Some(generation)).await
    }

    /// Shares the original verification and publication owner with a captured optional generation fence.
    async fn validate_selected(
        self: Arc<Self>,
        generation: Option<&str>,
    ) -> io::Result<launcher::accounts::AccountProfile> {
        tokio::time::timeout(ACQUISITION_TIMEOUT, async {
            let mut operation = self.acquire_verified_for(generation).await?;
            let identity = operation
                .cached
                .as_ref()
                .expect("account owner")
                .identity
                .as_ref()
                .expect("verified identity");
            let profile = launcher::accounts::AccountProfile::from_identity(
                &identity.xuid,
                &identity.gamertag,
            )?;
            operation.persist()?;
            let profile = operation
                .lease
                .as_mut()
                .expect("acquired lease")
                .remember_verified_profile(&profile)?;
            let lease = operation.lease.as_ref().expect("acquired lease");
            let cached = operation.cached.as_mut().expect("account owner");
            cached.binding = lease.binding();
            cached.checkpoint_revision = lease.checkpoint_revision();
            Ok(profile)
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "native account validation timed out",
            )
        })?
    }

    /// Acquires and verifies one concrete account owner before either validation or service minting.
    pub(super) async fn acquire_verified(self: Arc<Self>) -> io::Result<Operation> {
        self.acquire_verified_for(None).await
    }

    /// Rejects a retired service selection immediately after leasing, before restoring or acquiring credentials.
    pub(super) async fn acquire_verified_for(
        self: Arc<Self>,
        expected_generation: Option<&str>,
    ) -> io::Result<Operation> {
        let mut operation = loop {
            match self.begin() {
                Ok(operation) => break operation,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(LEASE_RETRY).await
                }
                Err(error) => return Err(error),
            }
        };
        operation.lease = Some(loop {
            match self.store.try_native_lease() {
                Ok(lease) => break lease,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(LEASE_RETRY).await
                }
                Err(error) => {
                    // No new request began; retired or unreadable storage must also release cached secrets.
                    operation.cached = None;
                    return Err(error);
                }
            }
        });
        let lease = operation.lease.as_ref().expect("acquired lease");
        if expected_generation.is_some_and(|expected| expected != lease.generation()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native service account generation changed",
            ));
        }
        if self
            .selected_id
            .as_deref()
            .is_some_and(|selected| lease.selected_id() != Some(selected))
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "selected native account changed",
            ));
        }
        let policy = authentication_policy();
        if lease.client_id().is_some_and(|id| id != policy.client_id) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "saved account uses another OAuth policy",
            ));
        }
        if operation
            .cached
            .as_ref()
            .is_some_and(|cached| cached.generation != lease.generation())
        {
            // A new explicit join may use the new sign-in, but no credential from the retired owner survives.
            operation.cached = None;
        }
        if operation
            .cached
            .as_ref()
            .is_none_or(|cached| cached.checkpoint_revision != lease.checkpoint_revision())
        {
            let mut identity = operation.cached.take().and_then(|cached| cached.identity);
            if let Some(identity) = &mut identity {
                identity.current_credential = false;
            }
            let account = restore::restore_account(lease, &policy)?;
            operation.cached = Some(Cached {
                provider: None,
                generation: lease.generation().to_owned(),
                binding: lease.binding(),
                checkpoint_revision: lease.checkpoint_revision(),
                owner: Owner::Xbox(Box::new(account)),
                identity,
            });
        } else if let Some(cached) = operation.cached.as_mut()
            && cached.binding != lease.binding()
        {
            cached.adopt_credentials(import_credentials(lease, &policy)?, lease.binding())?;
        }

        if let Some(identity) = operation
            .cached
            .as_ref()
            .expect("account owner")
            .identity
            .as_ref()
            && operation
                .lease
                .as_ref()
                .expect("acquired lease")
                .selected_id()
                .is_some_and(|selected| selected != identity.xuid)
        {
            operation.rejected = true;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native account identity does not match the selection",
            ));
        }

        if operation
            .cached
            .as_ref()
            .expect("account owner")
            .identity
            .as_ref()
            .is_none_or(|identity| !identity.current_credential)
        {
            operation.rearm()?;
            let result = operation
                .cached
                .as_mut()
                .expect("account owner")
                .owner
                .xbox_authorization()
                .await;
            let authorization = match result {
                Err(error)
                    if error
                        .get_ref()
                        .is_some_and(|cause| cause.is::<xbox::AccountCreationRequired>()) =>
                {
                    // A browser continuation may outlive this borrow only after its same proof is durable.
                    operation.persist()?;
                    return Err(error);
                }
                result => result?,
            };
            operation.verify_identity(authorization.xuid(), authorization.gamertag())?;
        }
        Ok(operation)
    }
}
