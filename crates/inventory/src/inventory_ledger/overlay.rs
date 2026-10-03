//! Atomic operations used only to create a request's absolute sparse snapshot.
//! Backing slot updates and responses never rerun these operations.

use super::cells::{Cell, Cells, Held};

/// One atomic prediction step. A group either applies whole or is skipped.
///
/// Every group names the exact stack network ids its request carried, so a
/// cell the server rewrote with a different stack no longer admits it: the
/// server validates requests by those ids and would refuse the same step.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum DeltaGroup {
    /// Moves `amount` from `source` into an empty or compatible destination.
    Transfer {
        source: Cell,
        destination: Cell,
        amount: u16,
        source_id: i32,
        /// `None` requires an empty destination.
        destination_id: Option<i32>,
        /// Merge capacity for an occupied destination.
        capacity: Option<u16>,
    },
    Swap {
        source: Cell,
        destination: Cell,
        source_id: i32,
        destination_id: i32,
    },
    /// Removes `amount` from `source` (consume, destroy, drop).
    Shrink {
        source: Cell,
        amount: u16,
        source_id: i32,
    },
    /// Writes a created stack (crafting and creative output).
    Set { cell: Cell, held: Held },
}

impl DeltaGroup {
    pub(super) fn touched(&self) -> impl Iterator<Item = Cell> {
        let (first, second) = match self {
            Self::Transfer {
                source,
                destination,
                ..
            }
            | Self::Swap {
                source,
                destination,
                ..
            } => (*source, Some(*destination)),
            Self::Shrink { source, .. } => (*source, None),
            Self::Set { cell, .. } => (*cell, None),
        };
        std::iter::once(first).chain(second)
    }

    /// Applies this group to `cells`, reporting whether it held.
    pub(super) fn apply(&self, cells: &mut Cells) -> bool {
        match self {
            Self::Transfer {
                source,
                destination,
                amount,
                source_id,
                destination_id,
                capacity,
            } => transfer(
                cells,
                *source,
                *destination,
                *amount,
                *source_id,
                *destination_id,
                *capacity,
            ),
            Self::Swap {
                source,
                destination,
                source_id,
                destination_id,
            } => {
                if source == destination
                    || !holds(cells, *source, *source_id)
                    || !holds(cells, *destination, *destination_id)
                {
                    return false;
                }
                let left = cells.take(*source);
                let right = cells.take(*destination);
                cells.set(*source, right);
                cells.set(*destination, left);
                true
            }
            Self::Shrink {
                source,
                amount,
                source_id,
            } => {
                if *amount == 0 || !holds(cells, *source, *source_id) {
                    return false;
                }
                let held = cells.get_mut(*source).expect("held above");
                if held.stack.count < *amount {
                    return false;
                }
                held.stack.count -= amount;
                if held.stack.count == 0 {
                    cells.set(*source, None);
                }
                true
            }
            Self::Set { cell, held } => cells.set(*cell, Some(held.clone())),
        }
    }
}

fn holds(cells: &Cells, cell: Cell, stack_network_id: i32) -> bool {
    cells
        .get(cell)
        .is_some_and(|held| held.stack.stack_network_id == stack_network_id)
}

fn transfer(
    cells: &mut Cells,
    source: Cell,
    destination: Cell,
    amount: u16,
    source_id: i32,
    destination_id: Option<i32>,
    capacity: Option<u16>,
) -> bool {
    if amount == 0 || source == destination || !cells.contains(destination) {
        return false;
    }
    let Some(from) = cells
        .get(source)
        .filter(|held| held.stack.stack_network_id == source_id && held.stack.count >= amount)
    else {
        return false;
    };
    let partial = from.stack.count > amount;
    let moved = match (destination_id, cells.get(destination)) {
        // A split keeps the source overlay on both halves until the server
        // restates either one.
        (None, None) => {
            let mut moved = from.clone();
            moved.stack.count = amount;
            moved
        }
        (Some(id), Some(into)) if into.stack.stack_network_id == id => {
            let Some(count) = into.stack.count.checked_add(amount) else {
                return false;
            };
            // Both ids still match, so these are the stacks the gesture
            // judged compatible.
            if capacity.is_none_or(|cap| count > cap) {
                return false;
            }
            let mut merged = into.clone();
            merged.stack.count = count;
            merged
        }
        _ => return false,
    };
    let residual = partial.then(|| {
        let mut residual = from.clone();
        residual.stack.count -= amount;
        residual
    });
    cells.set(source, residual);
    cells.set(destination, Some(moved));
    true
}
