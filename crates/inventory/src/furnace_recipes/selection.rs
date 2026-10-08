use protocol::{NetworkItemStack, ScreenRecipe};

use crate::PlayerInventoryLedger;
use crate::inventory_ledger::InventoryGestureError;

#[derive(Clone, Debug)]
pub(crate) struct Selection {
    generation: u64,
    result: NetworkItemStack,
    ghosts: [Option<NetworkItemStack>; 3],
    original: [Option<NetworkItemStack>; protocol::PLAYER_INVENTORY_SLOTS as usize],
}

impl PlayerInventoryLedger {
    fn furnace_selection(&self) -> Option<&Selection> {
        self.furnace_selection
            .as_ref()
            .filter(|selection| self.storage_generation() == Some(selection.generation))
    }

    /// Preview stacks never enter inventory authority or gesture addressing.
    pub fn furnace_ghost_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        self.furnace_selection()?
            .ghosts
            .get(usize::from(slot))?
            .as_ref()
    }

    /// Actual station items take precedence over preview items in presentation.
    pub fn furnace_visual_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        self.storage_stack(slot)
            .or_else(|| self.furnace_ghost_stack(slot))
    }

    /// The result selected in the current window generation.
    pub fn selected_furnace_result(&self) -> Option<&NetworkItemStack> {
        Some(&self.furnace_selection()?.result)
    }

    /// Deselecting removes previews without moving actual inventory items.
    pub fn clear_furnace_recipe(&mut self) {
        self.furnace_selection = None;
    }

    /// A role retires its preview once it has held an actual item.
    pub(crate) fn observe_furnace_cells(&mut self) {
        let occupied = [
            self.storage_stack(0).is_some(),
            self.storage_stack(2).is_some(),
        ];
        if let Some(selection) = &mut self.furnace_selection {
            for (slot, occupied) in [0, 2].into_iter().zip(occupied) {
                if occupied {
                    selection.ghosts[slot] = None;
                }
            }
        }
    }

    pub(super) fn furnace_result_selected(&self, recipe: &ScreenRecipe) -> bool {
        recipe
            .output
            .zip(self.selected_furnace_result())
            .is_some_and(|(output, selected)| {
                selected.network_id == output.network_id
                    && selected.metadata == u32::from(output.aux)
                    && selected.block_runtime_id == output.block_runtime_id as i32
            })
    }

    pub(super) fn prepare_furnace_selection(
        &mut self,
        recipe: &ScreenRecipe,
        supplied: bool,
    ) -> Result<Option<i32>, InventoryGestureError> {
        let generation = self
            .storage_generation()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let previous = self.furnace_selection().cloned();
        let mut last_request = None;
        if !supplied
            || self
                .storage_stack(0)
                .is_some_and(|stack| !self.furnace_input_matches(recipe, stack))
        {
            last_request = self.restore_furnace_cell(0, previous.as_ref())?;
        }
        if let Some(request) = self.restore_furnace_cell(2, previous.as_ref())? {
            last_request = Some(request);
        }
        let original = std::array::from_fn(|slot| self.displayed_stack(slot as u8).cloned());
        let output = recipe.output.ok_or(InventoryGestureError::InvalidRequest)?;
        let result = NetworkItemStack {
            network_id: output.network_id,
            metadata: u32::from(output.aux),
            count: u16::from(output.count),
            block_runtime_id: output.block_runtime_id as i32,
            ..NetworkItemStack::empty()
        };
        let mut ghosts = [None, None, None];
        if !supplied {
            ghosts[2] = Some(result.clone());
            let ingredient = &recipe.ingredients[0];
            let input = self.item_registry_snapshot().and_then(|registry| {
                registry
                    .values()
                    .find(|entry| {
                        crate::screen_ingredient_accepts(
                            ingredient,
                            &entry.identifier,
                            u32::from(ingredient.aux),
                            &entry.item_tags,
                        )
                    })
                    .map(|entry| NetworkItemStack {
                        network_id: entry.network_id,
                        metadata: u32::from(ingredient.aux),
                        count: 1,
                        ..NetworkItemStack::empty()
                    })
            });
            ghosts[0] = input;
        }
        self.furnace_selection = Some(Selection {
            generation,
            result,
            ghosts,
            original,
        });
        Ok(last_request)
    }

    fn restore_furnace_cell(
        &mut self,
        source: u8,
        previous: Option<&Selection>,
    ) -> Result<Option<i32>, InventoryGestureError> {
        let mut last_request = None;
        if let Some(previous) = previous {
            for (slot, original) in previous.original.iter().enumerate() {
                let (Some(stack), Some(original)) = (self.storage_stack(source), original) else {
                    continue;
                };
                if !Self::same_item(stack, original) {
                    continue;
                }
                let current_count = self
                    .displayed_stack(slot as u8)
                    .map_or(0, |stack| stack.count);
                let missing = original.count.saturating_sub(current_count);
                if missing != 0
                    && let Ok(request) =
                        self.begin_restore_furnace_source(source, slot as u8, missing)
                {
                    last_request = Some(request);
                }
            }
        }
        if self.storage_stack(source).is_some() {
            last_request = Some(self.begin_return_furnace_cell(source)?);
        }
        if self.storage_stack(source).is_some() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        Ok(last_request)
    }
}
