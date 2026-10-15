//! Equipment admission from cached session item facts.

use protocol::NetworkItemStack;

use super::{Cell, PlayerInventoryLedger};

/// Effective equipment roles; unresolved declarations do not grant admission.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct EquipmentAdmission {
    pub armor: Option<u8>,
    pub offhand: Option<bool>,
}

/// Reads equipment permission without reparsing an item declaration.
pub(super) fn equipment_admission(
    facts: &crate::SessionItemFacts,
    network_id: i32,
) -> EquipmentAdmission {
    EquipmentAdmission {
        armor: facts.armour_row(network_id),
        offhand: facts.get(network_id).allow_off_hand,
    }
}

impl PlayerInventoryLedger {
    /// Returns zero for refused equipment, one for admitted armor, or no extra stack limit.
    pub(super) fn equipment_capacity(&self, cell: Cell, stack: &NetworkItemStack) -> Option<u16> {
        let admission = || equipment_admission(&self.item_facts, stack.network_id);
        match cell {
            Cell::Armor(slot) => Some(u16::from(admission().armor == Some(slot))),
            Cell::Offhand => (admission().offhand != Some(true)).then_some(0),
            _ => None,
        }
    }
}
