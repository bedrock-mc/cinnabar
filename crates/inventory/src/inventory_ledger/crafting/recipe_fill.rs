//! Recipe-book placement: returns the grid to the inventory, lays a recipe's ingredients into
//! it, as many crafts' worth as the inventory allows, and keeps the recipe as ghost items for
//! the cells it could not fill.

use protocol::{NetworkItemStack, RecipeHandle, RecipeIngredientView, StackRequestContainer};

use super::super::cells::{Cell, Held};
use super::super::gesture::{Built, StackRequestActionKind, counted_merge, counted_transfer};
use super::super::registry::{OccupiedStackRelation, entry_capacity};
use super::super::{InventoryGestureError, PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger};
use super::CraftingGrid;

/// What the last recipe-book placement left behind in one open crafting screen.
#[derive(Clone, Debug)]
pub(crate) struct RecipePlacement {
    /// The storage and personal window generations it belongs to.
    window: (Option<u64>, Option<u64>),
    /// Player cells before the placement drew from them; returns refill these first.
    original: Vec<Option<NetworkItemStack>>,
    selection: Option<Selection>,
}

/// The selected recipe's layout while the grid still holds what the placement left.
#[derive(Clone, Debug)]
struct Selection {
    output: NetworkItemStack,
    ghosts: Vec<Option<NetworkItemStack>>,
    grid: Vec<Option<(i32, u32, u16)>>,
}

impl PlayerInventoryLedger {
    /// Moves `recipe`'s ingredients from the inventory into the crafting grid after returning
    /// what the grid held, one request per transfer. Cells the inventory cannot fill keep the
    /// ingredient as a ghost until the grid changes. Returns the last request id, or 0.
    pub fn begin_recipe_placement(
        &mut self,
        recipe: &RecipeHandle,
    ) -> Result<i32, InventoryGestureError> {
        if !matches!(
            self.window_kind(),
            None | Some(protocol::WindowKind::Workbench)
        ) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.gesture_preflight(true)?;
        let mut staged = self.clone();
        let last = staged.place_recipe(recipe)?;
        *self = staged;
        Ok(last)
    }

    fn place_recipe(&mut self, recipe: &RecipeHandle) -> Result<i32, InventoryGestureError> {
        let grid = self.crafting_grid();
        let mut last = self.evacuate_grid(grid)?;
        let window = self.recipe_window();
        let original = (0..PLAYER_INVENTORY_SLOT_COUNT as u8)
            .map(|slot| self.displayed_stack(slot).cloned())
            .collect();
        let Some(layout) = grid_layout(recipe, grid) else {
            // A recipe larger than the grid only clears it.
            self.recipe_placement = Some(RecipePlacement {
                window,
                original,
                selection: None,
            });
            return Ok(last);
        };
        let crafts = self.placement_crafts(recipe, &layout);
        let slots: Vec<u8> = grid.slots().collect();
        // Ghosts name what the inventory held before any cell drew from it.
        let ghosts: Vec<_> = layout
            .iter()
            .map(|ingredient| {
                ingredient
                    .as_ref()
                    .and_then(|ingredient| self.ghost_item(ingredient))
            })
            .collect();
        for (index, ingredient) in layout.iter().enumerate() {
            let Some(ingredient) = ingredient else {
                continue;
            };
            let cell = Cell::Craft(slots[index]);
            let mut remaining = u16::from(ingredient.count) * u16::from(crafts);
            for slot in self.cell_sources(ingredient, crafts) {
                if remaining == 0 {
                    break;
                }
                let Some(held) = self.view().get(Cell::Inventory(slot)).cloned() else {
                    continue;
                };
                let amount = remaining.min(held.stack.count);
                if let Some(request) = self.place_into_grid(slot, &held, cell, amount)? {
                    remaining -= amount;
                    last = request;
                }
            }
        }
        let output = recipe.output();
        self.recipe_placement = Some(RecipePlacement {
            window,
            original,
            selection: Some(Selection {
                output: NetworkItemStack {
                    network_id: output.network_id,
                    metadata: u32::from(output.aux),
                    count: u16::from(output.count),
                    block_runtime_id: output.block_runtime_id as i32,
                    ..NetworkItemStack::empty()
                },
                ghosts,
                grid: self.grid_contents(grid),
            }),
        });
        Ok(last)
    }

