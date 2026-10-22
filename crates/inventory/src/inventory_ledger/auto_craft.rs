//! Recipe-book crafting: the auto-craft request draws ingredients straight from
//! the player inventory and places the result back into it, hotbar first.

use std::sync::Arc;

use protocol::{
    AutoCraftIngredient, CraftResult, NetworkItemStack, RecipeHandle, RecipeIngredientView,
    StackItemDescriptor, StackRequestAction,
};
use sha2::{Digest, Sha256};

use super::cells::{Cell, Held};
use super::crafting::{OutputPlanner, action_amounts};
use super::gesture::Submission;
use super::helpers::{combined_request_slot, request_slot};
use super::overlay::DeltaGroup;
use super::registry::{entry_capacity, recipe_input_valid};
use super::{InventoryGestureError, PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};

mod allocation;

impl PlayerInventoryLedger {
    /// Settled stacks with intact retained bytes and a negotiated binding can supply ingredients.
    fn auto_craft_stack(
        &self,
        slot: u8,
    ) -> Option<(&NetworkItemStack, &protocol::ItemRegistryEntry)> {
        let held = self.view().get(Cell::Inventory(slot))?;
        let entry = self.negotiated_item_entry(held.stack.network_id)?;
        (recipe_input_valid(&held.stack)
            && !crate::item_permissions::special_removal_locked(&held.stack.extra_data)
            && !self.awaiting_identity(held))
        .then_some((&held.stack, entry))
    }

    /// Player cells to draw `ingredient` from: `(cell, amount, stack id)`.
    fn ingredient_sources(
        &self,
        ingredient: &RecipeIngredientView,
        crafts: u8,
        taken: &[(u8, u16)],
    ) -> Option<Vec<(u8, u16, i32)>> {
        let mut needed = u16::from(ingredient.count) * u16::from(crafts);
        let mut sources = Vec::new();
        for slot in 0..PLAYER_INVENTORY_SLOT_COUNT as u8 {
            if needed == 0 {
                break;
            }
            let Some((stack, entry)) = self.auto_craft_stack(slot) else {
                continue;
            };
            let already: u16 = taken
                .iter()
                .filter(|(cell, _)| *cell == slot)
                .map(|(_, amount)| *amount)
                .sum();
            if !crate::ingredient_accepts(
                ingredient,
                &entry.identifier,
                stack.metadata,
                &entry.item_tags,
            ) {
                continue;
            }
            let amount = needed.min(stack.count.saturating_sub(already));
            if amount > 0 {
                needed -= amount;
                sources.push((slot, amount, stack.stack_network_id));
            }
        }
        (needed == 0).then_some(sources)
    }

    /// Whether any inventory stack is one of `recipe`'s ingredients.
    #[must_use]
    pub fn holds_any_ingredient(&self, recipe: &RecipeHandle) -> bool {
        let views = recipe.ingredient_views();
        (0..PLAYER_INVENTORY_SLOT_COUNT as u8).any(|slot| {
            let Some(held) = self.view().get(Cell::Inventory(slot)) else {
                return false;
            };
            let Some(entry) = self.negotiated_item_entry(held.stack.network_id) else {
                return false;
            };
            views.iter().flatten().any(|ingredient| {
                crate::ingredient_accepts(
                    ingredient,
                    &entry.identifier,
                    held.stack.metadata,
                    &entry.item_tags,
                )
            })
        })
    }

    /// Whether the inventory holds every ingredient of `recipe`.
    #[must_use]
    pub fn can_auto_craft(&self, recipe: &RecipeHandle) -> bool {
        self.auto_craft_sources(recipe, 1).is_some()
    }

    /// Shares deterministic ingredient allocation between craftability and request construction.
    fn auto_craft_sources(&self, recipe: &RecipeHandle, crafts: u8) -> Option<Vec<(u8, u16, i32)>> {
        let mut taken: Vec<(u8, u16)> = Vec::new();
        let mut all = Vec::new();
        let ingredients = recipe.ingredient_views();
        for ingredient in ingredients.iter().flatten() {
            let Some(sources) = self.ingredient_sources(ingredient, crafts, &taken) else {
                return self.reassigned_sources(&ingredients, crafts);
            };
            taken.extend(sources.iter().map(|(cell, amount, _)| (*cell, *amount)));
            all.extend(sources);
        }
        Some(all)
    }

