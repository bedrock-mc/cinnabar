//! Response correlation and the per-cell overlay of server-stated names and
//! durability; unstated fields stay absent rather than defaulted.

use std::sync::Arc;

use protocol::{ItemStackResponseEvent, StackResponseSlot, StackResponseStatus};

use super::{Cell, PlayerInventoryLedger};

/// Names and durability an accepted correction stated for one cell; `None`
/// while unstated. Any other authoritative write to the cell drops it.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct StackResponseOverlay {
    /// Server-owned display name once a response states it.
    pub custom_name: Option<Arc<str>>,
    /// Redacted half of the same redactable wire string pair once stated.
    pub filtered_custom_name: Option<Arc<str>>,
    /// Authoritative damage for the presented durability bar once a
    /// response states it.
    pub durability_correction: Option<i32>,
}

impl PlayerInventoryLedger {
    /// The authoritative response overlay retained for one player-inventory
    /// slot, or `None` when no accepted correction currently describes it.
    #[must_use]
    pub fn slot_overlay(&self, slot: u8) -> Option<&StackResponseOverlay> {
        self.confirmed.get(Cell::Inventory(slot))?.overlay.as_ref()
    }

    /// The overlay presented for one player-inventory slot: the one travelling
    /// with its predicted stack while a request touches it.
    #[must_use]
    pub fn presented_slot_overlay(&self, slot: u8) -> Option<&StackResponseOverlay> {
        self.view().get(Cell::Inventory(slot))?.overlay.as_ref()
    }

    /// The overlay presented for any gesture target, including armor cells.
    #[must_use]
    pub fn presented_target_overlay(
        &self,
        target: super::InventoryTarget,
    ) -> Option<&StackResponseOverlay> {
        self.view().get(target.cell())?.overlay.as_ref()
    }

    /// The authoritative response overlay retained for the cursor cell.
    #[must_use]
    pub fn cursor_overlay(&self) -> Option<&StackResponseOverlay> {
        self.confirmed.get(Cell::Cursor)?.overlay.as_ref()
    }

    /// The authoritative response overlay retained for one open generic
    /// storage slot.
    #[must_use]
    pub fn storage_slot_overlay(&self, slot: u8) -> Option<&StackResponseOverlay> {
        self.confirmed.get(Cell::Storage(slot))?.overlay.as_ref()
    }

    /// Resolve the request independently of later sparse owners. Unknown or
    /// repeated ids cannot mutate the backing inventory.
    pub(super) fn apply_response(&mut self, event: &ItemStackResponseEvent) {
        for response in event.responses.iter() {
            tracing::debug!(target: "bedrock_client::inventory_requests",
                request_id = response.request_id, status = ?response.status,
                pending = self.queue.len(), "inventory response received");
            for container in response.containers.iter() {
                for slot in container.slots.iter() {
                    tracing::debug!(target: "bedrock_client::inventory_requests",
                        request_id = response.request_id, container = ?container.container,
                        slot = slot.slot, requested_slot = slot.hotbar_slot,
                        stack_id = slot.item_stack_id, count = slot.count,
                        "inventory response slot");
                }
            }
            let Some(index) = self.queue.iter().position(|pending| {
                pending.request_id == response.request_id && pending.accepted.is_none()
            }) else {
                continue;
            };
            if response.status == StackResponseStatus::Accepted
                && self.request_is_current(&self.queue[index])
            {
                self.queue[index].accepted = Some(Arc::clone(&response.containers));
            } else {
                self.queue.remove(index);
            }
            self.settle_accepted_heads();
        }
        self.refold();
        // Settlement may have released the last request of a closing window.
        self.finish_closing();
    }
}

/// Merges one accepted correction into a cell's retained overlay, creating
/// the overlay when this is the cell's first corrected response. Only stated
/// fields are written, so a fresh overlay keeps unstated facts absent.
pub(super) fn merge_response_overlay(
    entry: &mut Option<StackResponseOverlay>,
    correction: &StackResponseSlot,
) {
    let overlay = entry.get_or_insert_with(Default::default);
    if !correction.custom_name.is_empty() {
        overlay.custom_name = Some(Arc::clone(&correction.custom_name));
    }
    if !correction.filtered_custom_name.is_empty() {
        overlay.filtered_custom_name = Some(Arc::clone(&correction.filtered_custom_name));
    }
    if correction.durability_correction >= 0 {
        overlay.durability_correction = Some(correction.durability_correction);
    }
}
