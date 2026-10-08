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
use super::registry::{entry_capacity, plain_stack};
use super::{InventoryGestureError, PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};

impl PlayerInventoryLedger {
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
            let Some(held) = self.view().get(Cell::Inventory(slot)) else {
                continue;
            };
            let already: u16 = taken
                .iter()
                .filter(|(cell, _)| *cell == slot)
                .map(|(_, amount)| *amount)
                .sum();
            let stack = &held.stack;
            let Some(entry) = self.negotiated_item_entry(stack.network_id) else {
                continue;
            };
            if !plain_stack(stack)
                || self.awaiting_identity(held)
                || !crate::ingredient_accepts(
                    ingredient,
                    &entry.identifier,
                    stack.metadata,
                    &entry.item_tags,
                )
            {
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

    fn auto_craft_sources(&self, recipe: &RecipeHandle) -> Option<Vec<(u8, u16, i32)>> {
        let mut taken: Vec<(u8, u16)> = Vec::new();
        let mut all = Vec::new();
        for ingredient in recipe.ingredient_views().iter().flatten() {
            let sources = self.ingredient_sources(ingredient, &taken)?;
            taken.extend(sources.iter().map(|(cell, amount, _)| (*cell, *amount)));
            all.extend(sources);
        }
        Some(all)
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
        let ingredients: Arc<[AutoCraftIngredient]> = recipe
            .ingredient_views()
            .iter()
            .map(|slot| match slot {
                Some(ingredient) => AutoCraftIngredient {
                    descriptor: if ingredient.tag {
                        StackItemDescriptor::Tag(Arc::clone(&ingredient.name))
                    } else {
                        StackItemDescriptor::Name {
                            identifier: Arc::clone(&ingredient.name),
                            aux: i32::from(ingredient.aux),
                        }
                    },
                    count: u16::from(ingredient.count),
                },
                None => AutoCraftIngredient {
                    descriptor: StackItemDescriptor::Empty,
                    count: 0,
                },
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
                stack: created,
                overlay: None,
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
