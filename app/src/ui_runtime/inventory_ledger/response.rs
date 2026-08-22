//! Authoritative item-stack responses plus the retained per-cell response
//! overlay.
//!
//! An accepted correction is authoritative for its whole cell: beyond the
//! count/stack-network-id corrections applied directly to the retained
//! stack, the server's custom display names and durability damage are kept
//! as one [`StackResponseOverlay`] keyed to that cell. The overlay travels
//! with its predicted stack through a pending gesture, and each correction
//! restates only what changed: an omitted (empty or nonpositive) field
//! retains the previously accepted value until the server affirms a new one
//! or another authoritative path replaces the cell, so a retained name or
//! damage value can never silently outlive or misdescribe its stack.
//! Rejected requests roll back without writing one.

use std::sync::Arc;

use protocol::{ItemStackResponseEvent, StackResponseSlot, StackResponseStatus};

use super::{Cell, GENERIC_STORAGE_SLOT_TYPE, PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};

/// Authoritative presentation facts an accepted server correction attached
/// to one inventory cell: custom display names plus the exact durability
/// damage. The overlay never alters stack identity; replacing the cell
/// through any other authoritative path drops it.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct StackResponseOverlay {
    /// Server-owned display name for this stack (empty when none was sent).
    pub custom_name: Arc<str>,
    /// Redacted half of the same redactable wire string pair.
    pub filtered_custom_name: Arc<str>,
    /// Authoritative damage for the presented durability bar.
    pub durability_correction: i32,
}

impl PlayerInventoryLedger {
    /// The authoritative response overlay retained for one player-inventory
    /// slot, or `None` when no accepted correction currently describes it.
    #[must_use]
    pub fn slot_overlay(&self, slot: u8) -> Option<&StackResponseOverlay> {
        self.slot_overlays.get(usize::from(slot))?.as_ref()
    }

    /// The authoritative response overlay retained for the cursor cell.
    #[must_use]
    pub fn cursor_overlay(&self) -> Option<&StackResponseOverlay> {
        self.cursor_overlay.as_ref()
    }

    /// The authoritative response overlay retained for one open generic
    /// storage slot.
    #[must_use]
    pub fn storage_slot_overlay(&self, slot: u8) -> Option<&StackResponseOverlay> {
        let storage = self.storage.as_ref()?;
        storage.overlays.get(usize::from(slot))?.as_ref()
    }

    pub(super) fn apply_response(&mut self, event: &ItemStackResponseEvent) {
        let Some(request_id) = self.pending_request_id() else {
            return;
        };
        let Some(response) = event
            .responses
            .iter()
            .find(|response| response.request_id == request_id)
        else {
            return;
        };
        let pending = self.pending.as_ref().expect("pending request exists");
        if pending.session_generation != self.session_generation
            || pending.storage_generation.is_some_and(|generation| {
                self.storage.as_ref().map(|storage| storage.generation) != Some(generation)
            })
            || pending.storage_identity.is_some_and(|identity| {
                self.storage.as_ref().and_then(|storage| storage.identity) != Some(identity)
            })
        {
            self.pending = None;
            return;
        }
        if response.status != StackResponseStatus::Accepted {
            self.rollback_pending();
            return;
        }
        if response.containers.iter().any(|container| {
            container.container.slot_type == Some(GENERIC_STORAGE_SLOT_TYPE)
                && self.pending_identity_mismatch(container.container)
        }) {
            self.require_authoritative_recovery();
            return;
        }
        let prediction = &self
            .pending
            .as_ref()
            .expect("the matching pending request was observed")
            .prediction;
        if self.cell_revision(prediction.source) != prediction.source_revision
            || self.cell_revision(prediction.destination) != prediction.destination_revision
        {
            self.require_authoritative_recovery();
            return;
        }
        let prediction = self
            .pending
            .take()
            .expect("the matching pending request was observed")
            .prediction;
        self.set_cell(prediction.source, prediction.source_stack);
        self.set_cell(prediction.destination, prediction.destination_stack);
        // The predicted halves carry each travelling overlay so a moved stack
        // keeps its retained identity across the gesture it participated in.
        self.replace_cell_overlay(prediction.source, prediction.source_overlay);
        self.replace_cell_overlay(prediction.destination, prediction.destination_overlay);
        self.bump_cell_revision(prediction.source);
        self.bump_cell_revision(prediction.destination);
        for container in response.containers.iter() {
            for correction in container.slots.iter() {
                let cell = match container.container.slot_type {
                    Some(12) if usize::from(correction.slot) < PLAYER_INVENTORY_SLOT_COUNT => {
                        Cell::Inventory(correction.slot)
                    }
                    Some(59) if correction.slot == 0 => Cell::Cursor,
                    Some(GENERIC_STORAGE_SLOT_TYPE)
                        if self.storage.as_ref().is_some_and(|storage| {
                            usize::from(correction.slot) < storage.slots.len()
                        }) =>
                    {
                        Cell::Storage(correction.slot)
                    }
                    _ => continue,
                };
                if correction.count == 0 {
                    self.set_cell(cell, None);
                } else if self.correct_stack_count(cell, correction.count, correction.item_stack_id)
                {
                    self.merge_cell_overlay(cell, correction);
                } else {
                    self.mark_cell_recovery(cell);
                }
                self.bump_cell_revision(cell);
            }
        }
    }

