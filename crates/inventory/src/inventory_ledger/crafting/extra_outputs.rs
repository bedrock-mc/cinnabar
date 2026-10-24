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
use super::super::registry::entry_capacity;
use super::super::{InventoryGestureError, PlayerInventoryLedger, helpers};
use super::{OutputPlanner, action_amounts};

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

    /// Creates each extra result `crafts` times over and places it through the request's
    /// `planner`: the grid's matching stacks and first free cell, then the inventory (or only
    /// the inventory), dropping what fits nowhere. Returns whether a placement merged.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::inventory_ledger) fn push_extra_outputs(
        &self,
        recipe: &RecipeHandle,
        crafts: u8,
        request_id: i32,
        destination: ExtraDestination,
        planner: &mut OutputPlanner,
        actions: &mut Vec<StackRequestAction>,
        groups: &mut Vec<DeltaGroup>,
    ) -> Result<bool, InventoryGestureError> {
        let mut merged = false;
        let output_slot = helpers::request_slot(Cell::CreatedOutput, request_id, None)?;
        let grid: Vec<Cell> = self.crafting_grid().slots().map(Cell::Craft).collect();
        let player = self.player_cells();
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
                    // No retained source: each placement is stamped with its own cell, so
                    // results of one craft never share a lineage.
                    source_stack_id: None,
                    source_origin: None,
                    stack: created.clone(),
                    overlay: None,
                    acknowledged_request_id: None,
                },
            });
            let mut plan = match destination {
                ExtraDestination::Grid => planner.plan(self, &created, capacity, &grid, true),
                ExtraDestination::Inventory => Vec::new(),
            };
            let placed: u16 = plan.iter().map(|(_, amount, _)| *amount).sum();
            if placed < total {
                let rest = NetworkItemStack {
                    count: total - placed,
                    ..created.clone()
                };
                plan.extend(planner.plan(self, &rest, capacity, &player, false));
            }
            let mut left = total;
            for (cell, amount, into) in plan {
                left -= amount;
                merged |= into.is_some();
                let destination_id = into.map_or(0, |(id, _)| id);
                actions.push(StackRequestAction::Place {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: output_slot,
                    destination: match cell {
                        Cell::Inventory(slot) => {
                            helpers::combined_request_slot(slot, destination_id)?
                        }
                        _ => helpers::request_slot(cell, destination_id, None)?,
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
            for amount in action_amounts(left) {
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
        Ok(merged)
    }
}