    /// Returns every grid stack: first to the player cells the last placement drew it from,
    /// then wherever it fits, dropping what does not.
    fn evacuate_grid(&mut self, grid: CraftingGrid) -> Result<i32, InventoryGestureError> {
        let original = self
            .recipe_placement
            .as_ref()
            .filter(|placement| placement.window == self.recipe_window())
            .map(|placement| placement.original.clone())
            .unwrap_or_default();
        let mut last = 0;
        for slot in grid.slots() {
            let cell = Cell::Craft(slot);
            for (player, before) in original.iter().enumerate() {
                let (Some(held), Some(before)) = (self.view().get(cell).cloned(), before) else {
                    continue;
                };
                if !Self::same_item(&held.stack, before) {
                    continue;
                }
                let destination = Cell::Inventory(player as u8);
                let now = self.view().get(destination).cloned();
                let missing = before
                    .count
                    .saturating_sub(now.as_ref().map_or(0, |now| now.stack.count))
                    .min(held.stack.count);
                if missing != 0
                    && let Some(request) =
                        self.transfer_one(cell, &held, destination, now.as_ref(), missing)?
                {
                    last = request;
                }
            }
        }
        for slot in grid.slots() {
            let cell = Cell::Craft(slot);
            if self.view().get(cell).is_some() {
                last = self.return_close_cell(cell)?;
            }
        }
        Ok(last)
    }

    /// One Place request from player `slot`, named in the combined inventory, into `cell`.
    fn place_into_grid(
        &mut self,
        slot: u8,
        held: &Held,
        cell: Cell,
        amount: u16,
    ) -> Result<Option<i32>, InventoryGestureError> {
        let into = self.view().get(cell).cloned();
        let source = Cell::Inventory(slot);
        let Some(mut built) = self.build_transfer(source, held, cell, into.as_ref(), amount)?
        else {
            return Ok(None);
        };
        if let protocol::StackRequestAction::Place { source, .. } = &mut built.action {
            source.container = StackRequestContainer::CombinedInventory;
        }
        let personal_generation = self.gesture_preflight(true)?;
        self.submit_built(built, personal_generation).map(Some)
    }

    /// One Place request moving `amount` of `held` from `source` into `destination`.
    fn transfer_one(
        &mut self,
        source: Cell,
        held: &Held,
        destination: Cell,
        into: Option<&Held>,
        amount: u16,
    ) -> Result<Option<i32>, InventoryGestureError> {
        let Some(built) = self.build_transfer(source, held, destination, into, amount)? else {
            return Ok(None);
        };
        let personal_generation = self.gesture_preflight(true)?;
        self.submit_built(built, personal_generation).map(Some)
    }

    /// A Place into an empty or compatible cell; `None` when `into` cannot take it.
    fn build_transfer(
        &self,
        source: Cell,
        held: &Held,
        destination: Cell,
        into: Option<&Held>,
        amount: u16,
    ) -> Result<Option<Built>, InventoryGestureError> {
        self.check_surfaces([source, destination])?;
        let held = self
            .named(Some(held.clone()))?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let Some(into) = into else {
            return counted_transfer(
                StackRequestActionKind::Place,
                source,
                destination,
                &held.stack,
                address,
                Some(amount),
            )
            .map(Some);
        };
        let OccupiedStackRelation::Compatible { capacity } =
            self.occupied_stack_relation(&held, into)
        else {
            return Ok(None);
        };
        let amount = amount.min(capacity.saturating_sub(into.stack.count));
        if amount == 0 {
            return Ok(None);
        }
        counted_merge(
            StackRequestActionKind::Place,
            source,
            destination,
            &held.stack,
            &into.stack,
            address,
            amount,
            capacity,
        )
        .map(Some)
    }

