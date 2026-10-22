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
        // Returning the grid needs every player cell known, or it could drop what would fit.
        if self.known[..self.confirmed.player_len()]
            .iter()
            .any(|known| !known)
        {
            return Err(InventoryGestureError::ResyncRequired);
        }
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
        let crafts = self.placement_crafts(&layout);
        let (plan, _) = self.placement_plan(&layout, crafts);
        for (index, transfers) in plan.into_iter().enumerate() {
            let cell = Cell::Craft(slots[index]);
            for (source, amount) in transfers {
                let Some(held) = self.view().get(source).cloned() else {
                    continue;
                };
                let amount = amount.min(held.stack.count);
                if let Some(request) = self.place_into_grid(source, &held, cell, amount)? {
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

    /// One Place request from `source` into `cell`; a player cell is named in the combined
    /// inventory.
    fn place_into_grid(
        &mut self,
        source: Cell,
        held: &Held,
        cell: Cell,
        amount: u16,
    ) -> Result<Option<i32>, InventoryGestureError> {
        let into = self.view().get(cell).cloned();
        let Some(mut built) = self.build_transfer(source, held, cell, into.as_ref(), amount)?
        else {
            return Ok(None);
        };
        if let (Cell::Inventory(_), protocol::StackRequestAction::Place { source, .. }) =
            (source, &mut built.action)
        {
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

    /// Crafts' worth to place per cell: the most for which every cell gets one item variant
    /// holding its whole share, within a stack, else one.
    fn placement_crafts(&self, layout: &[Option<RecipeIngredientView>]) -> u8 {
        let (mut low, mut high) = (0u8, u8::MAX);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if self.placement_plan(layout, middle).1 {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        low.max(1)
    }

    /// Per grid cell, the `(source, amount)` transfers that give it `crafts` of one item
    /// variant, and whether every cell got its whole share. Cells trade variants through
    /// augmenting paths, so a tag cell never keeps the only variant an exact cell accepts; a
    /// cell no variant can fill stays empty.
    fn placement_plan(
        &self,
        layout: &[Option<RecipeIngredientView>],
        crafts: u8,
    ) -> (Vec<Vec<(Cell, u16)>>, bool) {
        let mut variants: Vec<Variant> = Vec::new();
        for cell in self.recipe_source_cells() {
            let Some((stack, entry)) = self.auto_craft_stack(cell) else {
                continue;
            };
            match variants
                .iter_mut()
                .find(|variant| Self::same_item(&variant.stack, stack))
            {
                Some(variant) => {
                    variant.cells.push((cell, stack.count));
                    variant.total = variant.total.saturating_add(stack.count);
                }
                None => variants.push(Variant {
                    stack: stack.clone(),
                    capacity: entry_capacity(entry).map_or(0, u16::from),
                    cells: vec![(cell, stack.count)],
                    total: stack.count,
                }),
            }
        }
        let demands: Vec<u16> = layout
            .iter()
            .map(|ingredient| {
                ingredient.as_ref().map_or(0, |ingredient| {
                    u16::from(ingredient.count) * u16::from(crafts)
                })
            })
            .collect();
        let accepts: Vec<Vec<usize>> = layout
            .iter()
            .zip(&demands)
            .map(|(ingredient, demand)| {
                let Some(ingredient) = ingredient else {
                    return Vec::new();
                };
                (0..variants.len())
                    .filter(|index| {
                        let variant = &variants[*index];
                        *demand <= variant.capacity
                            && self.source_accepts(variant.cells[0].0, ingredient)
                    })
                    .collect()
            })
            .collect();
        let mut matching = Matching {
            demands: &demands,
            accepts: &accepts,
            remaining: variants.iter().map(|variant| variant.total).collect(),
            assigned: vec![None; layout.len()],
        };
        let mut complete = true;
        for (cell, demand) in demands.iter().enumerate() {
            if *demand != 0 {
                let mut visited = vec![false; variants.len()];
                complete &= matching.assign(cell, &mut visited);
            }
        }
        let mut used: Vec<Vec<u16>> = variants
            .iter()
            .map(|variant| vec![0; variant.cells.len()])
            .collect();
        let plan = matching
            .assigned
            .iter()
            .zip(&demands)
            .map(|(variant, demand)| {
                let Some(variant) = *variant else {
                    return Vec::new();
                };
                let mut needed = *demand;
                let mut transfers = Vec::new();
                for ((cell, count), used) in variants[variant].cells.iter().zip(&mut used[variant])
                {
                    let amount = needed.min(count - *used);
                    if amount > 0 {
                        *used += amount;
                        needed -= amount;
                        transfers.push((*cell, amount));
                    }
                }
                transfers
            })
            .collect();
        (plan, complete)
    }

    /// The item a cell shows while empty: the held item the ingredient accepts, else the first
    /// registered one.
    fn ghost_item(&self, ingredient: &RecipeIngredientView) -> Option<NetworkItemStack> {
        if let Some(cell) = self
            .recipe_source_cells()
            .into_iter()
            .find(|cell| self.source_accepts(*cell, ingredient))
        {
            let stack = &self.view().get(cell)?.stack;
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

/// One item variant among the source cells.
struct Variant {
    stack: NetworkItemStack,
    capacity: u16,
    cells: Vec<(Cell, u16)>,
    total: u16,
}

/// Assigns grid cells to variants with room for each cell's whole demand.
struct Matching<'a> {
    demands: &'a [u16],
    accepts: &'a [Vec<usize>],
    remaining: Vec<u16>,
    assigned: Vec<Option<usize>>,
}

impl Matching<'_> {
    /// Gives `cell` a variant, moving earlier cells to other variants when that frees room;
    /// leaves every assignment unchanged when it fails.
    fn assign(&mut self, cell: usize, visited: &mut [bool]) -> bool {
        let demand = self.demands[cell];
        for &variant in &self.accepts[cell] {
            if visited[variant] {
                continue;
            }
            visited[variant] = true;
            if self.remaining[variant] >= demand {
                self.take(cell, variant);
                return true;
            }
            for other in 0..self.assigned.len() {
                if other == cell || self.assigned[other] != Some(variant) {
                    continue;
                }
                self.assigned[other] = None;
                self.remaining[variant] += self.demands[other];
                if self.remaining[variant] >= demand && self.assign(other, visited) {
                    self.take(cell, variant);
                    return true;
                }
                self.take(other, variant);
            }
        }
        false
    }

    fn take(&mut self, cell: usize, variant: usize) {
        self.assigned[cell] = Some(variant);
        self.remaining[variant] -= self.demands[cell];
    }
}