    /// The committed overlay for one cell, ignoring any in-flight prediction.
    pub(super) fn cell_overlay(&self, cell: Cell) -> Option<&StackResponseOverlay> {
        match cell {
            Cell::Inventory(slot) => self.slot_overlays.get(usize::from(slot))?.as_ref(),
            Cell::Storage(slot) => {
                let storage = self.storage.as_ref()?;
                storage.overlays.get(usize::from(slot))?.as_ref()
            }
            Cell::Cursor => self.cursor_overlay.as_ref(),
        }
    }

    /// Writes one cell's overlay outright, including clearing it with `None`.
    pub(super) fn replace_cell_overlay(
        &mut self,
        cell: Cell,
        overlay: Option<StackResponseOverlay>,
    ) {
        match cell {
            Cell::Inventory(slot) => {
                if let Some(entry) = self.slot_overlays.get_mut(usize::from(slot)) {
                    *entry = overlay;
                }
            }
            Cell::Storage(slot) => {
                if let Some(storage) = self.storage.as_mut()
                    && let Some(entry) = storage.overlays.get_mut(usize::from(slot))
                {
                    *entry = overlay;
                }
            }
            Cell::Cursor => self.cursor_overlay = overlay,
        }
    }

    pub(super) fn clear_cell_overlay(&mut self, cell: Cell) {
        self.replace_cell_overlay(cell, None);
    }

    /// Restates one accepted correction onto the cell's retained overlay.
    ///
    /// Empty name halves and nonpositive durability values are read as
    /// unstated rather than as erasures, so a lazy correction cannot strip a
    /// previously accepted name or damage value from a stack that is still
    /// present.
    fn merge_cell_overlay(&mut self, cell: Cell, correction: &StackResponseSlot) {
        match cell {
            Cell::Inventory(slot) => {
                if let Some(entry) = self.slot_overlays.get_mut(usize::from(slot)) {
                    merge_response_overlay(entry, correction);
                }
            }
            Cell::Storage(slot) => {
                if let Some(storage) = self.storage.as_mut()
                    && let Some(entry) = storage.overlays.get_mut(usize::from(slot))
                {
                    merge_response_overlay(entry, correction);
                }
            }
            Cell::Cursor => merge_response_overlay(&mut self.cursor_overlay, correction),
        }
    }

    /// Applies the count/stack-network-id half of one accepted correction,
    /// returning whether a retained stack existed to correct.
    fn correct_stack_count(&mut self, cell: Cell, count: u8, item_stack_id: i32) -> bool {
        let Some(stack) = self.cell_mut(cell) else {
            return false;
        };
        stack.count = u16::from(count);
        if item_stack_id > 0 {
            stack.stack_network_id = item_stack_id;
        }
        true
    }
}

/// Merges one accepted correction into a cell's retained overlay, creating
/// the overlay when this is the cell's first corrected response.
fn merge_response_overlay(
    entry: &mut Option<StackResponseOverlay>,
    correction: &StackResponseSlot,
) {
    let overlay = entry.get_or_insert_with(Default::default);
    if !correction.custom_name.is_empty() && overlay.custom_name != correction.custom_name {
        overlay.custom_name = Arc::clone(&correction.custom_name);
    }
    if !correction.filtered_custom_name.is_empty()
        && overlay.filtered_custom_name != correction.filtered_custom_name
    {
        overlay.filtered_custom_name = Arc::clone(&correction.filtered_custom_name);
    }
    if correction.durability_correction > 0
        && overlay.durability_correction != correction.durability_correction
    {
        overlay.durability_correction = correction.durability_correction;
    }
}
