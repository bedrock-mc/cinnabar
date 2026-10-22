//! Chooses equipment definitions before deciding which model can supply a fallback.

use super::*;

/// The equipment location whose owner context controls definition admission.
pub(super) enum EquipmentSlot {
    Held { off_hand: bool },
    Worn(ArmorSlot),
}

/// A drawable equipment category or its ordinary mesh fallback after owner admission.
pub(super) enum EquipmentRoute {
    NativeHeld,
    Held(Option<pack::BindingSource>),
    Skull(SkullKind),
    HeadBlock,
    Armor(pack::BindingSource),
    Elytra(pack::BindingSource),
    Hidden,
}

impl EquipmentRuntime {
    /// Resolves one admitted definition before choosing the held, worn or block fallback.
    pub(super) fn equipment_route(
        &mut self,
        item: &WornItem,
        equipment: &ActorEquipmentInput,
        animation: Option<EquipmentAnimation<'_>>,
        slot: EquipmentSlot,
    ) -> EquipmentRoute {
        let slot = match slot {
            EquipmentSlot::Worn(slot) => slot,
            EquipmentSlot::Held { off_hand } => {
                if self.uses_native_item_mesh(&item.identifier) {
                    return EquipmentRoute::NativeHeld;
                }
                let source = self
                    .held_binding_source(item, equipment, animation, off_hand)
                    .filter(|source| {
                        matches!(
                            self.effective_category(
                                &item.identifier,
                                source.catalog.bindings()[source.index].category
                            ),
                            EquipmentCategory::Held
                                | EquipmentCategory::ArmorAndHeld { .. }
                                | EquipmentCategory::Shield
                        )
                    });
                return EquipmentRoute::Held(source);
            }
        };
        if slot == ArmorSlot::Helmet
            && let Some(kind) = skull_kind(&item.identifier)
        {
            return EquipmentRoute::Skull(kind);
        }
        let Some(source) = self.worn_binding_source(item, equipment, animation, slot) else {
            return if slot == ArmorSlot::Helmet && matches!(item.kind, HeldKind::Block(_)) {
                EquipmentRoute::HeadBlock
            } else {
                EquipmentRoute::Hidden
            };
        };
        match self.effective_category(
            &item.identifier,
            source.catalog.bindings()[source.index].category,
        ) {
            EquipmentCategory::Elytra if slot == ArmorSlot::Chestplate => {
                EquipmentRoute::Elytra(source)
            }
            EquipmentCategory::Armor { slot: admitted }
            | EquipmentCategory::ArmorAndHeld { slot: admitted }
                if slot == admitted =>
            {
                EquipmentRoute::Armor(source)
            }
            _ => EquipmentRoute::Hidden,
        }
    }
}
