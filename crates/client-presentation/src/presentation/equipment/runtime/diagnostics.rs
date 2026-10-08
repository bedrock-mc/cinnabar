//! Why a worn or held item drew no layer, logged once per identifier and reason.

use super::*;

/// Distinct `(identifier, reason)` lines kept; later misses go unlogged.
const MAX_LOGGED_MISSES: usize = 512;

impl EquipmentRuntime {
    /// Logs the first unmet precondition for an item that produced no layer.
    pub(super) fn note_missing_layer(
        &mut self,
        item: &WornItem,
        slot: Option<ArmorSlot>,
        bone: Option<usize>,
    ) {
        let reason = self.missing_reason(item, slot, bone);
        if self.logged_misses.len() >= MAX_LOGGED_MISSES
            || !self
                .logged_misses
                .insert((item.identifier.as_ref().into(), reason))
        {
            return;
        }
        bevy::log::info!(
            identifier = item.identifier.as_ref(),
            metadata = item.metadata,
            slot = ?slot,
            kind = ?item.kind,
            "equipment drew no layer: {reason}"
        );
    }

    fn missing_reason(
        &self,
        item: &WornItem,
        slot: Option<ArmorSlot>,
        bone: Option<usize>,
    ) -> &'static str {
        let source = self.binding_source(&item.identifier);
        let binding = source.as_ref().and_then(|(catalog, from_pack)| {
            Some((catalog.binding(&item.identifier)?, *from_pack))
        });
        if let Some(slot) = slot {
            let Some((binding, from_pack)) = binding else {
                return if matches!(item.kind, HeldKind::Block(_)) {
                    "worn block has no cube sheet or head bone"
                } else {
                    "no attachable armor binding for this identifier"
                };
            };
            let category = binding.category;
            if category != (EquipmentCategory::Armor { slot })
                && category != EquipmentCategory::Elytra
            {
                return "attachable binding is not armor for this slot";
            }
            let texture = self.texture_location(&binding.texture.identifier, from_pack);
            return match (from_pack, texture.is_some()) {
                (true, false) => "server-pack armor binding texture is not on the artwork pages",
                (false, false) => "vanilla armor binding texture is not on the artwork pages",
                (true, true) => "server-pack armor geometry is unavailable or unplaceable",
                (false, true) => "vanilla armor geometry is unavailable or unplaceable",
            };
        }
        if bone.is_none() {
            return "body has no item bone for this hand";
        }
        match item.kind {
            HeldKind::Sprite
                if self
                    .icons
                    .lookup_index(&item.identifier, item.metadata)
                    .is_none() =>
            {
                "no icon sprite for this identifier in the vanilla icon carrier"
            }
            HeldKind::Sprite => "icon sprite has no atlas placement or mesh",
            HeldKind::Block(_) => "block item has no cube sheet",
            HeldKind::Other => "item has no drawable visual route",
        }
    }
}
