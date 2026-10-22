//! Recipe-book crafting: the auto-craft request draws ingredients from the crafting grid and
//! the player inventory and places the result into the inventory, hotbar first.

use std::sync::Arc;

use protocol::{
    AutoCraftIngredient, NetworkItemStack, RecipeHandle, RecipeIngredientView, StackItemDescriptor,
    StackRequestAction,
};

use super::cells::{Cell, Held};
use super::crafting::{ExtraDestination, OutputPlanner, action_amounts, created_stack};
use super::gesture::Submission;
use super::helpers::{combined_request_slot, request_slot};
use super::overlay::DeltaGroup;
use super::registry::{entry_capacity, recipe_input_valid};
use super::{InventoryGestureError, PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};

/// One ingredient's `(cell, amount, stack id)` sources.
pub(super) type IngredientSources = Vec<(Cell, u16, i32)>;

impl PlayerInventoryLedger {
    /// Cells recipe-book availability, placement and crafting draw from, in order: the open
    /// grid, then the player inventory hotbar first.
    pub(super) fn recipe_source_cells(&self) -> Vec<Cell> {
        self.crafting_grid()
            .slots()
            .map(Cell::Craft)
            .chain((0..PLAYER_INVENTORY_SLOT_COUNT as u8).map(Cell::Inventory))
            .collect()
    }

    /// Settled stacks with intact retained bytes and a negotiated binding can supply ingredients.
    pub(super) fn auto_craft_stack(
        &self,
        cell: Cell,
    ) -> Option<(&NetworkItemStack, &protocol::ItemRegistryEntry)> {
        let held = self.view().get(cell)?;
        let entry = self.negotiated_item_entry(held.stack.network_id)?;
        (recipe_input_valid(&held.stack)
            && !crate::item_permissions::special_removal_locked(&held.stack.extra_data)
            && !self.awaiting_identity(held))
        .then_some((&held.stack, entry))
    }

    /// Whether the source stack in `cell` is one `ingredient` accepts.
    pub(super) fn source_accepts(&self, cell: Cell, ingredient: &RecipeIngredientView) -> bool {
        self.auto_craft_stack(cell).is_some_and(|(stack, entry)| {
            crate::ingredient_accepts(
                ingredient,
                &entry.identifier,
                stack.metadata,
                &entry.item_tags,
            )
        })
    }

    /// Whether the grid or inventory holds any of `recipe`'s ingredients.
    #[must_use]
    pub fn holds_any_ingredient(&self, recipe: &RecipeHandle) -> bool {
        let views = recipe.ingredient_views();
        self.recipe_source_cells().into_iter().any(|cell| {
            views
                .iter()
                .flatten()
                .any(|ingredient| self.source_accepts(cell, ingredient))
        })
    }

    /// Whether the grid and inventory together hold every ingredient of `recipe`.
    #[must_use]
    pub fn can_auto_craft(&self, recipe: &RecipeHandle) -> bool {
        self.auto_craft_sources(recipe, 1).is_some()
    }

    /// Shares deterministic ingredient allocation between craftability and request construction:
    /// each ingredient's sources, in ingredient order.
    pub(super) fn auto_craft_sources(
        &self,
        recipe: &RecipeHandle,
        crafts: u8,
    ) -> Option<Vec<IngredientSources>> {
        let views = recipe.ingredient_views();
        let sources = self.recipe_allocation(&views).plan(crafts, false)?;
        Some(
            views
                .iter()
                .zip(sources)
                .filter_map(|(view, sources)| view.as_ref().map(|_| sources))
                .collect(),
        )
    }

