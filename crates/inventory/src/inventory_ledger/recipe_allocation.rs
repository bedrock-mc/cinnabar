//! Exact recipe allocation shared by availability, grid placement and auto-craft requests.

use std::collections::HashSet;

use protocol::{ItemRegistryEntry, NetworkItemStack, RecipeHandle, RecipeIngredientView};

use super::PlayerInventoryLedger;
use super::cells::Cell;
use super::registry::entry_capacity;

const _: () = assert!(protocol::MAX_RECIPE_INGREDIENTS <= u16::BITS as usize);

/// One ingredient's `(cell, amount, stack id)` sources.
pub(super) type IngredientSources = Vec<(Cell, u16, i32)>;

/// One stack-compatible variant, with its source cells in their original order.
struct Variant<'a> {
    stack: &'a NetworkItemStack,
    entry: &'a ItemRegistryEntry,
    capacity: u16,
    sources: IngredientSources,
    total: u32,
}

/// Validated source variants shared by every candidate in a recipe-book projection.
pub(crate) struct RecipeInputs<'a> {
    variants: Vec<Variant<'a>>,
}

/// At most nine ingredient cells and the concrete variants each accepts.
pub(super) struct RecipeAllocation<'a> {
    variants: &'a [Variant<'a>],
    counts: Vec<u16>,
    accepts: Vec<Vec<usize>>,
}

impl<'a> RecipeInputs<'a> {
    /// Groups usable grid and player stacks once, preserving source order and item data.
    pub(super) fn new(ledger: &'a PlayerInventoryLedger) -> Self {
        let mut variants: Vec<Variant<'a>> = Vec::new();
        for cell in ledger.recipe_source_cells() {
            let Some((stack, entry)) = ledger.auto_craft_stack(cell) else {
                continue;
            };
            let source = (cell, stack.count, stack.stack_network_id);
            match variants
                .iter_mut()
                .find(|variant| PlayerInventoryLedger::same_item(variant.stack, stack))
            {
                Some(variant) => {
                    variant.sources.push(source);
                    variant.total += u32::from(stack.count);
                }
                None => variants.push(Variant {
                    stack,
                    entry,
                    capacity: entry_capacity(entry).map_or(0, u16::from),
                    sources: vec![source],
                    total: u32::from(stack.count),
                }),
            }
        }
        Self { variants }
    }

    /// Whether the admitted sources can supply one complete craft of this candidate.
    pub(crate) fn can_craft(&self, recipe: &RecipeHandle) -> bool {
        self.allocation(&recipe.ingredient_views())
            .plan(1, false)
            .is_some()
    }

    /// Whether any admitted variant matches an ingredient, without revalidating source bytes.
    pub(crate) fn holds_any(&self, recipe: &RecipeHandle) -> bool {
        recipe
            .ingredient_views()
            .iter()
            .flatten()
            .any(|ingredient| {
                self.variants
                    .iter()
                    .any(|variant| variant.accepts(ingredient))
            })
    }

    /// Binds a recipe's ingredient rows to already validated source variants.
    pub(super) fn allocation(
        &self,
        layout: &[Option<RecipeIngredientView>],
    ) -> RecipeAllocation<'_> {
        RecipeAllocation {
            counts: layout
                .iter()
                .map(|view| view.as_ref().map_or(0, |view| u16::from(view.count)))
                .collect(),
            accepts: layout
                .iter()
                .map(|view| {
                    self.variants
                        .iter()
                        .enumerate()
                        .filter_map(|(index, variant)| {
                            view.as_ref()
                                .filter(|view| variant.accepts(view))
                                .map(|_| index)
                        })
                        .collect()
                })
                .collect(),
            variants: &self.variants,
        }
    }
}

impl Variant<'_> {
    /// Matches an ingredient against the variant's retained registry binding and metadata.
    fn accepts(&self, ingredient: &RecipeIngredientView) -> bool {
        crate::ingredient_accepts(
            ingredient,
            &self.entry.identifier,
            self.stack.metadata,
            &self.entry.item_tags,
        )
    }
}

