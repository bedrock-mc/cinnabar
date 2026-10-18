//! Slot permissions shared by every inventory gesture builder.

use assets::facts::ContainerRoles;
use protocol::{NetworkItemStack, StackRequestAction};
use protocol_ids::ContainerName;

use super::cells::{Cells, Held};
use super::equipment::equipment_admission;
use super::gesture::Submission;
use super::overlay::DeltaGroup;
use super::{Cell, InventoryGestureError, PlayerInventoryLedger};

/// Reports an equipment refusal separately from other invalid placements.
pub(super) const fn placement_refused(cell: Cell) -> InventoryGestureError {
    if matches!(cell, Cell::Inventory(_) | Cell::Armor(_) | Cell::Offhand) {
        InventoryGestureError::EquipmentRefused
    } else {
        InventoryGestureError::InvalidRequest
    }
}

impl PlayerInventoryLedger {
    /// Returns a slot-specific limit, zero for refusal, or no extra item limit.
    pub(super) fn placement_capacity(&self, cell: Cell, stack: &NetworkItemStack) -> Option<u16> {
        self.placement_capacity_with_facts(&self.item_facts, cell, stack)
    }

    /// Evaluates the same destination contract against a candidate registry admission.
    fn placement_capacity_with_facts(
        &self,
        facts: &crate::SessionItemFacts,
        cell: Cell,
        stack: &NetworkItemStack,
    ) -> Option<u16> {
        let roles = facts.get(stack.network_id).roles;
        match cell {
            Cell::Armor(slot) => Some(u16::from(
                equipment_admission(facts, stack.network_id).armor == Some(slot),
            )),
            Cell::Offhand => {
                (equipment_admission(facts, stack.network_id).offhand != Some(true)).then_some(0)
            }
            Cell::Storage(slot) => match self
                .window_kind()
                .and_then(|kind| protocol::open_cell_request(kind, slot, None, None))
            {
                Some((protocol::StackRequestContainer::OpenWindow { name, .. }, _))
                    if name == ContainerName::BrewingStandResult.wire_value() =>
                {
                    Some(1)
                }
                _ => None,
            },
            Cell::Craft(slot) => {
                match protocol::ui_slot_container_name(slot).and_then(ContainerName::from_wire) {
                    Some(ContainerName::LoomInput)
                        if !facts.is_banner(stack.network_id)
                            || protocol::item_banner_type(&stack.extra_data)
                                .is_some_and(|kind| kind != 0) =>
                    {
                        Some(0)
                    }
                    Some(ContainerName::GrindstoneInput | ContainerName::GrindstoneAdditional)
                        if !protocol::item_has_enchantment_list(&stack.extra_data)
                            && (facts.get(stack.network_id).durability.damageable()
                                != Some(true)
                                || protocol::item_extra_unbreakable(&stack.extra_data)) =>
                    {
                        Some(0)
                    }
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

    /// Detects changed destination permissions without restricting removal-only gestures.
    pub(super) fn placement_admission_changes(
        &self,
        request: &super::PendingRequest,
        facts: &crate::SessionItemFacts,
    ) -> bool {
        request.placements.iter().any(|placement| {
            self.placement_capacity(placement.destination, &placement.incoming)
                != self.placement_capacity_with_facts(
                    facts,
                    placement.destination,
                    &placement.incoming,
                )
        })
    }

    /// Checks permissions against each atomic step before any request is retained.
    pub(super) fn validate_submission(
        &self,
        submission: &Submission,
    ) -> Result<(), InventoryGestureError> {
        let mut cells = self.view().clone();
        for group in &submission.groups {
            self.validate_group(&cells, group, &submission.actions)?;
            if !group.apply(&mut cells) {
                return Err(InventoryGestureError::InvalidRequest);
            }
        }
        Ok(())
    }

    /// Rechecks one operation's permissions and item compatibility against its actual inputs.
    pub(super) fn validate_group(
        &self,
        cells: &Cells,
        group: &DeltaGroup,
        actions: &[StackRequestAction],
    ) -> Result<(), InventoryGestureError> {
        match group {
            DeltaGroup::Transfer {
                source,
                destination,
                amount,
                ..
            } => {
                self.validate_removal(*source, cells.get(*source))?;
                if let Some(from) = cells.get(*source) {
                    if let Some(into) = cells.get(*destination)
                        && !matches!(
                            self.merge_item_relation(from, into),
                            super::registry::OccupiedStackRelation::Compatible { .. }
                        )
                    {
                        return Err(InventoryGestureError::InvalidRequest);
                    }
                    self.validate_placement(cells, *destination, from, *amount)?;
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
                self.validate_removal(*source, cells.get(*source))?;
                if cells.get(*source).is_some_and(|held| {
                    crate::item_permissions::special_removal_locked(&held.stack.extra_data)
                }) && actions.iter().any(|action| {
                    matches!(action,
                    StackRequestAction::Drop { source: slot, .. }
                    | StackRequestAction::Destroy { source: slot, .. }
                    | StackRequestAction::Consume { source: slot, .. }
                        if self.request_cell(*slot) == Some(*source))
                }) {
                    return Err(InventoryGestureError::SlotLocked);
                }
            }
            DeltaGroup::Set { .. } => {}
        }
        Ok(())
    }

    /// Refuses ordinary removal from a locked player or equipment slot.
    fn validate_removal(
        &self,
        cell: Cell,
        held: Option<&Held>,
    ) -> Result<(), InventoryGestureError> {
        if matches!(cell, Cell::Inventory(_) | Cell::Armor(_) | Cell::Offhand)
            && held.is_some_and(|held| protocol::item_extra_slot_locked(&held.stack.extra_data))
        {
            return Err(InventoryGestureError::SlotLocked);
        }
        if matches!(cell, Cell::Armor(_))
            && !self.permissions.creative
            && held.is_some_and(|held| {
                protocol::item_enchantment_level_value(
                    &held.stack.extra_data,
                    i16::from(protocol_ids::Enchantment::Curseofbinding.wire_value()),
                )
                .is_some_and(|level| level > 0)
            })
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
        if matches!(cell, Cell::Storage(_) | Cell::Craft(_))
            && protocol::item_extra_lock(&held.stack.extra_data).is_some_and(|lock| lock != 0)
        {
            return Err(InventoryGestureError::SlotLocked);
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