    /// Names `ingredient` as the request declares it: a tag by its sources' shared item,
    /// since servers match consumed items against concrete names, anything else as listed.
    fn auto_craft_descriptor(
        &self,
        ingredient: &RecipeIngredientView,
        sources: &[(Cell, u16, i32)],
    ) -> StackItemDescriptor {
        let held = sources
            .first()
            .and_then(|(cell, _, _)| self.auto_craft_stack(*cell));
        match held {
            Some((_, entry)) if ingredient.tag => StackItemDescriptor::Name {
                identifier: Arc::clone(&entry.identifier),
                aux: i32::from(protocol::RECIPE_ANY_AUX),
            },
            _ if ingredient.tag => StackItemDescriptor::Tag(Arc::clone(&ingredient.name)),
            _ => StackItemDescriptor::Name {
                identifier: Arc::clone(&ingredient.name),
                aux: i32::from(ingredient.aux),
            },
        }
    }

    /// The highest repeat count one output stack and one variant per ingredient can supply.
    fn max_auto_crafts(&self, recipe: &RecipeHandle) -> u8 {
        let allocation = self.recipe_allocation(&recipe.ingredient_views());
        (1..=self.crafts_per_request(recipe).unwrap_or(0))
            .rev()
            .find(|crafts| allocation.plan(*crafts, false).is_some())
            .unwrap_or(0)
    }

    /// Crafts `recipe` from inventory ingredients once, or with `all` as many times as one
    /// output stack allows, placing the output into the inventory and any remainder on an
    /// empty cursor.
    pub fn begin_auto_craft(
        &mut self,
        recipe: &RecipeHandle,
        all: bool,
    ) -> Result<i32, InventoryGestureError> {
        let crafts = if all { self.max_auto_crafts(recipe) } else { 1 };
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
        let one_craft = created_stack(output, u16::from(output.count), 0);
        // Before anything is consumed, one craft's output must fit an empty cell or a partial stack.
        if !self.player_cell_fits(&one_craft, capacity) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let sources = self
            .auto_craft_sources(recipe, crafts)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let total = output.count * crafts;
        let request_id = self.peek_request_id()?;
        let created = created_stack(output, u16::from(total), request_id);
        // Each ingredient is listed once with count 1 and empty grid cells are skipped;
        // the sources above still consume every ingredient's full count per craft.
        let ingredients: Arc<[AutoCraftIngredient]> = recipe
            .ingredient_views()
            .iter()
            .flatten()
            .zip(&sources)
            .map(|(ingredient, sources)| AutoCraftIngredient {
                descriptor: self.auto_craft_descriptor(ingredient, sources),
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
                results: self
                    .craft_results(recipe)
                    .ok_or(InventoryGestureError::InvalidRequest)?,
                crafts,
            },
        ];
        let mut groups = Vec::new();
        let mut consumed = Vec::new();
        for (cell, total, id) in sources.into_iter().flatten() {
            // Crafting accepts ingredients only from the combined inventory name, and each
            // consume carries at most one action's amount.
            for amount in action_amounts(total) {
                actions.push(StackRequestAction::Consume {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: match cell {
                        Cell::Inventory(slot) => combined_request_slot(slot, id)?,
                        cell => request_slot(cell, id, None)?,
                    },
                });
                groups.push(DeltaGroup::Shrink {
                    source: cell,
                    amount,
                    source_id: id,
                });
            }
            consumed.push(cell);
        }
        // Extra results are placed before the main one, which is then created by index too.
        // Neither goes into a cell this request empties: a queued earlier request may still
        // rename it.
        let mut planner = OutputPlanner::new(consumed);
        let mut registry_bound_merge = false;
        if recipe.extra_outputs().len() != 0 {
            registry_bound_merge = self.push_extra_outputs(
                recipe,
                crafts,
                request_id,
                ExtraDestination::Inventory,
                &mut planner,
                &mut actions,
                &mut groups,
            )?;
            actions.push(StackRequestAction::Create { results_index: 0 });
        }
        let plan = planner.plan(self, &created, capacity, &self.player_cells(), false);
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
        registry_bound_merge |= plan.iter().any(|(_, _, into)| into.is_some());
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