impl RecipeAllocation<'_> {
    /// Assigns every ingredient to one variant. Only physical grid cells need a stack limit;
    /// an auto-craft may consume several stacks of that variant in one request.
    pub(super) fn plan(&self, crafts: u8, in_grid: bool) -> Option<Vec<IngredientSources>> {
        self.assign(crafts, in_grid, 0)
            .map(|assigned| self.sources(crafts, &assigned))
    }

    /// Places one craft's worth in as many cells as possible, keeping missing cells empty.
    pub(super) fn partial_plan(&self) -> Vec<IngredientSources> {
        (1..=self.counts.iter().filter(|count| **count > 0).count())
            .find_map(|missing| self.assign(1, true, missing))
            .map(|assigned| self.sources(1, &assigned))
            .unwrap_or_else(|| vec![Vec::new(); self.counts.len()])
    }

    /// Searches complete assignments before allowing the requested number of empty cells.
    fn assign(&self, crafts: u8, in_grid: bool, missing: usize) -> Option<Vec<Option<usize>>> {
        let mut search = Search::new(self, crafts, in_grid);
        search.assign(0, missing).then_some(search.assigned)
    }

    /// Draws each assigned cell's full demand from its variant, without reusing quantities.
    fn sources(&self, crafts: u8, assigned: &[Option<usize>]) -> Vec<IngredientSources> {
        let mut variants: Vec<_> = self
            .variants
            .iter()
            .map(|variant| variant.sources.clone())
            .collect();
        assigned
            .iter()
            .zip(&self.counts)
            .map(|(variant, count)| {
                let Some(variant) = variant else {
                    return Vec::new();
                };
                let mut needed = count * u16::from(crafts);
                let mut sources = Vec::new();
                for (cell, count, id) in &mut variants[*variant] {
                    let amount = needed.min(*count);
                    if amount > 0 {
                        sources.push((*cell, amount, *id));
                        *count -= amount;
                        needed -= amount;
                    }
                }
                sources
            })
            .collect()
    }
}

/// Mutable state for one exact search; each failed branch restores its quantities and choices.
struct Search<'a> {
    allocation: &'a RecipeAllocation<'a>,
    demands: Vec<u16>,
    compatible: Vec<u16>,
    visited: HashSet<SearchState>,
    remaining: Vec<u32>,
    assigned: Vec<Option<usize>>,
    order: Vec<usize>,
    #[cfg(test)]
    work_budget: Option<usize>,
}

/// A canonical capacity multiset for the remaining cells and allowed omissions.
#[derive(Hash, Eq, PartialEq)]
struct SearchState {
    cells: u16,
    missing: usize,
    capacities: Vec<(u16, u32)>,
}

impl<'a> Search<'a> {
    /// Retains each variant's eligible cells, including physical stack limits when placing.
    fn new(allocation: &'a RecipeAllocation<'a>, crafts: u8, in_grid: bool) -> Self {
        let demands: Vec<_> = allocation
            .counts
            .iter()
            .map(|count| count * u16::from(crafts))
            .collect();
        let compatible = allocation
            .variants
            .iter()
            .enumerate()
            .map(|(variant, input)| {
                allocation
                    .accepts
                    .iter()
                    .enumerate()
                    .filter(|(cell, accepts)| {
                        demands[*cell] > 0
                            && accepts.contains(&variant)
                            && (!in_grid || input.capacity >= demands[*cell])
                    })
                    .fold(0, |mask, (cell, _)| mask | (1 << cell))
            })
            .collect();
        Self {
            allocation,
            demands,
            compatible,
            visited: HashSet::new(),
            remaining: allocation
                .variants
                .iter()
                .map(|variant| variant.total)
                .collect(),
            assigned: vec![None; allocation.counts.len()],
            order: (0..allocation.counts.len()).collect(),
            #[cfg(test)]
            work_budget: None,
        }
    }

    /// Canonicalizes interchangeable variants only across cells that still need assignment.
    fn state(&self, first: usize, missing: usize) -> SearchState {
        let cells = self.order[first..]
            .iter()
            .fold(0, |mask, cell| mask | (1 << cell));
        let mut capacities: Vec<_> = self
            .compatible
            .iter()
            .zip(&self.remaining)
            .filter_map(|(mask, count)| (mask & cells != 0).then_some((mask & cells, *count)))
            .collect();
        capacities.sort_unstable();
        SearchState {
            cells,
            missing,
            capacities,
        }
    }

