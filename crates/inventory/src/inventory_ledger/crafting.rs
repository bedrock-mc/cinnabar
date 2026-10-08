//! Crafting-table requests over the personal 2x2 or workbench 3x3 grid,
//! ported from the owner's proxy prediction: every ingredient must claim a
//! distinct grid cell or nothing is predicted.

use std::sync::Arc;

use crate::CraftGridItem;
use protocol::{CraftResult, NetworkItemStack, RecipeHandle, StackRequestAction};
use sha2::{Digest, Sha256};

use super::cells::{Cell, FIRST_CRAFT_SLOT, Held};
use super::gesture::Submission;
use super::overlay::DeltaGroup;
use super::registry::{entry_capacity, plain_stack};
use super::{InventoryGestureError, PlayerInventoryLedger, WORKBENCH_WINDOW_TYPE, helpers};

/// Which crafting grid the open screen offers.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CraftingGrid {
    /// The personal 2x2 grid, UI slots 28..=31.
    Personal,
    /// The workbench 3x3 grid, UI slots 32..=40.
    Workbench,
}

impl CraftingGrid {
    #[must_use]
    pub const fn width(self) -> u8 {
        match self {
            Self::Personal => 2,
            Self::Workbench => 3,
        }
    }

    /// The UI inventory slot of each grid cell in row-major order.
    pub fn slots(self) -> impl Iterator<Item = u8> {
        let (first, count) = match self {
            Self::Personal => (FIRST_CRAFT_SLOT, 4),
            Self::Workbench => (FIRST_CRAFT_SLOT + 4, 9),
        };
        first..first + count
    }
}

/// Where a creative take lands.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CreativeDestination {
    Cursor,
    /// An empty player cell.
    Player(u8),
    /// A drop from the catalog creates one item, or its whole stack with Control held.
    Drop {
        whole_stack: bool,
    },
}

/// Where crafted output lands.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CraftSink {
    Cursor,
    /// An empty hotbar or inventory cell.
    Player(u8),
    /// Every player cell that can take the stack, like a shift-click.
    Inventory,
}

/// `(cell, amount, occupied destination stack id and count)`.
type OutputPlacement = (Cell, u16, Option<(i32, u16)>);

/// One presented grid cell resolved through the session item registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftGridCell {
    pub identifier: Arc<str>,
    pub metadata: u32,
    pub count: u16,
    pub plain: bool,
    pub tags: Arc<[Arc<str>]>,
}

impl CraftGridCell {
    #[must_use]
    pub fn item(&self) -> CraftGridItem<'_> {
        CraftGridItem {
            identifier: &self.identifier,
            metadata: self.metadata,
            count: self.count,
            plain: self.plain,
            tags: &self.tags,
        }
    }
}

impl PlayerInventoryLedger {
    #[must_use]
    pub fn crafting_grid(&self) -> CraftingGrid {
        if self
            .storage
            .as_ref()
            .is_some_and(|storage| storage.window_type == WORKBENCH_WINDOW_TYPE)
        {
            CraftingGrid::Workbench
        } else {
            CraftingGrid::Personal
        }
    }

    /// The presented grid, or `None` while any occupied cell's item identity
    /// is unknown to the session registry.
    #[must_use]
    pub fn crafting_grid_cells(&self) -> Option<Vec<Option<CraftGridCell>>> {
        self.crafting_grid()
            .slots()
            .map(|slot| self.grid_cell(Cell::Craft(slot)))
            .collect()
    }

    /// The open crafter's nine slots as a 3x3 grid, or `None` while an
    /// occupant's identity is unknown.
    #[must_use]
    pub fn crafter_grid_cells(&self) -> Option<Vec<Option<CraftGridCell>>> {
        (0..9)
            .map(|slot| self.grid_cell(Cell::Storage(slot)))
            .collect()
    }

    fn grid_cell(&self, cell: Cell) -> Option<Option<CraftGridCell>> {
        let Some(held) = self.view().get(cell) else {
            return Some(None);
        };
        let entry = self.negotiated_item_entry(held.stack.network_id)?;
        Some(Some(CraftGridCell {
            identifier: Arc::clone(&entry.identifier),
            metadata: held.stack.metadata,
            count: held.stack.count,
            plain: plain_stack(&held.stack),
            tags: Arc::clone(&entry.item_tags),
        }))
    }

