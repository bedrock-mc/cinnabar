//! Generation-bound title presence borrowing the existing leased SDK owner.
use super::*;
use bedrock_services::xbox::presence::{RichPresence, State as PresenceState, TitleUpdate};

/// Original generation and verified account; no credential or independent SDK cache is retained.
#[derive(Clone)]
pub(crate) struct PresenceScope {
    generation: String,
    xuid: String,
}

impl NativeAccount {
    /// Binds the account before any title write, preserving refresh durability on cancellation.
    pub(crate) async fn bind_title_presence(self: Arc<Self>) -> io::Result<PresenceScope> {
        let mut operation = self.acquire_verified().await?;
        let cached = operation.cached.as_ref().expect("acquired account");
        let scope = PresenceScope {
            generation: cached.generation.clone(),
            xuid: cached
                .identity
                .as_ref()
                .expect("verified account")
                .xuid
                .clone(),
        };
        operation.persist()?;
        Ok(scope)
    }

    /// Performs one original-generation update, retaining final dispatch evidence if the caller cancels.
    /// The caller owns the absolute deadline and the evidence beyond this borrowed future.
    pub(crate) async fn update_title_presence(
        self: Arc<Self>,
        scope: &PresenceScope,
        state: &bridge::XboxPresenceState,
        dispatched: &mut bool,
    ) -> io::Result<Option<Duration>> {
        let mut operation = self.acquire_verified_for(Some(&scope.generation)).await?;
        operation.check_presence_scope(scope)?;
        operation.rearm()?;
        let id = state.activity_id();
        let rich = RichPresence::new(
            uuid::Uuid::from_u128(u128::from(bridge::contract::PRESENCE_TITLE_ID)),
            &id,
        )
        .map_err(service_error)?;
        let update = TitleUpdate::new(PresenceState::Active).with_rich_presence(rich);
        let result = match &mut operation.cached.as_mut().expect("account owner").owner {
            Owner::Xbox(account) => {
                account
                    .title_presence()
                    .update_with_dispatch(&update, || *dispatched = true)
                    .await
            }
            Owner::Minecraft(services) => {
                services
                    .title_presence()
                    .update_with_dispatch(&update, || *dispatched = true)
                    .await
            }
        }
        .map_err(service_error);
        operation.persist()?;
        result.map(|result| result.heartbeat_after)
    }

    /// Removes only the original generation's title once; changed credentials never select a new account.
    pub(crate) async fn remove_title_presence(
        self: Arc<Self>,
        scope: &PresenceScope,
    ) -> io::Result<()> {
        let mut operation = self.acquire_verified_for(Some(&scope.generation)).await?;
        operation.check_presence_scope(scope)?;
        operation.rearm()?;
        let result = match &mut operation.cached.as_mut().expect("account owner").owner {
            Owner::Xbox(account) => account.title_presence().remove().await,
            Owner::Minecraft(services) => services.title_presence().remove().await,
        }
        .map_err(service_error);
        operation.persist()?;
        result
    }
}

impl Operation {
    /// Refuses a new account before any publication or removal can be dispatched.
    fn check_presence_scope(&self, scope: &PresenceScope) -> io::Result<()> {
        let cached = self.cached.as_ref().expect("acquired account");
        if cached.generation != scope.generation
            || cached
                .identity
                .as_ref()
                .is_none_or(|id| !id.current_credential || id.xuid != scope.xuid)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native presence account retired",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