    /// Checks a cell's whole demand against one variant's remaining supply and grid capacity.
    fn fits(&self, cell: usize, variant: usize) -> bool {
        self.remaining[variant] >= u32::from(self.demands[cell])
            && self.compatible[variant] & (1 << cell) != 0
    }

    /// Rejects missing individual supply and insufficient total capacity before branching.
    fn remaining_capacity_fits(&self, first: usize, missing: usize) -> bool {
        let cells = &self.order[first..];
        let impossible = cells
            .iter()
            .filter(|cell| {
                self.demands[**cell] > 0
                    && !self.allocation.accepts[**cell]
                        .iter()
                        .any(|variant| self.fits(**cell, *variant))
            })
            .count();
        if impossible > missing {
            return false;
        }
        for &cell in cells {
            let demand = u32::from(self.demands[cell]);
            if demand == 0 {
                continue;
            }
            // Every counted cell consumes at least this demand from a subset of these variants.
            let shared = cells
                .iter()
                .filter(|other| {
                    u32::from(self.demands[**other]) >= demand
                        && self
                            .compatible
                            .iter()
                            .all(|mask| mask & (1 << **other) == 0 || mask & (1 << cell) != 0)
                })
                .count();
            let available: u32 = self
                .compatible
                .iter()
                .zip(&self.remaining)
                .filter(|(mask, _)| **mask & (1 << cell) != 0)
                .map(|(_, count)| count / demand)
                .sum();
            if available < shared.saturating_sub(missing) as u32 {
                return false;
            }
        }
        let mut demands: Vec<_> = cells.iter().map(|cell| self.demands[*cell]).collect();
        demands.sort_unstable_by(|a, b| b.cmp(a));
        let needed: u32 = demands.into_iter().skip(missing).map(u32::from).sum();
        let available: u32 = self
            .remaining
            .iter()
            .enumerate()
            .filter(|(variant, _)| {
                cells.iter().any(|cell| {
                    self.allocation.accepts[*cell].contains(variant) && self.fits(*cell, *variant)
                })
            })
            .map(|(_, count)| *count)
            .sum();
        available >= needed
    }

    /// Resolves the cell with the fewest whole-demand choices first, then backtracks every
    /// variant. Unrelated flexible cells wait until constrained cells have been resolved.
    fn assign(&mut self, first: usize, missing: usize) -> bool {
        #[cfg(test)]
        if let Some(budget) = &mut self.work_budget {
            *budget = budget
                .checked_sub(1)
                .expect("allocation work budget exceeded");
        }
        if first == self.demands.len() {
            return true;
        }
        if !self.remaining_capacity_fits(first, missing) {
            return false;
        }
        let state = self.state(first, missing);
        let cells = state.cells;
        if !self.visited.insert(state) {
            return false;
        }
        let next = (first..self.order.len())
            .min_by_key(|index| {
                let cell = self.order[*index];
                self.allocation.accepts[cell]
                    .iter()
                    .filter(|variant| self.fits(cell, **variant))
                    .count()
            })
            .expect("remaining cell");
        self.order.swap(first, next);
        let cell = self.order[first];
        let allocation = self.allocation;
        let demand = self.demands[cell];
        if demand == 0 {
            let complete = self.assign(first + 1, missing);
            self.order.swap(first, next);
            return complete;
        }
        let mut choices = Vec::new();
        for &variant in &allocation.accepts[cell] {
            if !self.fits(cell, variant) {
                continue;
            }
            let choice = (self.compatible[variant] & cells, self.remaining[variant]);
            if choices.contains(&choice) {
                continue;
            }
            choices.push(choice);
            self.remaining[variant] -= u32::from(demand);
            self.assigned[cell] = Some(variant);
            if self.assign(first + 1, missing) {
                return true;
            }
            self.remaining[variant] += u32::from(demand);
            self.assigned[cell] = None;
        }
        let complete = missing > 0 && self.assign(first + 1, missing - 1);
        self.order.swap(first, next);
        complete
    }
}

#[cfg(test)]
mod tests;
