//! Results after a recipe's first, such as a cake's empty buckets: each is created by index
//! and returned to the crafting grid, the inventory, or dropped.

use std::sync::Arc;

use protocol::{
    CraftResult, ItemRegistryEntry, NetworkItemStack, RecipeHandle, RecipeOutput,
    StackRequestAction,
};
use sha2::{Digest, Sha256};

use super::super::cells::{Cell, Held};
use super::super::overlay::DeltaGroup;
use super::super::registry::{entry_capacity, plain_stack};
use super::super::{InventoryGestureError, PlayerInventoryLedger, helpers};

/// Where the extra results of a craft go after the main result.
#[derive(Clone, Copy)]
pub(in crate::inventory_ledger) enum ExtraDestination {
    /// The crafting grid first, as a manual craft returns them.
    Grid,
    /// The player inventory only, as a recipe-book craft returns them.
    Inventory,
}

/// The wire declaration of one craft's result.
pub(in crate::inventory_ledger) fn craft_result(
    entry: &ItemRegistryEntry,
    output: RecipeOutput,
) -> CraftResult {
    CraftResult {
        identifier: Arc::clone(&entry.identifier),
        aux: i32::from(output.aux),
        count: u16::from(output.count),
        block_runtime_id: output.block_runtime_id,
        user_data: user_data(output),
    }
}

/// The stack a craft creates for `count` items of `output` under `request_id`.
pub(in crate::inventory_ledger) fn created_stack(
    output: RecipeOutput,
    count: u16,
    request_id: i32,
) -> NetworkItemStack {
    let user_data = user_data(output);
    NetworkItemStack {
        network_id: output.network_id,
        metadata: u32::from(output.aux),
        stack_network_id: request_id,
        count,
        nbt_digest: Sha256::digest(&user_data).into(),
        block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
        extra_data: user_data,
    }
}

fn user_data(output: RecipeOutput) -> Arc<[u8]> {
    if output.empty_envelope {
        Arc::from([0; 10])
    } else {
        Arc::from([])
    }
}

impl PlayerInventoryLedger {
    /// Every result's declaration in recipe order, or `None` when one is unknown to the registry.
    pub(in crate::inventory_ledger) fn craft_results(
        &self,
        recipe: &RecipeHandle,
    ) -> Option<Arc<[CraftResult]>> {
        std::iter::once(recipe.output())
            .chain(recipe.extra_outputs())
            .map(|output| {
                self.negotiated_item_entry(output.network_id)
                    .map(|entry| craft_result(entry, output))
            })
            .collect()
    }

    /// Creates each extra result `crafts` times over and places it, after `consumed` grid and
    /// inventory amounts and away from the main result's `taken` cells; returns whether a
    /// placement merged into an occupied stack, and every cell filled.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::inventory_ledger) fn push_extra_outputs(
        &self,
        recipe: &RecipeHandle,
        crafts: u8,
        request_id: i32,
        destination: ExtraDestination,
        consumed: &[(Cell, u16)],
        taken: &[Cell],
        actions: &mut Vec<StackRequestAction>,
        groups: &mut Vec<DeltaGroup>,
    ) -> Result<(bool, Vec<Cell>), InventoryGestureError> {
        let mut merged = false;
        let output_slot = helpers::request_slot(Cell::CreatedOutput, request_id, None)?;
        // Cells this request already filled are not offered to a later result.
        let mut filled: Vec<Cell> = taken.to_vec();
        for (index, output) in recipe.extra_outputs().enumerate() {
            let capacity = self
                .negotiated_item_entry(output.network_id)
                .and_then(entry_capacity)
                .ok_or(InventoryGestureError::InvalidRequest)?;
            let total = u16::from(output.count) * u16::from(crafts);
            let created = created_stack(output, total, request_id);
            actions.push(StackRequestAction::Create {
                results_index: u8::try_from(index + 1)
                    .map_err(|_| InventoryGestureError::InvalidRequest)?,
            });
            groups.push(DeltaGroup::Set {
                cell: Cell::CreatedOutput,
                held: Held {
                    source_stack_id: Some(request_id),
                    source_origin: None,
                    stack: created.clone(),
                    overlay: None,
                    acknowledged_request_id: None,
                },
            });
            let mut plan = match destination {
                ExtraDestination::Grid => self.grid_plan(&created, capacity, consumed, &filled),
                ExtraDestination::Inventory => Vec::new(),
            };
            let placed: u16 = plan.iter().map(|(_, amount, _)| *amount).sum();
            if placed < total {
                let rest = NetworkItemStack {
                    count: total - placed,
                    ..created.clone()
                };
                plan.extend(self.spread_plan_partial(&rest, capacity, consumed, &filled));
            }
            let mut left = total;
            for (cell, amount, into) in plan {
                let amount = amount.min(left);
                if amount == 0 {
                    break;
                }
                left -= amount;
                merged |= into.is_some();
                filled.push(cell);
                actions.push(StackRequestAction::Place {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: output_slot,
                    destination: match cell {
                        Cell::Inventory(slot) => {
                            helpers::combined_request_slot(slot, into.map_or(0, |(id, _)| id))?
                        }
                        _ => helpers::request_slot(cell, into.map_or(0, |(id, _)| id), None)?,
                    },
                });
                groups.push(DeltaGroup::Transfer {
                    source: Cell::CreatedOutput,
                    destination: cell,
                    amount,
                    source_id: request_id,
                    destination_id: into.map(|(id, _)| id),
                    capacity: into.map(|_| u16::from(capacity)),
                });
            }
            // Whatever fits nowhere is dropped, in amounts one action can carry.
            while left != 0 {
                let amount = left.min(u16::from(protocol::MAX_STACK_REQUEST_AMOUNT));
                left -= amount;
                actions.push(StackRequestAction::Drop {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: output_slot,
                    randomly: false,
                });
                groups.push(DeltaGroup::Shrink {
                    source: Cell::CreatedOutput,
                    amount,
                    source_id: request_id,
                });
            }
        }
        Ok((merged, filled))
    }

    /// Grid placements: stackable cells in grid order, then the first empty one.
    fn grid_plan(
        &self,
        created: &NetworkItemStack,
        capacity: u8,
        consumed: &[(Cell, u16)],
        filled: &[Cell],
    ) -> Vec<super::OutputPlacement> {
        let capacity = u16::from(capacity);
        let mut remaining = created.count;
        let mut plan = Vec::new();
        let mut first_empty = None;
        for cell in self.crafting_grid().slots().map(Cell::Craft) {
            if remaining == 0 || filled.contains(&cell) {
                continue;
            }
            let used: u16 = consumed
                .iter()
                .filter(|(consumed, _)| *consumed == cell)
                .map(|(_, amount)| *amount)
                .sum();
            let held = self.view().get(cell).filter(|held| held.stack.count > used);
            let Some(into) = held else {
                first_empty.get_or_insert(cell);
                continue;
            };
            let count = into.stack.count - used;
            let same = Self::same_item(&into.stack, created)
                && plain_stack(&into.stack)
                && plain_stack(created);
            if !same || self.awaiting_identity(into) {
                continue;
            }
            let amount = remaining.min(capacity.saturating_sub(count));
            if amount > 0 {
                remaining -= amount;
                plan.push((cell, amount, Some((into.stack.stack_network_id, count))));
            }
        }
        if let Some(cell) = first_empty.filter(|_| remaining > 0) {
            plan.push((cell, remaining.min(capacity), None));
        }
        plan
    }
}
