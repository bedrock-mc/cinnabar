//! Cursor-free moves: number-key hotbar swaps and drops.
//!
//! Hotbar swaps follow the owner's vanilla-mirroring rule (Swap between two
//! occupied cells, otherwise one Place) and drops use one Drop action.

use protocol::StackRequestAction;

use super::cells::{Cell, Held};
use super::gesture::{
    Built, InventoryTarget, StackRequestActionKind, Submission, counted_transfer, swap,
};
use super::helpers::{WindowAddress, request_slot};
use super::overlay::DeltaGroup;
use super::{InventoryGestureError, PlayerInventoryLedger};

/// What a drop takes from.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DropSource {
    Target(InventoryTarget),
    Cursor,
}

impl PlayerInventoryLedger {
    /// Deletes the held stack, as clicking the creative catalog does.
    pub fn begin_destroy_cursor(&mut self) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor])?;
        let held = self
            .named(self.view().get(Cell::Cursor).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let amount = u8::try_from(held.stack.count)
            .ok()
            .filter(|amount| *amount != 0)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let id = held.stack.stack_network_id;
        let built = Built {
            action: StackRequestAction::Destroy {
                amount,
                source: request_slot(Cell::Cursor, id, None)?,
            },
            group: DeltaGroup::Shrink {
                source: Cell::Cursor,
                amount: held.stack.count,
                source_id: id,
            },
            requires_distinct_stack_ids: false,
            registry_bound_merge: false,
        };
        self.submit_built(built, personal_generation)
    }

    /// Swaps a hovered cell with hotbar cell `hotbar`, or places into
    /// whichever of the two is empty.
    pub fn begin_hotbar_swap(
        &mut self,
        target: InventoryTarget,
        hotbar: u8,
    ) -> Result<i32, InventoryGestureError> {
        let (source, destination) = (target.cell(), Cell::Inventory(hotbar));
        if hotbar >= 9 || source == destination {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let personal_generation = self.gesture_preflight(!matches!(source, Cell::Storage(_)))?;
        self.check_surfaces([source, destination])?;
        let from = self.movable(source)?;
        let to = self.movable(destination)?;
        let identity = self.window_address();
        let built = match (from, to) {
            (Some(from), Some(to)) => swap(source, destination, &from.stack, &to.stack, identity)?,
            (Some(from), None) => place(source, destination, &from, identity)?,
            (None, Some(to)) => place(destination, source, &to, identity)?,
            (None, None) => return Err(InventoryGestureError::EmptyGesture),
        };
        self.submit_built(built, personal_generation)
    }

    /// Drops `amount` (or the whole stack) from a cell or the cursor.
    pub fn begin_drop(
        &mut self,
        source: DropSource,
        amount: Option<u16>,
    ) -> Result<i32, InventoryGestureError> {
        self.drop_from(source, amount, true)
    }

    /// Drops from hotbar cell `slot` with no window open, like the in-world drop key.
    pub fn begin_world_drop(
        &mut self,
        slot: u8,
        amount: Option<u16>,
    ) -> Result<i32, InventoryGestureError> {
        let request = self.drop_from(
            DropSource::Target(InventoryTarget::Player(slot)),
            amount,
            false,
        )?;
        self.pending_world_drops = self
            .pending_world_drops
            .saturating_add(1)
            .min(super::MAX_PENDING_REQUESTS);
        Ok(request)
    }

    /// Takes admitted in-world drop gestures once, independently of transport or server replies.
    pub fn take_world_drops(&mut self) -> usize {
        std::mem::take(&mut self.pending_world_drops)
    }

    fn drop_from(
        &mut self,
        source: DropSource,
        amount: Option<u16>,
        needs_window: bool,
    ) -> Result<i32, InventoryGestureError> {
        let cell = match source {
            DropSource::Target(target) => target.cell(),
            DropSource::Cursor => Cell::Cursor,
        };
        let personal_generation =
            self.gesture_preflight(needs_window && !matches!(cell, Cell::Storage(_)))?;
        self.check_surfaces([cell])?;
        let held = match cell {
            Cell::Cursor => self.named(self.view().get(Cell::Cursor).cloned())?,
            cell => self.movable(cell)?,
        }
        .ok_or(InventoryGestureError::EmptyGesture)?;
        let amount = amount.unwrap_or(held.stack.count);
        let wire = u8::try_from(amount)
            .ok()
            .filter(|amount| *amount != 0 && u16::from(*amount) <= held.stack.count)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let id = held.stack.stack_network_id;
        let built = Built {
            action: StackRequestAction::Drop {
                amount: wire,
                source: request_slot(cell, id, self.window_address())?,
                randomly: false,
            },
            group: DeltaGroup::Shrink {
                source: cell,
                amount,
                source_id: id,
            },
            requires_distinct_stack_ids: false,
            registry_bound_merge: false,
        };
        self.submit_built(built, personal_generation)
    }

    /// The current stack in a validated gesture cell, refusing one that
    /// cannot be named in a request yet.
    pub(super) fn movable(&self, cell: Cell) -> Result<Option<Held>, InventoryGestureError> {
        self.check_target(cell)?;
        self.named(self.view().get(cell).cloned())
    }

    pub(super) fn named(&self, held: Option<Held>) -> Result<Option<Held>, InventoryGestureError> {
        match held {
            Some(held) if self.awaiting_identity(&held) => {
                Err(InventoryGestureError::AwaitingIdentity)
            }
            held => Ok(held),
        }
    }

    pub(super) fn submit_built(
        &mut self,
        built: Built,
        personal_generation: Option<u64>,
    ) -> Result<i32, InventoryGestureError> {
        self.submit(Submission {
            actions: vec![built.action],
            groups: vec![built.group],
            personal_generation,
            requires_distinct_stack_ids: built.requires_distinct_stack_ids,
            registry_bound_merge: built.registry_bound_merge,
        })
    }
}

/// One Place of a whole stack into an empty cell.
pub(super) fn place(
    source: Cell,
    destination: Cell,
    held: &Held,
    identity: Option<WindowAddress>,
) -> Result<Built, InventoryGestureError> {
    counted_transfer(
        StackRequestActionKind::Place,
        source,
        destination,
        &held.stack,
        identity,
        None,
    )
}