    /// Crafts `recipe` `crafts` times into an empty cursor as one request.
    pub fn begin_craft(
        &mut self,
        recipe: &RecipeHandle,
        crafts: u8,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_craft_into(recipe, crafts, CraftSink::Cursor)
    }

    /// Crafts as many times as the grid and the inventory allow, placing the
    /// output like a shift-click. Halves the batch until everything fits.
    pub fn begin_craft_all(&mut self, recipe: &RecipeHandle) -> Result<i32, InventoryGestureError> {
        let mut crafts = self.max_crafts(recipe)?;
        loop {
            match self.begin_craft_into(recipe, crafts, CraftSink::Inventory) {
                Err(InventoryGestureError::InvalidRequest) if crafts > 1 => crafts /= 2,
                result => return result,
            }
        }
    }

    /// How many times the presented grid can craft `recipe` at once.
    fn max_crafts(&self, recipe: &RecipeHandle) -> Result<u8, InventoryGestureError> {
        let cells = self
            .crafting_grid_cells()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let (mut low, mut high) = (0u8, u8::MAX);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if ingredient_assignment(recipe, &cells, middle).is_some() {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        if low == 0 {
            Err(InventoryGestureError::InvalidRequest)
        } else {
            Ok(low)
        }
    }

    /// Crafts `recipe` once per `crafts` into `sink` as one request:
    /// CraftRecipe, CraftResultsDeprecated, one Consume per claimed cell, and
    /// transfers out of created output named by this request's id.
    pub fn begin_craft_into(
        &mut self,
        recipe: &RecipeHandle,
        crafts: u8,
        sink: CraftSink,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor, Cell::CreatedOutput])?;
        let destination = match sink {
            CraftSink::Cursor => {
                if self.view().get(Cell::Cursor).is_some() {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                Some(Cell::Cursor)
            }
            CraftSink::Player(slot) => {
                let cell = Cell::Inventory(slot);
                self.check_target(cell)?;
                self.check_surfaces([cell])?;
                if self.view().get(cell).is_some() {
                    return Err(InventoryGestureError::InvalidRequest);
                }
                Some(cell)
            }
            CraftSink::Inventory => None,
        };
        if crafts == 0 {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let cells = self
            .crafting_grid_cells()
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let slots: Vec<u8> = self.crafting_grid().slots().collect();
        let assignment = ingredient_assignment(recipe, &cells, crafts)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let mut consumed = Vec::new();
        for ((_, per_craft), cell) in recipe.ingredient_counts().enumerate().zip(assignment) {
            let held = self
                .view()
                .get(Cell::Craft(slots[cell]))
                .expect("claimed cells are occupied");
            if self.awaiting_identity(held) {
                return Err(InventoryGestureError::AwaitingIdentity);
            }
            let amount = u8::try_from(u16::from(per_craft) * u16::from(crafts))
                .map_err(|_| InventoryGestureError::InvalidRequest)?;
            consumed.push((
                Cell::Craft(slots[cell]),
                amount,
                held.stack.stack_network_id,
            ));
        }
        let output = recipe.output();
        let entry = self
            .negotiated_item_entry(output.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let capacity = entry_capacity(entry).ok_or(InventoryGestureError::InvalidRequest)?;
        let total = u8::try_from(u16::from(output.count) * u16::from(crafts))
            .ok()
            .filter(|total| destination.is_none() || *total <= capacity)
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
        let mut actions = vec![
            StackRequestAction::CraftRecipe {
                recipe_network_id: recipe.network_id(),
                crafts,
            },
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
        for (cell, amount, id) in consumed {
            actions.push(StackRequestAction::Consume {
                amount,
                source: helpers::request_slot(cell, id, None)?,
            });
            groups.push(DeltaGroup::Shrink {
                source: cell,
                amount: u16::from(amount),
                source_id: id,
            });
        }
        let output_slot = helpers::request_slot(Cell::CreatedOutput, request_id, None)?;
        groups.push(DeltaGroup::Set {
            cell: Cell::CreatedOutput,
            held: Held {
                stack: created.clone(),
                overlay: None,
            },
        });
        let mut registry_bound_merge = false;
        match destination {
            Some(cell) => {
                let destination_slot = helpers::request_slot(cell, 0, None)?;
                actions.push(match sink {
                    CraftSink::Cursor => StackRequestAction::Take {
                        amount: total,
                        source: output_slot,
                        destination: destination_slot,
                    },
                    _ => StackRequestAction::Place {
                        amount: total,
                        source: output_slot,
                        destination: destination_slot,
                    },
                });
                groups.push(DeltaGroup::Transfer {
                    source: Cell::CreatedOutput,
                    destination: cell,
                    amount: u16::from(total),
                    source_id: request_id,
                    destination_id: None,
                    capacity: None,
                });
            }
            None => {
                let plan = self
                    .spread_plan(&created, capacity)
                    .ok_or(InventoryGestureError::InvalidRequest)?;
                for (cell, amount, into) in plan {
                    registry_bound_merge |= into.is_some();
                    actions.push(StackRequestAction::Place {
                        amount: u8::try_from(amount)
                            .map_err(|_| InventoryGestureError::InvalidRequest)?,
                        source: output_slot,
                        destination: helpers::request_slot(
                            cell,
                            into.map_or(0, |(id, _)| id),
                            None,
                        )?,
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
            }
        }
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            // The crafted stack must settle under a real server id.
            requires_distinct_stack_ids: true,
            registry_bound_merge,
        })
    }

    /// Splits `created` over player cells: compatible partial stacks first,
    /// then empty cells, both in hotbar-first player slot order.
    /// Each entry is `(cell, amount, occupied destination
    /// stack id and count)`; `None` when the inventory cannot hold it all.
    fn spread_plan(
        &self,
        created: &NetworkItemStack,
        capacity: u8,
    ) -> Option<Vec<OutputPlacement>> {
        let capacity = u16::from(capacity);
        let mut remaining = created.count;
        let mut plan = Vec::new();
        let cells: Vec<Cell> = (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter(|slot| self.known[usize::from(*slot)])
            .map(Cell::Inventory)
            .collect();
        for cell in &cells {
            let Some(into) = self.view().get(*cell) else {
                continue;
            };
            if remaining == 0 {
                break;
            }
            let same = into.stack.network_id == created.network_id
                && into.stack.metadata == created.metadata
                && into.stack.block_runtime_id == created.block_runtime_id
                && plain_stack(&into.stack)
                && plain_stack(created);
            if !same || self.awaiting_identity(into) {
                continue;
            }
            let amount = remaining.min(capacity.saturating_sub(into.stack.count));
            if amount > 0 {
                remaining -= amount;
                plan.push((
                    *cell,
                    amount,
                    Some((into.stack.stack_network_id, into.stack.count)),
                ));
            }
        }
        for cell in &cells {
            if remaining == 0 {
                break;
            }
            if self.view().get(*cell).is_none() {
                let amount = remaining.min(capacity);
                remaining -= amount;
                plan.push((*cell, amount, None));
            }
        }
        (remaining == 0).then_some(plan)
    }

    /// The server's current creative catalog.
    #[must_use]
    pub fn creative_catalog(&self) -> Option<&protocol::CreativeContentEvent> {
        self.creative.as_ref()
    }

    /// Creates a catalog entry, declares its prototype, then transfers or drops
    /// the requested amount from this request's created output.
    pub fn begin_creative_take(
        &mut self,
        creative_network_id: u32,
        destination: CreativeDestination,
    ) -> Result<i32, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        let item = self
            .creative
            .as_ref()
            .and_then(|catalog| catalog.item(creative_network_id))
            .ok_or(InventoryGestureError::InvalidRequest)?;
        // The client takes a full stack even though entries advertise one.
        let entry = self
            .negotiated_item_entry(item.stack.network_id)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let full = entry_capacity(entry).ok_or(InventoryGestureError::InvalidRequest)?;
        let amount = if destination == (CreativeDestination::Drop { whole_stack: false }) {
            1
        } else {
            full
        };
        // Vanilla's creative create-item scope
        // declares the selected prototype before creating the full transfer.
        let result = CraftResult {
            identifier: Arc::clone(&entry.identifier),
            aux: i32::from_ne_bytes(item.stack.metadata.to_ne_bytes()),
            count: item.stack.count,
            block_runtime_id: u32::from_ne_bytes(item.stack.block_runtime_id.to_ne_bytes()),
            user_data: Arc::clone(&item.stack.extra_data),
        };
        let target = match destination {
            CreativeDestination::Cursor => Some(Cell::Cursor),
            CreativeDestination::Player(slot) => {
                if !self.known.get(usize::from(slot)).copied().unwrap_or(false) {
                    return Err(InventoryGestureError::UnknownSlot(slot));
                }
                Some(Cell::Inventory(slot))
            }
            CreativeDestination::Drop { .. } => None,
        };
        self.check_surfaces(std::iter::once(Cell::CreatedOutput).chain(target))?;
        if let Some(target) = target
            && self.view().get(target).is_some()
        {
            return Err(InventoryGestureError::InvalidRequest);
        }
        let request_id = self.peek_request_id()?;
        let mut stack = item.stack.clone();
        stack.count = u16::from(amount);
        stack.stack_network_id = request_id;
        let source = helpers::request_slot(Cell::CreatedOutput, request_id, None)?;
        let transfer = match destination {
            CreativeDestination::Cursor => StackRequestAction::Take {
                amount,
                source,
                destination: helpers::request_slot(Cell::Cursor, 0, None)?,
            },
            CreativeDestination::Player(slot) => StackRequestAction::Place {
                amount,
                source,
                destination: helpers::request_slot(Cell::Inventory(slot), 0, None)?,
            },
            CreativeDestination::Drop { .. } => StackRequestAction::Drop {
                amount,
                source,
                randomly: false,
            },
        };
        self.submit(Submission {
            actions: vec![
                StackRequestAction::CraftCreative {
                    creative_item_network_id: creative_network_id,
                    crafts: 1,
                },
                StackRequestAction::CraftResultsDeprecated {
                    results: Arc::from([result]),
                    crafts: 1,
                },
                transfer,
            ],
            groups: vec![
                DeltaGroup::Set {
                    cell: Cell::CreatedOutput,
                    held: Held {
                        stack,
                        overlay: None,
                    },
                },
                match target {
                    Some(target) => DeltaGroup::Transfer {
                        source: Cell::CreatedOutput,
                        destination: target,
                        amount: u16::from(amount),
                        source_id: request_id,
                        destination_id: None,
                        capacity: None,
                    },
                    None => DeltaGroup::Shrink {
                        source: Cell::CreatedOutput,
                        amount: u16::from(amount),
                        source_id: request_id,
                    },
                },
            ],
            personal_generation,
            requires_distinct_stack_ids: true,
            registry_bound_merge: false,
        })
    }
}

/// Finds a complete ingredient assignment, allowing broad matches to move for specific ones.
fn ingredient_assignment(
    recipe: &RecipeHandle,
    cells: &[Option<super::CraftGridCell>],
    crafts: u8,
) -> Option<Vec<usize>> {
    let matcher = IngredientMatcher {
        recipe,
        cells,
        crafts,
        counts: recipe.ingredient_counts().collect(),
    };
    let mut occupied = vec![None; cells.len()];
    for ingredient in 0..matcher.counts.len() {
        if !matcher.assign(ingredient, &mut occupied, &mut vec![false; cells.len()]) {
            return None;
        }
    }
    let mut assignment = vec![0; matcher.counts.len()];
    for (cell, ingredient) in occupied.into_iter().enumerate() {
        if let Some(ingredient) = ingredient {
            assignment[ingredient] = cell;
        }
    }
    Some(assignment)
}

struct IngredientMatcher<'a> {
    recipe: &'a RecipeHandle,
    cells: &'a [Option<super::CraftGridCell>],
    crafts: u8,
    counts: Vec<u8>,
}
impl IngredientMatcher<'_> {
    /// Augments the matching through assigned cells, visiting each cell at most once.
    fn assign(&self, ingredient: usize, occupied: &mut [Option<usize>], seen: &mut [bool]) -> bool {
        if u16::from(self.counts[ingredient]) * u16::from(self.crafts) > u16::from(u8::MAX) {
            return false;
        }
        // Keep the ordinary first-free assignment; move earlier ingredients only
        // when no unclaimed cell can satisfy this ingredient.
        for move_assigned in [false, true] {
            for cell in 0..self.cells.len() {
                if seen[cell]
                    || occupied[cell].is_some() != move_assigned
                    || !self.cells[cell].as_ref().is_some_and(|grid| {
                        crate::recipe_ingredient_accepts(
                            self.recipe,
                            ingredient,
                            &grid.item(),
                            self.crafts,
                        )
                    })
                {
                    continue;
                }
                seen[cell] = true;
                if occupied[cell].is_none_or(|previous| self.assign(previous, occupied, seen)) {
                    occupied[cell] = Some(ingredient);
                    return true;
                }
            }
        }
        false
    }
}