    /// Crafts' worth to place per cell: as many as the inventory supplies, at least one, and
    /// no more than one stack of any ingredient.
    fn placement_crafts(
        &self,
        recipe: &RecipeHandle,
        layout: &[Option<RecipeIngredientView>],
    ) -> u8 {
        let stack_limit = layout
            .iter()
            .flatten()
            .filter_map(|ingredient| {
                let slot = self.matching_slots(ingredient).next()?;
                let held = self.view().get(Cell::Inventory(slot))?;
                let capacity = entry_capacity(self.negotiated_item_entry(held.stack.network_id)?)?;
                Some(capacity / ingredient.count.max(1))
            })
            .min()
            .unwrap_or(1)
            .max(1);
        let (mut low, mut high) = (0u8, stack_limit);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if self.auto_craft_sources(recipe, middle).is_some() {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        low.max(1)
    }

    /// Player slots that may fill one cell: one item variant only, the first whose total
    /// covers `crafts`, else the first variant at all.
    fn cell_sources(&self, ingredient: &RecipeIngredientView, crafts: u8) -> Vec<u8> {
        let matching: Vec<u8> = self.matching_slots(ingredient).collect();
        let needed = u16::from(ingredient.count) * u16::from(crafts);
        let mut variants: Vec<(&NetworkItemStack, u16)> = Vec::new();
        for slot in &matching {
            let stack = &self
                .view()
                .get(Cell::Inventory(*slot))
                .expect("matched")
                .stack;
            match variants
                .iter_mut()
                .find(|(first, _)| Self::same_item(first, stack))
            {
                Some((_, total)) => *total = total.saturating_add(stack.count),
                None => variants.push((stack, stack.count)),
            }
        }
        let Some(chosen) = variants
            .iter()
            .find(|(_, total)| *total >= needed)
            .or(variants.first())
            .map(|(stack, _)| (*stack).clone())
        else {
            return Vec::new();
        };
        matching
            .into_iter()
            .filter(|slot| {
                self.view()
                    .get(Cell::Inventory(*slot))
                    .is_some_and(|held| Self::same_item(&held.stack, &chosen))
            })
            .collect()
    }

    /// Player slots, hotbar first, whose settled stack `ingredient` accepts.
    fn matching_slots<'a>(
        &'a self,
        ingredient: &'a RecipeIngredientView,
    ) -> impl Iterator<Item = u8> + 'a {
        (0..PLAYER_INVENTORY_SLOT_COUNT as u8).filter(move |slot| {
            self.auto_craft_stack(*slot).is_some_and(|(stack, entry)| {
                crate::ingredient_accepts(
                    ingredient,
                    &entry.identifier,
                    stack.metadata,
                    &entry.item_tags,
                )
            })
        })
    }

    /// The item a cell shows while empty: the held item the ingredient accepts, else the first
    /// registered one.
    fn ghost_item(&self, ingredient: &RecipeIngredientView) -> Option<NetworkItemStack> {
        if let Some(slot) = self.matching_slots(ingredient).next() {
            let stack = &self.view().get(Cell::Inventory(slot))?.stack;
            return Some(NetworkItemStack {
                network_id: stack.network_id,
                metadata: stack.metadata,
                block_runtime_id: stack.block_runtime_id,
                count: 1,
                ..NetworkItemStack::empty()
            });
        }
        let metadata = if ingredient.aux == protocol::RECIPE_ANY_AUX {
            0
        } else {
            u32::from(ingredient.aux)
        };
        self.item_registry_snapshot()?
            .values()
            .find(|entry| {
                crate::ingredient_accepts(ingredient, &entry.identifier, metadata, &entry.item_tags)
            })
            .map(|entry| NetworkItemStack {
                network_id: entry.network_id,
                metadata,
                count: 1,
                ..NetworkItemStack::empty()
            })
    }

    fn grid_contents(&self, grid: CraftingGrid) -> Vec<Option<(i32, u32, u16)>> {
        grid.slots()
            .map(|slot| {
                self.view()
                    .get(Cell::Craft(slot))
                    .map(|held| (held.stack.network_id, held.stack.metadata, held.stack.count))
            })
            .collect()
    }

    fn recipe_window(&self) -> (Option<u64>, Option<u64>) {
        (
            self.storage_generation(),
            self.personal_generation_for_gesture(),
        )
    }

    fn recipe_selection(&self) -> Option<&Selection> {
        self.recipe_placement
            .as_ref()
            .filter(|placement| placement.window == self.recipe_window())?
            .selection
            .as_ref()
    }

    /// The ingredient an empty crafting cell at UI `slot` shows for the selected recipe.
    pub fn crafting_ghost_stack(&self, slot: u8) -> Option<&NetworkItemStack> {
        let index = self.crafting_grid().slots().position(|cell| cell == slot)?;
        if self.view().get(Cell::Craft(slot)).is_some() {
            return None;
        }
        self.recipe_selection()?.ghosts.get(index)?.as_ref()
    }

    /// The selected recipe's result, shown while the grid makes nothing.
    pub fn crafting_ghost_output(&self) -> Option<&NetworkItemStack> {
        Some(&self.recipe_selection()?.output)
    }

    /// Drops the selected recipe's ghosts, as a tab or filter change does.
    pub fn clear_recipe_selection(&mut self) {
        if let Some(placement) = &mut self.recipe_placement {
            placement.selection = None;
        }
    }

    /// Any grid change after the placement ends the selection.
    pub(in crate::inventory_ledger) fn observe_recipe_selection(&mut self) {
        let grid = self.crafting_grid();
        let contents = self.grid_contents(grid);
        if self
            .recipe_selection()
            .is_some_and(|selection| selection.grid != contents)
        {
            self.clear_recipe_selection();
        }
    }
}

/// Each grid cell's ingredient with the recipe at the top left, or `None` when the recipe
/// needs a larger grid. Shapeless ingredients fill cells in order.
fn grid_layout(
    recipe: &RecipeHandle,
    grid: CraftingGrid,
) -> Option<Vec<Option<RecipeIngredientView>>> {
    let size = usize::from(grid.width());
    let views = recipe.ingredient_views();
    let mut cells = vec![None; size * size];
    if recipe.is_shapeless() {
        if views.len() > cells.len() {
            return None;
        }
        for (cell, view) in cells.iter_mut().zip(views) {
            *cell = view;
        }
        return Some(cells);
    }
    let (width, height) = recipe.dimensions();
    let (width, height) = (usize::from(width), usize::from(height));
    if width > size || height > size {
        return None;
    }
    for (index, view) in views.into_iter().enumerate() {
        cells[(index / width) * size + index % width] = view;
    }
    Some(cells)
}
