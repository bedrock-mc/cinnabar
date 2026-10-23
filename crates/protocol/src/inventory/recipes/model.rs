use super::budget::Permit;
use std::sync::Arc;

/// Grid cells a crafting-table recipe may address.
pub const MAX_INGREDIENTS: usize = 9;
/// Ingredient metadata that accepts any item metadata.
pub const ANY_AUX: u16 = protocol_ids::ITEM_AUXILIARY_WILDCARD;

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
    /// Results after the first, such as the buckets a cake returns.
    pub(in crate::inventory) extra_outputs: Box<[Output]>,
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

    /// Results after [`Self::output`], in wire order.
    pub fn extra_outputs(
        &self,
    ) -> impl ExactSizeIterator<Item = super::crafting::RecipeOutput> + '_ {
        self.extra_outputs
            .iter()
            .map(|output| super::crafting::RecipeOutput {
                network_id: output.id,
                aux: output.aux,
                count: output.count,
                block_runtime_id: output.block,
                empty_envelope: output.empty_envelope,
            })
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
    pub(super) identity_index: Option<usize>,
}

/// Decoded crafting and station storage share one lifetime credit.
#[derive(Debug)]
pub(in crate::inventory) struct Batch {
    pub(in crate::inventory) records: Vec<Record>,
    pub(super) screen: super::screen::ScreenRecipes,
    pub(super) brewing: Vec<super::brewing::BrewingMix>,
    pub(super) identities: Vec<super::identity::RecipeIdentity>,
    pub(in crate::inventory) clear: bool,
    pub(super) _permit: Permit,
}

/// Cloning shares the immutable allocation and its lifetime credit.
#[derive(Debug, Clone)]
pub struct RecipeUpdate {
    pub(in crate::inventory) batch: Option<Arc<Batch>>,
}

impl RecipeUpdate {
    /// Borrows all mix records in wire order while this update retains their allocation credit.
    pub fn brewing_mixes(&self) -> Option<&[super::brewing::BrewingMix]> {
        Some(&self.batch.as_ref()?.brewing)
    }

    pub fn is_unavailable(&self) -> bool {
        self.batch.is_none()
    }
    pub fn clears_catalog(&self) -> bool {
        self.batch.as_ref().is_none_or(|b| b.clear)
    }
    /// Number of station recipes explicitly declined by this update.
    pub fn skipped_screen_recipes(&self) -> usize {
        self.batch.as_ref().map_or(0, |batch| batch.screen.skipped)
    }
    pub fn record_count(&self) -> usize {
        self.batch.as_ref().map_or(0, |b| b.records.len())
    }
    pub(in crate::inventory) fn unavailable() -> Self {
        Self { batch: None }
    }
}

impl PartialEq for RecipeUpdate {
    fn eq(&self, other: &Self) -> bool {
        match (&self.batch, &other.batch) {
            (Some(a), Some(b)) => {
                a.clear == b.clear
                    && a.records == b.records
                    && a.screen == b.screen
                    && a.brewing == b.brewing
                    && a.identities == b.identities
            }
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
    /// Reads the exact named metadata parsed with this numeric record, including after ID sorting.
    pub fn identity(&self) -> Option<&super::identity::RecipeIdentity> {
        self.batch.records[self.index]
            .identity_index
            .and_then(|index| self.batch.identities.get(index))
    }

    /// Owners of this handle's decoded batch, so tests can prove a cache released it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn batch_owners(&self) -> usize {
        Arc::strong_count(&self.batch)
    }
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
