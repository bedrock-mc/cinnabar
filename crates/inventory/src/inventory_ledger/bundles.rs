//! Bundle contents: dynamic containers keyed by the bundle's id, kept for
//! tooltips and for the insert and extract requests.

use protocol::{
    CONTAINER_NAME_DYNAMIC, NetworkItemStack, StackRequestAction, StackRequestContainer,
    StackRequestSlot, item_bundle_id,
};

use super::cells::{Cell, Held};
use super::gesture::{InventoryTarget, Submission};
use super::helpers::request_slot;
use super::overlay::DeltaGroup;
use super::{InventoryGestureError, PlayerInventoryLedger};

/// Bundles whose contents are retained; a hostile server cannot grow the map.
const MAX_BUNDLES: usize = 64;
/// Contents slots retained per bundle.
const MAX_BUNDLE_SLOTS: usize = 64;

fn dynamic_slot(dynamic_id: u32, slot: u8, stack_network_id: i32) -> StackRequestSlot {
    StackRequestSlot {
        container: StackRequestContainer::OpenWindow {
            name: CONTAINER_NAME_DYNAMIC,
            dynamic_id: Some(dynamic_id),
        },
        slot,
        stack_network_id,
    }
}

impl PlayerInventoryLedger {
    /// Replaces one bundle's contents from a dynamic-container content event.
    pub(super) fn apply_bundle_content(&mut self, dynamic_id: u32, slots: &[NetworkItemStack]) {
        if !self.bundles.contains_key(&dynamic_id) && self.bundles.len() >= MAX_BUNDLES {
            self.note_unrouted_container();
            return;
        }
        let mut contents: Vec<NetworkItemStack> =
            slots.iter().take(MAX_BUNDLE_SLOTS).cloned().collect();
        while contents.last().is_some_and(NetworkItemStack::is_empty) {
            contents.pop();
        }
        self.bundles.insert(dynamic_id, contents);
    }

    /// Updates one slot of a bundle's contents.
    pub(super) fn apply_bundle_slot(
        &mut self,
        dynamic_id: u32,
        slot: u16,
        stack: &NetworkItemStack,
    ) {
        let slot = usize::from(slot);
        if slot >= MAX_BUNDLE_SLOTS
            || (!self.bundles.contains_key(&dynamic_id) && self.bundles.len() >= MAX_BUNDLES)
        {
            self.note_unrouted_container();
            return;
        }
        let contents = self.bundles.entry(dynamic_id).or_default();
        if contents.len() <= slot {
            contents.resize(slot + 1, NetworkItemStack::empty());
        }
        contents[slot] = stack.clone();
        while contents.last().is_some_and(NetworkItemStack::is_empty) {
            contents.pop();
        }
    }

    /// The items a bundle holds, by its dynamic container id.
    #[must_use]
    pub fn bundle_contents(&self, dynamic_id: u32) -> Option<&[NetworkItemStack]> {
        self.bundles.get(&dynamic_id).map(Vec::as_slice)
    }

    /// The dynamic container id of the bundle in `target`, if it holds one.
    #[must_use]
    pub fn bundle_id_at(&self, target: InventoryTarget) -> Option<u32> {
        item_bundle_id(&self.target_stack(target)?.extra_data)
    }

    /// Drops the held stack into the bundle in `bundle`.
    pub fn begin_bundle_insert(
        &mut self,
        bundle: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        let id = self
            .bundle_id_at(bundle)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.check_surfaces([Cell::Cursor, bundle.cell()])?;
        let held = self
            .named(self.view().get(Cell::Cursor).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let amount = u8::try_from(held.stack.count)
            .ok()
            .filter(|amount| *amount != 0)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let next = u8::try_from(self.bundle_contents(id).map_or(0, <[_]>::len))
            .map_err(|_| InventoryGestureError::InvalidRequest)?;
        let stack_id = held.stack.stack_network_id;
        self.submit(Submission {
            actions: vec![StackRequestAction::Place {
                amount,
                source: request_slot(Cell::Cursor, stack_id, None)?,
                destination: dynamic_slot(id, next, 0),
            }],
            groups: vec![DeltaGroup::Shrink {
                source: Cell::Cursor,
                amount: held.stack.count,
                source_id: stack_id,
            }],
            personal_generation,
            requires_distinct_stack_ids: false,
            registry_bound_merge: false,
        })
    }

    /// Takes the newest item out of the bundle in `bundle` into an empty cursor.
    pub fn begin_bundle_extract(
        &mut self,
        bundle: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        let id = self
            .bundle_id_at(bundle)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.check_surfaces([Cell::Cursor, bundle.cell()])?;
        if self.view().get(Cell::Cursor).is_some() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let contents = self
            .bundle_contents(id)
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let index = contents
            .len()
            .checked_sub(1)
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let stack = contents[index].clone();
        if stack.stack_network_id <= 0 {
            return Err(InventoryGestureError::AwaitingIdentity);
        }
        let amount = u8::try_from(stack.count)
            .ok()
            .filter(|amount| *amount != 0)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let slot = u8::try_from(index).map_err(|_| InventoryGestureError::InvalidRequest)?;
        self.submit(Submission {
            actions: vec![StackRequestAction::Take {
                amount,
                source: dynamic_slot(id, slot, stack.stack_network_id),
                destination: request_slot(Cell::Cursor, 0, None)?,
            }],
            groups: vec![DeltaGroup::Set {
                cell: Cell::Cursor,
                held: Held {
                    stack,
                    overlay: None,
                },
            }],
            personal_generation,
            requires_distinct_stack_ids: false,
            registry_bound_merge: false,
        })
    }
}
