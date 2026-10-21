//! Recipe-book crafting: the vanilla auto-craft request draws ingredients
//! straight from the player inventory and hands the result to the cursor.

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
        taken: &[(u8, u16)],
    ) -> Option<Vec<(u8, u16, i32)>> {
        let mut needed = u16::from(ingredient.count);
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
        self.auto_craft_sources(recipe).is_some()
    }

    /// Shares deterministic ingredient allocation between craftability and request construction.
    fn auto_craft_sources(&self, recipe: &RecipeHandle) -> Option<Vec<(u8, u16, i32)>> {
        let mut taken: Vec<(u8, u16)> = Vec::new();
        let mut all = Vec::new();
        let ingredients = recipe.ingredient_views();
        for ingredient in ingredients.iter().flatten() {
            let Some(sources) = self.ingredient_sources(ingredient, &taken) else {
                return self.reassigned_sources(&ingredients);
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
    ) -> Option<Vec<(u8, u16, i32)>> {
        let ingredients: Vec<_> = views.iter().flatten().collect();
        let demands: Vec<_> = ingredients
            .iter()
            .map(|ingredient| u16::from(ingredient.count))
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

    /// Crafts `recipe` once from inventory ingredients into an empty cursor.
    pub fn begin_auto_craft(
        &mut self,
        recipe: &RecipeHandle,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor, Cell::CreatedOutput])?;
        if self.view().get(Cell::Cursor).is_some() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let sources = self
            .auto_craft_sources(recipe)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let output = recipe.output();
        let entry = self
            .negotiated_item_entry(output.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let total = Some(output.count)
            .filter(|total| entry_capacity(entry).is_some_and(|capacity| *total <= capacity))
            .ok_or(InventoryGestureError::InvalidRequest)?;
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
            block_runtime_id: i32::try_from(output.block_runtime_id)
                .map_err(|_| InventoryGestureError::InvalidRequest)?,
            extra_data: Arc::clone(&user_data),
        };
        // Vanilla lists only the recipe's real ingredients, each with count 1, and skips empty
        // grid cells; the sources above still consume each ingredient's full count.
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
                crafts: 1,
                ingredients,
            },
            StackRequestAction::CraftResultsDeprecated {
                results: Arc::from([CraftResult {
                    identifier: Arc::clone(&entry.identifier),
                    aux: i32::from(output.aux),
                    count: u16::from(output.count),
                    block_runtime_id: output.block_runtime_id,
                    user_data,
                }]),
                crafts: 1,
            },
        ];
        let mut groups = Vec::new();
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
        }
        actions.push(StackRequestAction::Take {
            amount: total,
            source: request_slot(Cell::CreatedOutput, request_id, None)?,
            destination: request_slot(Cell::Cursor, 0, None)?,
        });
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
        groups.push(DeltaGroup::Transfer {
            source: Cell::CreatedOutput,
            destination: Cell::Cursor,
            amount: u16::from(total),
            source_id: request_id,
            destination_id: None,
            capacity: None,
        });
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: true,
            registry_bound_merge: false,
        })
    }
}
