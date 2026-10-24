//! Actor use reads admitted class, behavior and item facts together.

use super::{ActorSnapshot, ActorStore};
use assets::entity::offered_use::{ActorUseResolver, HandItem, HeldItems, OfferedUse};

/// One equipped stack's identity; network id 0 is an empty hand.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HandStack {
    pub network_id: i32,
    pub metadata: u32,
}

impl HandStack {
    /// Reads an inventory stack, treating a zero count as empty.
    #[must_use]
    pub const fn of(stack: &protocol::NetworkItemStack) -> Self {
        if stack.is_empty() {
            Self {
                network_id: 0,
                metadata: 0,
            }
        } else {
            Self {
                network_id: stack.network_id,
                metadata: stack.metadata,
            }
        }
    }

    /// Reads the admitted held stack, treating a zero count as empty.
    #[must_use]
    pub fn verified(stack: &protocol::VerifiedNetworkItemStack) -> Self {
        if stack.is_empty() {
            Self::default()
        } else {
            Self {
                network_id: stack.network_id(),
                metadata: stack.metadata(),
            }
        }
    }
}

/// The interacting player's equipment as actor-use admission reads it.
#[derive(Clone, Copy, Debug)]
pub struct UseEquipment {
    pub main_hand: HandStack,
    /// `None` while the offhand is unobserved.
    pub offhand: Option<HandStack>,
    /// Enchantments across both hands and the armor, from [`equipment_enchantments`].
    pub enchantments: Option<u64>,
}

/// Folds the enchantment ids in every equipped stack's user data into one mask; an
/// unobserved slot (`None`) leaves the mask unknown.
pub fn equipment_enchantments<'a>(
    extra_data: impl IntoIterator<Item = Option<&'a [u8]>>,
) -> Option<u64> {
    extra_data.into_iter().try_fold(0, |mask, extra_data| {
        Some(
            protocol::item_display(extra_data?)
                .enchantments
                .iter()
                .filter_map(|(id, _)| u32::try_from(*id).ok().and_then(|id| 1u64.checked_shl(id)))
                .fold(mask, |mask, bit| mask | bit),
        )
    })
}

impl ActorStore {
    /// Proves absence only for a known component resolver and unmodified retail held items.
    pub(crate) fn offered_use(
        &self,
        actor: &ActorSnapshot,
        equipment: &UseEquipment,
    ) -> OfferedUse {
        if matches!(actor.kind, protocol::ActorKind::Player { .. }) {
            return OfferedUse::Absent;
        }
        if actor.entity_fact.actor_use_resolver != ActorUseResolver::Components {
            return OfferedUse::Unresolved;
        }
        let protocol::ActorKind::Entity { identifier } = &actor.kind else {
            return OfferedUse::Unresolved;
        };
        let main_hand = self.hand_item(Some(equipment.main_hand));
        // A modified held item may carry a role its identity does not reveal.
        if main_hand == HandItem::Unknown {
            return OfferedUse::Unresolved;
        }
        let held = HeldItems {
            main_hand,
            offhand: self.hand_item(equipment.offhand),
            equipment_enchantments: equipment.enchantments,
        };
        self.entity_assets
            .as_deref()
            .and_then(|assets| assets.offered_use_for(identifier))
            .map_or(OfferedUse::Unresolved, |definition| {
                definition.for_held(&held)
            })
    }

    /// A server-modified or unregistered item keeps every condition on its hand open.
    fn hand_item(&self, stack: Option<HandStack>) -> HandItem<'_> {
        match stack {
            None => HandItem::Unknown,
            Some(HandStack { network_id: 0, .. }) => HandItem::Empty,
            Some(stack) => self.items.native_identifier(stack.network_id).map_or(
                HandItem::Unknown,
                |identifier| HandItem::Item {
                    identifier,
                    metadata: stack.metadata,
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests;
