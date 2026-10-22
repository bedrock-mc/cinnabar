//! Actor use reads admitted class, behavior and item facts together.

use super::{ActorSnapshot, ActorStore};
use assets::entity::offered_use::{ActorUseResolver, OfferedUse};

impl ActorStore {
    /// Proves absence only for a known component resolver and an unchanged retail throwable.
    pub(crate) fn offered_throwable_use(
        &self,
        actor: &ActorSnapshot,
        item: &protocol::VerifiedNetworkItemStack,
    ) -> OfferedUse {
        if matches!(actor.kind, protocol::ActorKind::Player { .. }) {
            return OfferedUse::Absent;
        }
        if actor.entity_fact.actor_use_resolver != ActorUseResolver::Components {
            return OfferedUse::Unresolved;
        }
        let Some(item_identifier) = self.items.retail_throwable_identifier(item.network_id())
        else {
            return OfferedUse::Unresolved;
        };
        let protocol::ActorKind::Entity { identifier } = &actor.kind else {
            return OfferedUse::Unresolved;
        };
        self.entity_assets
            .as_deref()
            .and_then(|assets| assets.offered_use_for(identifier))
            .map_or(OfferedUse::Unresolved, |definition| {
                definition.for_throwable(Some(item_identifier), item.metadata())
            })
    }
}

#[cfg(test)]
mod tests;
