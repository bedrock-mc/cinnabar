//! Equipment admission from cached session item facts.

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