    /// Reassigns quantities only when the ordinary first-choice plan cannot satisfy every input.
    fn reassigned_sources(
        &self,
        views: &[Option<RecipeIngredientView>],
        crafts: u8,
    ) -> Option<Vec<(u8, u16, i32)>> {
        let ingredients: Vec<_> = views.iter().flatten().collect();
        let demands: Vec<_> = ingredients
            .iter()
            .map(|ingredient| u16::from(ingredient.count) * u16::from(crafts))
            .collect();
        let mut counts = [0; PLAYER_INVENTORY_SLOT_COUNT];
        let mut identities = [0; PLAYER_INVENTORY_SLOT_COUNT];
        let mut accepts = [[false; PLAYER_INVENTORY_SLOT_COUNT]; protocol::MAX_RECIPE_INGREDIENTS];
        for slot in 0..PLAYER_INVENTORY_SLOT_COUNT {
            let Some((stack, entry)) = self.auto_craft_stack(slot as u8) else {
                continue;
            };
            counts[slot] = stack.count;
            identities[slot] = stack.stack_network_id;
            for (index, ingredient) in ingredients.iter().enumerate() {
                accepts[index][slot] = crate::ingredient_accepts(
                    ingredient,
                    &entry.identifier,
                    stack.metadata,
                    &entry.item_tags,
                );
            }
        }
        let assigned = allocation::plan(&demands, &counts, &accepts)?;
        let mut sources = Vec::new();
        for row in assigned.iter().take(ingredients.len()) {
            for (slot, amount) in row
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, amount)| *amount > 0)
            {
                sources.push((slot as u8, amount, identities[slot]));
            }
        }
        Some(sources)
    }

    /// How many times the inventory can craft `recipe` at once, capped at one output stack.
    fn max_auto_crafts(&self, recipe: &RecipeHandle, capacity: u8) -> u8 {
        let per_stack = capacity / recipe.output().count.max(1);
        let (mut low, mut high) = (0u8, per_stack);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if self.auto_craft_sources(recipe, middle).is_some() {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        low
    }

    /// Crafts `recipe` from inventory ingredients once, or with `all` as many times as one
    /// output stack allows, placing the output into the inventory and any remainder on an
    /// empty cursor.
    pub fn begin_auto_craft(
        &mut self,
        recipe: &RecipeHandle,
        all: bool,
    ) -> Result<i32, InventoryGestureError> {
        let crafts = if all {
            let capacity = self
                .negotiated_item_entry(recipe.output().network_id)
                .and_then(entry_capacity)
                .ok_or(InventoryGestureError::InvalidRequest)?;
            self.max_auto_crafts(recipe, capacity)
        } else {
            1
        };
        if crafts == 0 {
            return Err(InventoryGestureError::InvalidRequest);
        }
        // Craft as many times as one encodable request allows; the rest waits for another click.
        let mut submission = self.auto_craft_submission(recipe, crafts);
        if matches!(submission, Err(InventoryGestureError::InvalidRequest)) && crafts > 1 {
            let (mut low, mut high) = (1u8, crafts - 1);
            submission = self.auto_craft_submission(recipe, 1);
            while low < high && submission.is_ok() {
                let middle = low + (high - low).div_ceil(2);
                match self.auto_craft_submission(recipe, middle) {
                    Ok(fits) => {
                        low = middle;
                        submission = Ok(fits);
                    }
                    Err(InventoryGestureError::InvalidRequest) => high = middle - 1,
                    Err(error) => return Err(error),
                }
            }
        }
        self.submit(submission?)
    }

    /// Plans one recipe-book request of `crafts` without reserving an ID or changing cells;
    /// `InvalidRequest` when it cannot be made or would not fit one encodable request.
    fn auto_craft_submission(
        &self,
        recipe: &RecipeHandle,
        crafts: u8,
    ) -> Result<Submission, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor, Cell::CreatedOutput])?;
        let output = recipe.output();
        let entry = self
            .negotiated_item_entry(output.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let capacity = entry_capacity(entry)
            .filter(|capacity| output.count != 0 && output.count <= *capacity)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let user_data: Arc<[u8]> = if output.empty_envelope {
            Arc::from([0; 10])
        } else {
            Arc::from([])
        };
        let one_craft = NetworkItemStack {
            network_id: output.network_id,
            metadata: u32::from(output.aux),
            stack_network_id: 0,
            count: u16::from(output.count),
            nbt_digest: Sha256::digest(&user_data).into(),
            block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
            extra_data: Arc::clone(&user_data),
        };
        // Before anything is consumed, one craft's output must fit an empty cell or a partial stack.
        if !self.player_cell_fits(&one_craft, capacity) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let sources = self
            .auto_craft_sources(recipe, crafts)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let total = output.count * crafts;
        let request_id = self.peek_request_id()?;
        let created = NetworkItemStack {
            stack_network_id: request_id,
            count: u16::from(total),
            ..one_craft
        };
        // Each ingredient is listed once with count 1 and empty grid cells are skipped;
        // the sources above still consume every ingredient's full count per craft.
        let ingredients: Arc<[AutoCraftIngredient]> = recipe
            .ingredient_views()
            .iter()
            .flatten()
            .map(|ingredient| AutoCraftIngredient {
                descriptor: if ingredient.tag {
                    StackItemDescriptor::Tag(Arc::clone(&ingredient.name))
                } else {
                    StackItemDescriptor::Name {
                        identifier: Arc::clone(&ingredient.name),
                        aux: i32::from(ingredient.aux),
                    }
                },
                count: 1,
            })
            .collect();
        let mut actions = vec![
            StackRequestAction::AutoCraft {
                recipe_network_id: recipe.network_id(),
                crafts,
                ingredients,
            },
            // Declares one craft's results; the times-crafted field carries the repeat.
            StackRequestAction::CraftResultsDeprecated {
                results: Arc::from([CraftResult {
                    identifier: Arc::clone(&entry.identifier),
                    aux: i32::from(output.aux),
                    count: u16::from(output.count),
                    block_runtime_id: output.block_runtime_id,
                    user_data,
                }]),
                crafts,
            },
        ];
        let mut groups = Vec::new();
        let mut consumed = Vec::new();
        for (slot, total, id) in sources {
            let cell = Cell::Inventory(slot);
            // Crafting accepts ingredients only from the combined inventory name, and each
            // consume carries at most one action's amount.
            for amount in action_amounts(total) {
                actions.push(StackRequestAction::Consume {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: combined_request_slot(slot, id)?,
                });
                groups.push(DeltaGroup::Shrink {
                    source: cell,
                    amount,
                    source_id: id,
                });
            }
            consumed.push(cell);
        }
        // Never into a cell this request empties: a queued earlier request may still rename it.
        let plan = OutputPlanner::new(consumed).plan(
            self,
            &created,
            capacity,
            &self.player_cells(),
            false,
        );
        let placed: u16 = plan.iter().map(|(_, amount, _)| *amount).sum();
        let remainder = created.count - placed;
        // A remainder goes to an empty cursor in one action.
        if remainder != 0
            && (self.view().get(Cell::Cursor).is_some()
                || remainder > u16::from(protocol::MAX_STACK_REQUEST_AMOUNT))
        {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let output_slot = request_slot(Cell::CreatedOutput, request_id, None)?;
        groups.push(DeltaGroup::Set {
            cell: Cell::CreatedOutput,
            held: Held {
                source_stack_id: Some(created.stack_network_id),
                source_origin: None,
                stack: created,
                overlay: None,
                acknowledged_request_id: None,
            },
        });
        let registry_bound_merge = plan.iter().any(|(_, _, into)| into.is_some());
        for (cell, amount, into) in plan {
            let Cell::Inventory(slot) = cell else {
                return Err(InventoryGestureError::InvalidRequest);
            };
            actions.push(StackRequestAction::Place {
                amount: u8::try_from(amount).map_err(|_| InventoryGestureError::InvalidRequest)?,
                source: output_slot,
                destination: combined_request_slot(slot, into.map_or(0, |(id, _)| id))?,
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
        if remainder != 0 {
            actions.push(StackRequestAction::Take {
                amount: u8::try_from(remainder)
                    .map_err(|_| InventoryGestureError::InvalidRequest)?,
                source: output_slot,
                destination: request_slot(Cell::Cursor, 0, None)?,
            });
            groups.push(DeltaGroup::Transfer {
                source: Cell::CreatedOutput,
                destination: Cell::Cursor,
                amount: remainder,
                source_id: request_id,
                destination_id: None,
                capacity: None,
            });
        }
        if actions.len() > protocol::MAX_STACK_REQUEST_ACTIONS {
            return Err(InventoryGestureError::InvalidRequest);
        }
        Ok(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: true,
            registry_bound_merge,
        })
    }
}
