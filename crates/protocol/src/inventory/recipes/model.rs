use super::budget::Permit;
use std::sync::Arc;

/// Grid cells a crafting-table recipe may address.
pub const MAX_INGREDIENTS: usize = 9;
/// Ingredient metadata that accepts any item metadata.
pub const ANY_AUX: u16 = 32767;

#[derive(Debug, PartialEq, Eq)]
pub struct Ingredient {
    /// An item identifier, or a tag when `tag` is set.
    pub(in crate::inventory) name: String,
    pub(in crate::inventory) tag: bool,
    pub(in crate::inventory) aux: u16,
    pub(in crate::inventory) count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::inventory) struct Output {
    pub(in crate::inventory) id: i32,
    pub(in crate::inventory) aux: u16,
    pub(in crate::inventory) count: u8,
    pub(in crate::inventory) block: u32,
    pub(in crate::inventory) empty_envelope: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Recipe {
    /// Zero for a shapeless recipe.
    pub(in crate::inventory) width: u8,
    pub(in crate::inventory) height: u8,
    pub(in crate::inventory) shapeless: bool,
    /// A shaped recipe that also matches its horizontal mirror.
    pub(in crate::inventory) mirror: bool,
    /// Lower values win when several recipes match one grid.
    pub(in crate::inventory) priority: i32,
    /// Row-major shaped cells, or the shapeless ingredient list.
    pub(in crate::inventory) ingredients: [Option<Ingredient>; MAX_INGREDIENTS],
    pub(in crate::inventory) output: Output,
}

impl Ingredient {
    /// The decoded item identifier or tag name, borrowed from its credited recipe.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Whether the name addresses an item tag.
    pub const fn is_tag(&self) -> bool {
        self.tag
    }
    /// The number of items consumed by one craft.
    pub const fn count(&self) -> u8 {
        self.count
    }

    pub fn accepts_metadata(&self, metadata: u32) -> bool {
        self.aux == ANY_AUX || u32::from(self.aux) == metadata
    }
}

impl Recipe {
    /// The decoded shape dimensions, zero for shapeless recipes.
    pub const fn dimensions(&self) -> (u8, u8) {
        (self.width, self.height)
    }
    /// Whether ingredient order is unconstrained.
    pub const fn is_shapeless(&self) -> bool {
        self.shapeless
    }
    /// Whether the wire recipe permits horizontal mirroring.
    pub const fn allows_mirror(&self) -> bool {
        self.mirror
    }
    /// The recipe priority carried by the wire.
    pub const fn priority(&self) -> i32 {
        self.priority
    }
    /// Borrow decoded cells without detaching their credited storage.
    pub fn ingredients(&self) -> &[Option<Ingredient>; MAX_INGREDIENTS] {
        &self.ingredients
    }
    /// The declared output, using the existing public wire view.
    pub fn output(&self) -> super::crafting::RecipeOutput {
        super::crafting::RecipeOutput {
            network_id: self.output.id,
            aux: self.output.aux,
            count: self.output.count,
            block_runtime_id: self.output.block,
            empty_envelope: self.output.empty_envelope,
        }
    }

    /// The shaped, name-only, fits-in-two-by-two domain the manual craft
    /// builder and passive observations were written for.
    pub fn is_personal_named(&self) -> bool {
        !self.shapeless
            && self.width <= 2
            && self.height <= 2
            && self
                .ingredients
                .iter()
                .flatten()
                .all(|ingredient| !ingredient.tag)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::inventory) struct Record {
    pub(in crate::inventory) id: u32,
    pub(in crate::inventory) recipe: Option<Recipe>,
}

#[derive(Debug)]
pub(in crate::inventory) struct Batch {
    pub(in crate::inventory) records: Vec<Record>,
    pub(in crate::inventory) clear: bool,
    pub(super) _permit: Permit,
}

/// Cloning shares the immutable allocation and its lifetime credit.
#[derive(Debug, Clone)]
pub struct RecipeUpdate {
    pub(in crate::inventory) batch: Option<Arc<Batch>>,
    pub(in crate::inventory) screen: Option<Arc<super::screen::ScreenRecipes>>,
}

impl RecipeUpdate {
    pub fn is_unavailable(&self) -> bool {
        self.batch.is_none()
    }
    pub fn clears_catalog(&self) -> bool {
        self.batch.as_ref().is_none_or(|b| b.clear)
    }
    pub fn record_count(&self) -> usize {
        self.batch.as_ref().map_or(0, |b| b.records.len())
    }
    pub(in crate::inventory) fn unavailable() -> Self {
        Self {
            batch: None,
            screen: None,
        }
    }
}

impl PartialEq for RecipeUpdate {
    fn eq(&self, other: &Self) -> bool {
        self.screen == other.screen
            && match (&self.batch, &other.batch) {
                (Some(a), Some(b)) => a.clear == b.clear && a.records == b.records,
                (None, None) => true,
                _ => false,
            }
    }
}
impl Eq for RecipeUpdate {}

/// A handle cannot detach or clone owned recipe storage from its charged batch.
#[derive(Debug, Clone)]
pub struct RecipeHandle {
    pub(in crate::inventory) batch: Arc<Batch>,
    pub(in crate::inventory) index: usize,
}
impl RecipeHandle {
    pub fn network_id(&self) -> u32 {
        self.batch.records[self.index].id
    }
    pub fn dimensions(&self) -> (u8, u8) {
        let recipe = self.recipe();
        (recipe.width, recipe.height)
    }
    /// Borrow the admitted recipe while this handle retains its lifetime credit.
    pub fn recipe(&self) -> &Recipe {
        self.batch.records[self.index]
            .recipe
            .as_ref()
            .expect("supported handle invariant")
    }
}
