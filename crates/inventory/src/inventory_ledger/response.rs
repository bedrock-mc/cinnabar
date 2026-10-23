//! Response correlation and the per-cell overlay of server-stated names and
//! durability; unstated fields stay absent rather than defaulted.

use sha2::{Digest, Sha256};
use std::sync::Arc;

use protocol::{ItemStackResponseEvent, StackResponseSlot, StackResponseStatus};

use super::cells::Held;
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

    /// Whether the target retains this item through moves, predictions or accepted corrections.
    /// Fresh slot and content writes reset its source identity; replacements carry their own.
    #[must_use]
    pub fn target_retains_stack_id(
        &self,
        target: super::InventoryTarget,
        previous_stack_id: i32,
    ) -> bool {
        let cell = target.cell();
        let Some(held) = self.view().get(cell) else {
            return false;
        };
        let mut current_id = held.stack.stack_network_id;
        if held.source_stack_id == Some(previous_stack_id)
            || current_id == previous_stack_id
            || held.acknowledged_request_id == Some(previous_stack_id)
        {
            return true;
        }
        for request in self.queue.iter().rev() {
            if request.request_id != current_id {
                continue;
            }
            let Some(prediction) = request
                .predicted
                .iter()
                .find(|prediction| prediction.cell == cell)
            else {
                return false;
            };
            let Some(before) = &prediction.before else {
                return false;
            };
            let mut touched = false;
            for group in &request.groups {
                if group.touched().any(|candidate| candidate == cell) {
                    touched = true;
                    if !group.retains_item_at(cell) {
                        return false;
                    }
                }
            }
            if !touched {
                return false;
            }
            current_id = before.stack.stack_network_id;
            if before.source_stack_id == Some(previous_stack_id)
                || current_id == previous_stack_id
                || before.acknowledged_request_id == Some(previous_stack_id)
            {
                return true;
            }
        }
        false
    }

    /// The overlay travelling with the cursor's presented stack during prediction.
    #[must_use]
    pub fn presented_cursor_overlay(&self) -> Option<&StackResponseOverlay> {
        self.view().get(Cell::Cursor)?.overlay.as_ref()
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
    /// repeated ids cannot mutate the backing inventory. Every result other than
    /// success is an error that rolls the request back, whatever its error code.
    pub(super) fn apply_response(&mut self, event: &ItemStackResponseEvent) {
        for response in event.responses.iter() {
            let accepted = response.status == StackResponseStatus::Accepted;
            tracing::debug!(target: "cinnabar::inventory_requests",
                request_id = response.request_id, status = ?response.status,
                pending = self.queue.len(), "inventory response received");
            for container in response.containers.iter() {
                for slot in container.slots.iter() {
                    tracing::debug!(target: "cinnabar::inventory_requests",
                        request_id = response.request_id, container = ?container.container,
                        slot = slot.slot, requested_slot = slot.hotbar_slot,
                        stack_id = slot.item_stack_id, count = slot.count,
                        "inventory response slot");
                }
            }
            let Some(index) = self.queue.iter().position(|pending| {
                if pending.locally_committed() {
                    pending.legacy_request_id() == Some(response.request_id)
                } else {
                    pending.request_id == response.request_id
                }
            }) else {
                if !accepted {
                    self.settle_abandoned_rejection(response.request_id);
                }
                continue;
            };
            let request = self.queue.remove(index).expect("index observed");
            let current = self.request_is_current(&request);
            if accepted {
                self.accept_trade_purchase(&request);
                self.settle(&request, &response.containers);
            } else if current {
                self.reject_request(&request);
            }
            self.finish_request_retirement(request.request_id, current && accepted);
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
        overlay.filtered_custom_name = Some(Arc::clone(&correction.filtered_custom_name));
    }
    let damage = i32::from(correction.durability_correction as i16);
    overlay.durability_correction = (damage >= 0).then_some(damage);
}

/// Applies response identity fields to retained NBT before future gestures compare it.
/// Normalizes the response damage while retaining the stored integer's full width.
/// Malformed or oversized edits leave the retained item and its overlay unchanged.
pub(super) fn apply_response_item_data(
    held: &mut Held,
    correction: &StackResponseSlot,
) -> Option<()> {
    let damage = i32::from(correction.durability_correction as i16);
    let previous = protocol::item_extra_damage_integer(&held.stack.extra_data).unwrap_or(0);
    let rename = !correction.custom_name.is_empty()
        && (protocol::item_custom_name(&held.stack.extra_data).as_deref()
            != Some(correction.custom_name.as_ref())
            || protocol::item_filtered_custom_name(&held.stack.extra_data).as_deref()
                != (!correction.filtered_custom_name.is_empty())
                    .then_some(correction.filtered_custom_name.as_ref()));
    if previous != damage || rename {
        let extra_data = protocol::edit_item_extra(
            &held.stack.extra_data,
            &protocol::ItemNbtEdit {
                damage: (previous != damage).then_some(damage),
                custom_name: rename.then(|| Some(correction.custom_name.to_string())),
                filtered_custom_name: rename.then(|| {
                    (!correction.filtered_custom_name.is_empty())
                        .then(|| correction.filtered_custom_name.to_string())
                }),
                ..Default::default()
            },
        )?;
        if extra_data.len() > protocol::MAX_ITEM_EXTRA_BYTES {
            tracing::warn!(target: "cinnabar::inventory_requests",
                bytes = extra_data.len(), max = protocol::MAX_ITEM_EXTRA_BYTES,
                "skipping oversized response item data");
            return None;
        }
        held.stack.nbt_digest = Sha256::digest(&extra_data).into();
        held.stack.extra_data = extra_data;
    }
    merge_response_overlay(&mut held.overlay, correction);
    Some(())
}
