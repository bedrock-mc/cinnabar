//! The grouped crafting-book listing, retained while every input its discovery and material
//! checks read stays the same. Screens read the book several times a frame, and recomputing
//! each recipe's availability against the inventory every time stalls large catalogs.
//! The listing names recipes by id only, so it never keeps a replaced catalog's recipes alive.

use std::{collections::BTreeMap, sync::Arc};

use protocol::{ItemRegistryEntry, WindowKind};

use super::BookGroup;
use crate::InventorySession;
use crate::inventory_ledger::AutoCraftMaterials;

#[derive(Debug, Default)]
pub(crate) struct Cache {
    inputs: Option<Inputs>,
    listing: Arc<Listing>,
}

/// Grouped outputs in catalog order, and their positions with craftable outputs first.
#[derive(Debug, Default)]
pub(super) struct Listing {
    pub(super) groups: Vec<ListedGroup>,
    pub(super) craftable_first: Vec<usize>,
}

/// One grouped output: its preferred candidate's recipe id and what its variants share.
#[derive(Debug, Clone, Copy)]
pub(super) struct ListedGroup {
    pub(super) recipe: u32,
    pub(super) craftable: bool,
    pub(super) held: bool,
    /// Discovery has not unlocked the preferred candidate.
    pub(super) locked: bool,
}

#[derive(Debug)]
struct Inputs {
    catalog: Option<(usize, u64)>,
    small: bool,
    registry: Option<Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    materials: AutoCraftMaterials,
    discovery: crate::crafting_authority::BookDiscoveryKey,
}

impl Inputs {
    fn same(&self, other: &Self) -> bool {
        self.catalog == other.catalog
            && self.small == other.small
            && self.materials == other.materials
            && self.discovery == other.discovery
            && match (&self.registry, &other.registry) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

/// The current listing, rebuilt only after a catalog, discovery, registry or material change.
pub(super) fn listing(inventory: &InventorySession) -> Arc<Listing> {
    let ledger = inventory.ledger();
    let inputs = Inputs {
        catalog: inventory
            .screen_catalog()
            .map(|catalog| (std::ptr::from_ref(catalog) as usize, catalog.revision())),
        small: ledger.window_kind() != Some(WindowKind::Workbench),
        registry: ledger.item_registry_snapshot().cloned(),
        materials: ledger.auto_craft_materials(),
        discovery: inventory.crafting_authority.book_discovery_key(),
    };
    let mut cache = inventory.book_cache.lock().unwrap();
    if !cache
        .inputs
        .as_ref()
        .is_some_and(|previous| previous.same(&inputs))
    {
        cache.listing = Arc::new(build(inventory, inputs.small));
        cache.inputs = Some(inputs);
    }
    Arc::clone(&cache.listing)
}

/// Groups every fitting crafting recipe by output, ranking each group's preferred candidate.
fn build(inventory: &InventorySession, small: bool) -> Listing {
    let Some(catalog) = inventory.screen_catalog() else {
        return Listing::default();
    };
    let ledger = inventory.ledger();
    let inputs = ledger.auto_craft_inputs();
    let mut groups: Vec<BookGroup> = Vec::new();
    let mut outputs: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
    for recipe in catalog.crafting_recipes().filter(|recipe| {
        let (width, height) = recipe.dimensions();
        !small
            || if recipe.is_shapeless() {
                recipe.ingredient_views().len() <= 4
            } else {
                width <= 2 && height <= 2
            }
    }) {
        let unlocked = inventory.crafting_authority.book_recipe_unlocked(recipe);
        let craftable = inputs.can_craft(recipe);
        let held = craftable || inputs.holds_any(recipe);
        let candidate = BookGroup::new(recipe, unlocked, craftable, held);
        let output = recipe.output();
        let key = (output.network_id, output.aux, output.block_runtime_id);
        if let Some(&index) = outputs.get(&key) {
            groups[index].include(candidate);
        } else {
            outputs.insert(key, groups.len());
            groups.push(candidate);
        }
    }
    let groups: Vec<ListedGroup> = groups
        .into_iter()
        .map(|group| ListedGroup {
            recipe: group.entry.recipe.network_id(),
            craftable: group.entry.craftable,
            held: group.held,
            locked: group.rank.0,
        })
        .collect();
    let mut craftable_first: Vec<usize> = (0..groups.len()).collect();
    craftable_first.sort_by_key(|index| !groups[*index].craftable);
    Listing {
        groups,
        craftable_first,
    }
}
