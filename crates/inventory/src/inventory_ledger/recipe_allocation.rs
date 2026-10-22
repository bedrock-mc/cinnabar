//! Exact recipe allocation shared by availability, grid placement and auto-craft requests.

use protocol::{NetworkItemStack, RecipeIngredientView};

use super::PlayerInventoryLedger;
use super::auto_craft::IngredientSources;
use super::registry::entry_capacity;

/// One stack-compatible variant, with its source cells in their original order.
struct Variant {
    stack: NetworkItemStack,
    capacity: u16,
    sources: IngredientSources,
    total: u32,
}

/// At most nine ingredient cells and the concrete variants each accepts.
pub(super) struct RecipeAllocation {
    variants: Vec<Variant>,
    counts: Vec<u16>,
    accepts: Vec<Vec<usize>>,
}

impl PlayerInventoryLedger {
    /// Groups usable grid and player stacks once, preserving source order and item data.
    pub(super) fn recipe_allocation(
        &self,
        layout: &[Option<RecipeIngredientView>],
    ) -> RecipeAllocation {
        let mut variants: Vec<Variant> = Vec::new();
        for cell in self.recipe_source_cells() {
            let Some((stack, entry)) = self.auto_craft_stack(cell) else {
                continue;
            };
            let source = (cell, stack.count, stack.stack_network_id);
            match variants
                .iter_mut()
                .find(|variant| Self::same_item(&variant.stack, stack))
            {
                Some(variant) => {
                    variant.sources.push(source);
                    variant.total += u32::from(stack.count);
                }
                None => variants.push(Variant {
                    stack: stack.clone(),
                    capacity: entry_capacity(entry).map_or(0, u16::from),
                    sources: vec![source],
                    total: u32::from(stack.count),
                }),
            }
        }
        RecipeAllocation {
            counts: layout
                .iter()
                .map(|view| view.as_ref().map_or(0, |view| u16::from(view.count)))
                .collect(),
            accepts: layout
                .iter()
                .map(|view| {
                    variants
                        .iter()
                        .enumerate()
                        .filter_map(|(index, variant)| {
                            view.as_ref()
                                .filter(|view| self.source_accepts(variant.sources[0].0, view))
                                .map(|_| index)
                        })
                        .collect()
                })
                .collect(),
            variants,
        }
    }
}

impl RecipeAllocation {
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
        let mut search = Search {
            allocation: self,
            demands: self
                .counts
                .iter()
                .map(|count| count * u16::from(crafts))
                .collect(),
            in_grid,
            remaining: self.variants.iter().map(|variant| variant.total).collect(),
            assigned: vec![None; self.counts.len()],
        };
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
    allocation: &'a RecipeAllocation,
    demands: Vec<u16>,
    in_grid: bool,
    remaining: Vec<u32>,
    assigned: Vec<Option<usize>>,
}

impl Search<'_> {
    /// Checks a cell's whole demand against one variant's remaining supply and grid capacity.
    fn fits(&self, cell: usize, variant: usize) -> bool {
        self.remaining[variant] >= u32::from(self.demands[cell])
            && (!self.in_grid || self.allocation.variants[variant].capacity >= self.demands[cell])
    }

    /// Tries every variant, pruning branches with insufficient capacity for remaining cells.
    fn assign(&mut self, cell: usize, missing: usize) -> bool {
        if cell == self.demands.len() {
            return true;
        }
        let allocation = self.allocation;
        let impossible = (cell..self.demands.len())
            .filter(|cell| {
                self.demands[*cell] != 0
                    && !allocation.accepts[*cell]
                        .iter()
                        .any(|variant| self.fits(*cell, *variant))
            })
            .count();
        if impossible > missing {
            return false;
        }
        if missing == 0 {
            let needed: u32 = self.demands[cell..].iter().copied().map(u32::from).sum();
            let available: u32 = self
                .remaining
                .iter()
                .enumerate()
                .filter(|(variant, _)| {
                    allocation.accepts[cell..]
                        .iter()
                        .any(|accepts| accepts.contains(variant))
                })
                .map(|(_, count)| *count)
                .sum();
            if available < needed {
                return false;
            }
        }
        let demand = self.demands[cell];
        if demand == 0 {
            return self.assign(cell + 1, missing);
        }
        for &variant in &allocation.accepts[cell] {
            if !self.fits(cell, variant) {
                continue;
            }
            self.remaining[variant] -= u32::from(demand);
            self.assigned[cell] = Some(variant);
            if self.assign(cell + 1, missing) {
                return true;
            }
            self.remaining[variant] += u32::from(demand);
            self.assigned[cell] = None;
        }
        missing > 0 && self.assign(cell + 1, missing - 1)
    }
}
