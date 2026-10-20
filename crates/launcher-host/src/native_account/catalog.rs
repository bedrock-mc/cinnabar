//! One account operation for independent Realms and friend-world catalog sections.

use super::*;
use launcher::menu::view::MenuRealmCard;

/// Each section retains its own service failure; successful sections can still replace visible rows.
pub struct CatalogCards {
    /// Accessible Realms, without resolving endpoints or accepting membership.
    pub realms: io::Result<Vec<MenuRealmCard>>,
    /// Listed friend worlds, without joining their multiplayer sessions.
    pub friends: io::Result<Vec<super::NativeFriendWorld>>,
}

/// A complete read bound to the final persisted account generation and OAuth revision.
pub struct CatalogListing {
    pub(super) cards: CatalogCards,
    pub(super) generation: String,
    pub(super) binding: [u8; 32],
}

impl CatalogListing {
    /// Moves rows out only after the operation returned its current verified owner.
    /// The menu additionally checks its selected account and request epoch.
    pub fn into_current_cards(self, account: &NativeAccount) -> Option<CatalogCards> {
        let state = account.state.try_lock().ok()?;
        let cached = state.cached.as_ref()?;
        (!state.busy
            && cached.generation == self.generation
            && cached.binding == self.binding
            && cached
                .identity
                .as_ref()
                .is_some_and(|identity| identity.current_credential))
        .then_some(self.cards)
    }
}

impl NativeAccount {
    /// Reads both sections sequentially on one verified account/proof/Minecraft owner.
    /// Acquisition and each section have a finite bound. Dropping the future cancels
    /// service I/O and retains acquired OAuth rotation through Operation persistence.
    pub async fn list_catalog(self: Arc<Self>) -> io::Result<CatalogListing> {
        let mut operation = tokio::time::timeout(ACQUISITION_TIMEOUT, self.acquire_verified())
            .await
            .map_err(|_| timed_out())??;
        operation.promote()?;
        let realms = tokio::time::timeout(ACQUISITION_TIMEOUT, async {
            operation
                .minecraft()?
                .realms()
                .await
                .map_err(realms::realm_error)?
                .list()
                .await
                .map(|values| values.into_iter().map(realms::card).collect())
                .map_err(realms::realm_error)
        })
        .await
        .map_err(|_| timed_out())
        .and_then(std::convert::identity);
        // Keep a rotation durable even if the next independent section is canceled.
        operation.persist()?;
        // A later credential acquisition must still persist on cancellation or failure.
        operation.published = false;
        let xuid = operation
            .cached
            .as_ref()
            .expect("account owner")
            .identity
            .as_ref()
            .expect("verified identity")
            .xuid
            .clone();
        let friends = tokio::time::timeout(ACQUISITION_TIMEOUT, async {
            let activities = operation
                .minecraft()?
                .social()
                .activities(minecraft::SERVICE_CONFIG_ID, &[])
                .await
                .map_err(service_error)?;
            let generation = operation
                .cached
                .as_ref()
                .expect("account owner")
                .generation
                .clone();
            Ok(friends::listed(activities, &xuid)
                .into_iter()
                .map(|world| {
                    super::NativeFriendWorld::new(
                        world.card,
                        world.selection.map(|selection| {
                            super::NativeFriendSelection::capture(
                                Arc::clone(&operation.account),
                                generation.clone(),
                                xuid.clone(),
                                selection,
                            )
                        }),
                    )
                })
                .collect())
        })
        .await
        .map_err(|_| timed_out())
        .and_then(std::convert::identity);
        operation.persist()?;
        let cached = operation.cached.as_ref().expect("account owner");
        let listing = CatalogListing {
            cards: CatalogCards { realms, friends },
            generation: cached.generation.clone(),
            binding: cached.binding,
        };
        // The menu never races a still-borrowed SDK value when consuming this result.
        drop(operation);
        Ok(listing)
    }
}

/// Keeps account and service diagnostics out of the menu's recovery copy.
fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "native account catalog timed out")
}

#[cfg(test)]
mod tests;
