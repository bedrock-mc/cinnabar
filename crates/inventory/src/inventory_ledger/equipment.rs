//! Equipment admission from retained item declarations and stock defaults.

use protocol::{ItemRegistryEntry, ItemRegistryVersion, NetworkItemStack};

use super::{Cell, PlayerInventoryLedger, item_roles};

/// Effective equipment roles; unresolved declarations do not grant admission.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct EquipmentAdmission {
    pub armor: Option<u8>,
    pub offhand: Option<bool>,
}

/// Resolves declarations before stock compatibility, with offhand wearables
/// admitting the offhand even when a separate permission says false.
pub(super) fn equipment_admission(entry: Option<&ItemRegistryEntry>) -> EquipmentAdmission {
    let Some(entry) = entry else {
        return EquipmentAdmission::default();
    };
    if matches!(entry.version, ItemRegistryVersion::Unknown(_)) {
        return EquipmentAdmission::default();
    }
    let components = entry.components.as_deref();
    let wearable = components.and_then(|components| components.wearable_slot.as_deref());
    let armor = match components.filter(|components| components.wearable_declared) {
        Some(_) => match wearable {
            Some("slot.armor.head") => Some(0),
            Some("slot.armor.chest") => Some(1),
            Some("slot.armor.legs") => Some(2),
            Some("slot.armor.feet") => Some(3),
            Some("slot.armor.body") => Some(4),
            _ => None,
        },
        None if !entry.component_based && entry.identifier.starts_with("minecraft:") => {
            item_roles::armor_row(&entry.identifier)
        }
        None => None,
    };
    let offhand = if wearable == Some("slot.weapon.offhand") {
        Some(true)
    } else if let Some(components) =
        components.filter(|components| components.allow_off_hand_declared)
    {
        components.allow_off_hand
    } else if !entry.component_based && item_roles::stock_offhand(&entry.identifier) {
        Some(true)
    } else {
        None
    };
    EquipmentAdmission { armor, offhand }
}

impl PlayerInventoryLedger {
    /// How many items of `stack` may rest in `cell`: `Some(0)` when an armor
    /// or offhand cell refuses the item, `Some(1)` for an armor cell that
    /// admits it, and `None` when only the stack's own capacity applies.
    pub(super) fn equipment_capacity(&self, cell: Cell, stack: &NetworkItemStack) -> Option<u16> {
        let admission = || equipment_admission(self.negotiated_item_entry(stack.network_id));
        match cell {
            Cell::Armor(slot) => Some(u16::from(admission().armor == Some(slot))),
            Cell::Offhand => (admission().offhand != Some(true)).then_some(0),
            _ => None,
        }
    }
}
