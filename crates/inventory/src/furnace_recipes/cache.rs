use std::{collections::BTreeMap, sync::Arc};

use protocol::{ItemRegistryEntry, RecipeCatalog, ScreenRecipe, ScreenRecipeKind};

use crate::InventorySession;

#[derive(Debug, Default)]
pub(crate) struct Cache {
    inputs: Option<Inputs>,
    all: Arc<[usize]>,
    supplied: Arc<[usize]>,
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
    indices: Arc<[usize]>,
}

impl<'a> FurnaceRecipes<'a> {
    pub fn iter(&self) -> impl Iterator<Item = &'a ScreenRecipe> + '_ {
        self.indices
            .iter()
            .filter_map(|index| self.catalog?.screen_recipe_entries().get(*index))
    }

    pub fn len(&self) -> usize {
        self.indices.len()
    }
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Immutable catalog positions, retained for presentation cache invalidation.
    pub fn shared_indices(&self) -> &Arc<[usize]> {
        &self.indices
    }
}

impl std::ops::Index<usize> for FurnaceRecipes<'_> {
    type Output = ScreenRecipe;
    fn index(&self, index: usize) -> &Self::Output {
        self.catalog
            .unwrap()
            .screen_recipe_entries()
            .get(self.indices[index])
            .unwrap()
    }
}

pub struct FurnaceRecipeIter<'a> {
    catalog: Option<&'a RecipeCatalog>,
    indices: Arc<[usize]>,
    position: usize,
}

impl<'a> Iterator for FurnaceRecipeIter<'a> {
    type Item = &'a ScreenRecipe;
    fn next(&mut self) -> Option<Self::Item> {
        let index = *self.indices.get(self.position)?;
        self.position += 1;
        self.catalog?.screen_recipe_entries().get(index)
    }
}

impl<'a> IntoIterator for FurnaceRecipes<'a> {
    type Item = &'a ScreenRecipe;
    type IntoIter = FurnaceRecipeIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        FurnaceRecipeIter {
            catalog: self.catalog,
            indices: self.indices,
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
        let mut positions: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
        let mut listed: Vec<(usize, &ScreenRecipe)> = Vec::new();
        if let (Some(catalog), Some(kind)) = (catalog, inputs.kind) {
            for (index, recipe) in
                catalog
                    .screen_recipe_entries()
                    .iter()
                    .enumerate()
                    .filter(|(_, recipe)| {
                        recipe.kind == kind
                            && recipe.ingredients.len() == 1
                            && recipe.output.is_some()
                    })
            {
                let output = recipe.output.unwrap();
                let key = (output.network_id, output.aux, output.block_runtime_id);
                if let Some(&position) = positions.get(&key) {
                    if ledger.can_supply_furnace_recipe(recipe)
                        && !ledger.can_supply_furnace_recipe(listed[position].1)
                    {
                        listed[position] = (index, recipe);
                    }
                } else {
                    positions.insert(key, listed.len());
                    listed.push((index, recipe));
                }
            }
        }
        cache.all = listed.iter().map(|(index, _)| *index).collect();
        cache.supplied = listed
            .into_iter()
            .filter(|(_, recipe)| ledger.can_supply_furnace_recipe(recipe))
            .map(|(index, _)| index)
            .collect();
        cache.inputs = Some(inputs);
    }
    FurnaceRecipes {
        catalog,
        indices: Arc::clone(if filtering {
            &cache.supplied
        } else {
            &cache.all
        }),
    }
}
