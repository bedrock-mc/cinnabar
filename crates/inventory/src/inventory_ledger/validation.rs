//! Slot permissions shared by every inventory gesture builder.

use assets::facts::ContainerRoles;
use protocol::NetworkItemStack;
use protocol_ids::ContainerName;

use super::cells::{Cells, Held};
use super::equipment::equipment_admission;
use super::gesture::Submission;
use super::overlay::DeltaGroup;
use super::{Cell, InventoryGestureError, PlayerInventoryLedger};

/// Reports an equipment refusal separately from other invalid placements.
pub(super) const fn placement_refused(cell: Cell) -> InventoryGestureError {
    if matches!(cell, Cell::Armor(_) | Cell::Offhand) {
        InventoryGestureError::EquipmentRefused
    } else {
        InventoryGestureError::InvalidRequest
    }
}

impl PlayerInventoryLedger {
    /// Returns a slot-specific limit, zero for refusal, or no extra item limit.
    pub(super) fn placement_capacity(&self, cell: Cell, stack: &NetworkItemStack) -> Option<u16> {
        let roles = self.item_facts.get(stack.network_id).roles;
        match cell {
            Cell::Armor(slot) => Some(u16::from(
                equipment_admission(&self.item_facts, stack.network_id).armor == Some(slot),
            )),
            Cell::Offhand => (equipment_admission(&self.item_facts, stack.network_id).offhand
                != Some(true))
            .then_some(0),
            Cell::Craft(slot) => {
                match protocol::ui_slot_container_name(slot).and_then(ContainerName::from_wire) {
                    Some(ContainerName::EnchantingInput) => Some(1),
                    Some(ContainerName::EnchantingMaterial)
                        if !roles.contains(ContainerRoles::ENCHANTING_LAPIS) =>
                    {
                        Some(0)
                    }
                    Some(ContainerName::BeaconPayment) => {
                        Some(u16::from(roles.contains(ContainerRoles::BEACON_PAYMENT)))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Checks permissions against each atomic step before any request is retained.
    pub(super) fn validate_submission(
        &self,
        submission: &Submission,
    ) -> Result<(), InventoryGestureError> {
        let mut cells = self.view().clone();
        for group in &submission.groups {
            match group {
                DeltaGroup::Transfer {
                    source,
                    destination,
                    amount,
                    ..
                } => {
                    self.validate_removal(*source, cells.get(*source))?;
                    if let Some(from) = cells.get(*source) {
                        self.validate_placement(&cells, *destination, from, *amount)?;
                    }
                }
                DeltaGroup::Swap {
                    source,
                    destination,
                    ..
                } => {
                    self.validate_removal(*source, cells.get(*source))?;
                    self.validate_removal(*destination, cells.get(*destination))?;
                    let mut empty = cells.clone();
                    empty.set(*source, None);
                    empty.set(*destination, None);
                    if let Some(from) = cells.get(*source) {
                        self.validate_placement(&empty, *destination, from, from.stack.count)?;
                    }
                    if let Some(from) = cells.get(*destination) {
                        self.validate_placement(&empty, *source, from, from.stack.count)?;
                    }
                }
                DeltaGroup::Shrink { source, .. } => {
                    self.validate_removal(*source, cells.get(*source))?
                }
                DeltaGroup::Set { .. } | DeltaGroup::Retain { .. } => {}
            }
            if !group.apply(&mut cells) {
                return Err(InventoryGestureError::InvalidRequest);
            }
        }
        Ok(())
    }

    /// Refuses removal of slot-locked equipment, including swaps and drops.
    fn validate_removal(
        &self,
        cell: Cell,
        held: Option<&Held>,
    ) -> Result<(), InventoryGestureError> {
        if matches!(cell, Cell::Armor(_) | Cell::Offhand)
            && held.is_some_and(|held| protocol::item_extra_slot_locked(&held.stack.extra_data))
        {
            return Err(InventoryGestureError::SlotLocked);
        }
        Ok(())
    }

    /// Checks destination ownership and the remaining room permitted by its validator.
    fn validate_placement(
        &self,
        cells: &Cells,
        cell: Cell,
        held: &Held,
        amount: u16,
    ) -> Result<(), InventoryGestureError> {
        if cell != Cell::Cursor {
            self.check_target(cell)?;
        }
        if self
            .placement_capacity(cell, &held.stack)
            .is_some_and(|limit| {
                amount > limit.saturating_sub(cells.get(cell).map_or(0, |held| held.stack.count))
            })
        {
            return Err(placement_refused(cell));
        }
        Ok(())
    }
}
