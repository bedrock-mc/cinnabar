//! Recipe-book crafting: the auto-craft request draws ingredients straight from
//! the player inventory and places the result back into it, hotbar first.

use std::sync::Arc;

use protocol::{
    AutoCraftIngredient, CraftResult, NetworkItemStack, RecipeHandle, RecipeIngredientView,
    StackItemDescriptor, StackRequestAction,
};
use sha2::{Digest, Sha256};

use super::cells::{Cell, Held};
use super::gesture::Submission;
use super::helpers::request_slot;
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
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor, Cell::CreatedOutput])?;
        let output = recipe.output();
        let entry = self
            .negotiated_item_entry(output.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let capacity = entry_capacity(entry)
            .filter(|capacity| output.count != 0 && output.count <= *capacity)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let crafts = if all {
            self.max_auto_crafts(recipe, capacity)
        } else {
            1
        };
        if crafts == 0 {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let sources = self
            .auto_craft_sources(recipe, crafts)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let total = output.count * crafts;
        let request_id = self.peek_request_id()?;
        let user_data: Arc<[u8]> = if output.empty_envelope {
            Arc::from([0; 10])
        } else {
            Arc::from([])
        };
        let created = NetworkItemStack {
            network_id: output.network_id,
            metadata: u32::from(output.aux),
            stack_network_id: request_id,
            count: u16::from(total),
            nbt_digest: Sha256::digest(&user_data).into(),
            block_runtime_id: i32::from_ne_bytes(output.block_runtime_id.to_ne_bytes()),
            extra_data: Arc::clone(&user_data),
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
        for (slot, amount, id) in sources {
            let cell = Cell::Inventory(slot);
            actions.push(StackRequestAction::Consume {
                amount: u8::try_from(amount).map_err(|_| InventoryGestureError::InvalidRequest)?,
                source: request_slot(cell, id, None)?,
            });
            groups.push(DeltaGroup::Shrink {
                source: cell,
                amount,
                source_id: id,
            });
            consumed.push((cell, amount));
        }
        let plan = self.spread_plan_partial(&created, capacity, &consumed);
        let placed: u16 = plan.iter().map(|(_, amount, _)| *amount).sum();
        let remainder = created.count - placed;
        if remainder != 0 && self.view().get(Cell::Cursor).is_some() {
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
            actions.push(StackRequestAction::Place {
                amount: u8::try_from(amount).map_err(|_| InventoryGestureError::InvalidRequest)?,
                source: output_slot,
                destination: request_slot(cell, into.map_or(0, |(id, _)| id), None)?,
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
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: true,
            registry_bound_merge,
        })
    }
}
