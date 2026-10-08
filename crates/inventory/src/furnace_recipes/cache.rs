use std::{collections::BTreeMap, sync::Arc};

use protocol::{ItemRegistryEntry, RecipeCatalog, ScreenRecipe, ScreenRecipeKind};

use crate::InventorySession;

#[derive(Debug, Default)]
pub(crate) struct Cache {
    inputs: Option<Inputs>,
    all: Arc<[u32]>,
    supplied: Arc<[u32]>,
}

#[derive(Debug)]
struct Inputs {
    catalog: Option<(usize, u64)>,
    kind: Option<ScreenRecipeKind>,
    registry: Option<Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    stacks: [Option<(i32, u32)>; protocol::PLAYER_INVENTORY_SLOTS as usize + 1],
}

impl Inputs {
    fn same(&self, other: &Self) -> bool {
        self.catalog == other.catalog
            && self.kind == other.kind
            && self.stacks == other.stacks
            && match (&self.registry, &other.registry) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

/// A shared result projection borrowed against its current recipe catalog.
pub struct FurnaceRecipes<'a> {
    catalog: Option<&'a RecipeCatalog>,
    ids: Arc<[u32]>,
}

impl<'a> FurnaceRecipes<'a> {
    pub fn iter(&self) -> impl Iterator<Item = &'a ScreenRecipe> + '_ {
        self.ids
            .iter()
            .filter_map(|id| self.catalog?.screen_recipe(*id))
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Immutable result identities, retained for presentation cache invalidation.
    pub fn shared_ids(&self) -> &Arc<[u32]> {
        &self.ids
    }
}

impl std::ops::Index<usize> for FurnaceRecipes<'_> {
    type Output = ScreenRecipe;
    fn index(&self, index: usize) -> &Self::Output {
        self.catalog
            .unwrap()
            .screen_recipe(self.ids[index])
            .unwrap()
    }
}

pub struct FurnaceRecipeIter<'a> {
    catalog: Option<&'a RecipeCatalog>,
    ids: Arc<[u32]>,
    position: usize,
}

impl<'a> Iterator for FurnaceRecipeIter<'a> {
    type Item = &'a ScreenRecipe;
    fn next(&mut self) -> Option<Self::Item> {
        let id = *self.ids.get(self.position)?;
        self.position += 1;
        self.catalog?.screen_recipe(id)
    }
}

impl<'a> IntoIterator for FurnaceRecipes<'a> {
    type Item = &'a ScreenRecipe;
    type IntoIter = FurnaceRecipeIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        FurnaceRecipeIter {
            catalog: self.catalog,
            ids: self.ids,
            position: 0,
        }
    }
}

pub(super) fn project(inventory: &InventorySession, filtering: bool) -> FurnaceRecipes<'_> {
    let ledger = inventory.ledger();
    let catalog = inventory.screen_catalog();
    let inputs = Inputs {
        catalog: catalog.map(|catalog| (std::ptr::from_ref(catalog) as usize, catalog.revision())),
        kind: ledger.window_kind().and_then(super::kind),
        registry: ledger.item_registry_snapshot().cloned(),
        stacks: std::array::from_fn(|index| {
            let stack = if index == usize::from(protocol::PLAYER_INVENTORY_SLOTS) {
                ledger.storage_stack(0)
            } else {
                ledger.displayed_stack(index as u8)
            }?;
            Some((stack.network_id, stack.metadata))
        }),
    };
    let mut cache = inventory.furnace_cache.lock().unwrap();
    if !cache
        .inputs
        .as_ref()
        .is_some_and(|previous| previous.same(&inputs))
    {
        let mut positions = std::collections::HashMap::new();
        let mut listed: Vec<&ScreenRecipe> = Vec::new();
        if let (Some(catalog), Some(kind)) = (catalog, inputs.kind) {
            for recipe in catalog
                .screen_recipes(kind)
                .filter(|recipe| recipe.ingredients.len() == 1 && recipe.output.is_some())
            {
                let output = recipe.output.unwrap();
                let key = (output.network_id, output.aux, output.block_runtime_id);
                if let Some(&position) = positions.get(&key) {
                    if ledger.can_supply_furnace_recipe(recipe)
                        && !ledger.can_supply_furnace_recipe(listed[position])
                    {
                        listed[position] = recipe;
                    }
                } else {
                    positions.insert(key, listed.len());
                    listed.push(recipe);
                }
            }
        }
        cache.all = listed.iter().map(|recipe| recipe.id).collect();
        cache.supplied = listed
            .into_iter()
            .filter(|recipe| ledger.can_supply_furnace_recipe(recipe))
            .map(|recipe| recipe.id)
            .collect();
        cache.inputs = Some(inputs);
    }
    FurnaceRecipes {
        catalog,
        ids: Arc::clone(if filtering {
            &cache.supplied
        } else {
            &cache.all
        }),
    }
}
