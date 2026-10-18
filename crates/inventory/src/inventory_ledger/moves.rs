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
use super::queue::WorldDrop;
use super::{InventoryGestureError, PlayerInventoryLedger};

/// What a drop takes from.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DropSource {
    Target(InventoryTarget),
    Cursor,
}

impl PlayerInventoryLedger {
    /// Deletes the requested cursor amount, or the whole selection when unspecified.
    pub fn begin_destroy_cursor(
        &mut self,
        requested: Option<u8>,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor])?;
        let held = self
            .named(self.view().get(Cell::Cursor).cloned())?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let amount = u8::try_from(held.stack.count)
            .ok()
            .filter(|amount| *amount != 0)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let amount = requested.unwrap_or(amount);
        if amount == 0 || u16::from(amount) > held.stack.count {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let id = held.stack.stack_network_id;
        let built = Built {
            action: StackRequestAction::Destroy {
                amount,
                source: request_slot(Cell::Cursor, id, None)?,
            },
            group: DeltaGroup::Shrink {
                source: Cell::Cursor,
                amount: u16::from(amount),
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
        self.hotbar_swap(target, hotbar, true)
    }

    /// [`Self::begin_hotbar_swap`] between the player's own cells with no window open, as a
    /// client part's modal over gameplay shows them; like the in-world drop, the request names
    /// the player's own containers and needs no open inventory.
    pub fn begin_windowless_hotbar_swap(
        &mut self,
        target: InventoryTarget,
        hotbar: u8,
    ) -> Result<i32, InventoryGestureError> {
        if !matches!(target, InventoryTarget::Player(_)) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.hotbar_swap(target, hotbar, false)
    }

    fn hotbar_swap(
        &mut self,
        target: InventoryTarget,
        hotbar: u8,
        needs_window: bool,
    ) -> Result<i32, InventoryGestureError> {
        let (source, destination) = (target.cell(), Cell::Inventory(hotbar));
        if hotbar >= 9 || source == destination {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let personal_generation = self
            .gesture_preflight(needs_window && !matches!(source, Cell::Storage(_)))
            .or_else(|error| {
                // A first-frame hotbar swap queues behind its unsent Open, never ahead of it.
                if error == InventoryGestureError::PersonalInventoryUnavailable
                    && let Some(super::control::ControlRequest::Open { generation, .. }) =
                        self.pending_control()
                {
                    return Ok(Some(generation));
                }
                Err(error)
            })?;
        self.check_surfaces([source, destination])?;
        let from = self.movable(source)?;
        let to = self.movable(destination)?;
        let identity = self.window_address();
        // The hotbar stack lands in the hovered cell, which may be equipment.
        let equipment_limit = to
            .as_ref()
            .and_then(|to| self.placement_capacity(source, &to.stack));
        if equipment_limit == Some(0) {
            return Err(super::validation::placement_refused(source));
        }
        let built = match (from, to) {
            (Some(_), Some(to)) if equipment_limit.is_some_and(|limit| to.stack.count > limit) => {
                return Err(super::validation::placement_refused(source));
            }
            (Some(from), Some(to)) => swap(source, destination, &from.stack, &to.stack, identity)?,
            (Some(from), None) => place(source, destination, &from, identity)?,
            (None, Some(to)) => counted_transfer(
                StackRequestActionKind::Place,
                destination,
                source,
                &to.stack,
                identity,
                equipment_limit.filter(|limit| *limit < to.stack.count),
            )?,
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
        self.drop_from(source, amount, None)
    }

    /// Drops from hotbar cell `slot` with no window open, like the in-world drop key.
    pub fn begin_world_drop(
        &mut self,
        slot: u8,
        amount: Option<u16>,
    ) -> Result<i32, InventoryGestureError> {
        let legacy_request_id =
            protocol::next_legacy_request_id().ok_or(InventoryGestureError::InvalidRequest)?;
        let request = self.drop_from(
            DropSource::Target(InventoryTarget::Player(slot)),
            amount,
            Some(WorldDrop { legacy_request_id }),
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

    /// Builds a drop against the presentation or the consumption-adjusted transaction source.
    fn drop_from(
        &mut self,
        source: DropSource,
        amount: Option<u16>,
        world_drop: Option<WorldDrop>,
    ) -> Result<i32, InventoryGestureError> {
        let cell = match source {
            DropSource::Target(target) => target.cell(),
            DropSource::Cursor => Cell::Cursor,
        };
        let personal_generation =
            self.gesture_preflight(world_drop.is_none() && !matches!(cell, Cell::Storage(_)))?;
        self.check_surfaces([cell])?;
        let held = match cell {
            Cell::Cursor => self.named(self.view().get(Cell::Cursor).cloned())?,
            cell if world_drop.is_some() => {
                self.check_target(cell)?;
                self.named(self.transaction_held(cell))?
            }
            cell => self.movable(cell)?,
        }
        .ok_or(InventoryGestureError::EmptyGesture)?;
        let amount = amount.unwrap_or(held.stack.count);
        let wire = u8::try_from(amount)
            .ok()
            .filter(|amount| *amount != 0 && u16::from(*amount) <= held.stack.count)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let id = held.stack.stack_network_id;
        let submission = Submission {
            actions: vec![StackRequestAction::Drop {
                amount: wire,
                source: request_slot(cell, id, self.window_address())?,
                randomly: false,
            }],
            groups: vec![DeltaGroup::Shrink {
                source: cell,
                amount,
                source_id: id,
            }],
            personal_generation,
            requires_distinct_stack_ids: false,
            registry_bound_merge: false,
        };
        self.submit_request(submission, world_drop)
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
