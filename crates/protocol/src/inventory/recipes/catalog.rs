use super::{
    budget::{Credits, MAX_RECORDS, Permit},
    model::{RecipeHandle, RecipeUpdate},
    screen::{MAX_SCREEN_RECIPES, ScreenRecipe, ScreenRecipeKind, ScreenRecipes},
};
use std::{mem::size_of, sync::Arc};

#[derive(Debug)]
struct Entry {
    id: u32,
    handle: Option<RecipeHandle>,
}

#[derive(Debug)]
struct CatalogEntries {
    entries: Vec<Entry>,
    _permit: Permit,
}

/// Admitted wire recipes and their retained lifetime credits.
#[derive(Debug, Default, Clone)]
pub struct RecipeCatalog {
    session: u64,
    sequence: u64,
    revision: u64,
    storage: Option<Arc<CatalogEntries>>,
    screen: ScreenRecipes,
    available: bool,
    exhausted: bool,
}

impl RecipeCatalog {
    pub fn begin_session(&mut self, session: u64) {
        self.session = session;
        self.sequence = 0;
        self.retire();
    }
    fn retire(&mut self) {
        self.storage = None;
        self.screen = ScreenRecipes::default();
        self.available = false;
        match self.revision.checked_add(1) {
            Some(next) => self.revision = next,
            None => self.exhausted = true,
        }
    }
    /// One stonecutter, cartography or smithing recipe by network id.
    #[must_use]
    pub fn screen_recipe(&self, id: u32) -> Option<&ScreenRecipe> {
        self.screen.recipes.iter().find(|recipe| recipe.id == id)
    }

    /// Catalog positions are valid until the catalog revision changes.
    pub fn screen_recipe_entries(&self) -> &[ScreenRecipe] {
        &self.screen.recipes
    }

    /// Every retained recipe of one screen kind.
    pub fn screen_recipes(&self, kind: ScreenRecipeKind) -> impl Iterator<Item = &ScreenRecipe> {
        self.screen
            .recipes
            .iter()
            .filter(move |recipe| recipe.kind == kind)
    }

    /// The network id of the item-repair multi-recipe the anvil names.
    #[must_use]
    pub fn repair_multi_recipe_id(&self) -> Option<u32> {
        self.screen
            .multi
            .iter()
            .find(|multi| multi.is_repair())
            .map(|multi| multi.id)
    }

    fn merge_screens(&mut self, update: &RecipeUpdate, clear: bool) {
        if clear {
            self.screen = ScreenRecipes::default();
        }
        let Some(incoming) = update.screen.as_ref() else {
            return;
        };
        self.screen
            .recipes
            .retain(|old| incoming.recipes.iter().all(|new| new.id != old.id));
        self.screen.recipes.extend(incoming.recipes.iter().cloned());
        self.screen.recipes.truncate(MAX_SCREEN_RECIPES);
        self.screen
            .multi
            .retain(|old| incoming.multi.iter().all(|new| new.id != old.id));
        self.screen.multi.extend(incoming.multi.iter().copied());
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The session whose admitted wire recipes this catalog retains.
    pub fn session(&self) -> u64 {
        self.session
    }
    pub fn is_available(&self) -> bool {
        self.available
    }
    pub fn recipe(&self, id: u32) -> Option<RecipeHandle> {
        let entries = &self.storage.as_ref()?.entries;
        let index = entries.binary_search_by_key(&id, |entry| entry.id).ok()?;
        entries[index].handle.clone()
    }
    /// Every retained crafting-table recipe, any shape.
    pub fn crafting_recipes(&self) -> impl Iterator<Item = &RecipeHandle> {
        self.storage
            .iter()
            .flat_map(|storage| &storage.entries)
            .filter_map(|entry| entry.handle.as_ref())
    }
    pub(super) fn observation_entries(&self) -> impl Iterator<Item = (u32, &super::model::Recipe)> {
        self.storage
            .iter()
            .flat_map(|storage| &storage.entries)
            .filter_map(|entry| {
                entry
                    .handle
                    .as_ref()
                    .map(|handle| (entry.id, handle.recipe()))
                    .filter(|(_, recipe)| recipe.is_personal_named())
            })
    }
    /// Every accepted FIFO update advances authority, including unavailable-only
    /// replacements. A policy refusal retires the complete previous catalog.
    pub fn apply(&mut self, session: u64, sequence: u64, update: &RecipeUpdate) -> bool {
        self.apply_with_credits(session, sequence, update, &Credits::shared())
    }
    fn apply_with_credits(
        &mut self,
        session: u64,
        sequence: u64,
        update: &RecipeUpdate,
        credits: &Arc<Credits>,
    ) -> bool {
        if self.exhausted || session != self.session || session == 0 || sequence <= self.sequence {
            return false;
        }
        self.sequence = sequence;
        if self.revision == u64::MAX {
            self.retire();
            return true;
        }
        let Some(batch) = update.batch.as_ref() else {
            self.retire();
            return true;
        };
        let old = if batch.clear {
            &[][..]
        } else {
            self.storage
                .as_ref()
                .map_or(&[][..], |storage| &storage.entries[..])
        };
        let capacity = match old.len().checked_add(batch.records.len()) {
            Some(n) if n <= MAX_RECORDS * 2 => n,
            _ => {
                self.retire();
                return true;
            }
        };
        let charge = capacity
            .checked_mul(size_of::<Entry>())
            .and_then(|n| n.checked_add(128));
        let Some(permit) = charge.and_then(|n| credits.reserve(n)) else {
            self.retire();
            return true;
        };
        let mut merged = Vec::new();
        if merged.try_reserve_exact(capacity).is_err() {
            self.retire();
            return true;
        }
        let mut previous = 0;
        for (index, record) in batch.records.iter().enumerate() {
            while previous < old.len() && old[previous].id < record.id {
                merged.push(Entry {
                    id: old[previous].id,
                    handle: old[previous].handle.clone(),
                });
                previous += 1;
            }
            if previous < old.len() && old[previous].id == record.id {
                previous += 1;
            }
            merged.push(Entry {
                id: record.id,
                handle: record.recipe.as_ref().map(|_| RecipeHandle {
                    batch: Arc::clone(batch),
                    index,
                }),
            });
        }
        for entry in &old[previous..] {
            merged.push(Entry {
                id: entry.id,
                handle: entry.handle.clone(),
            });
        }
        if merged.len() > MAX_RECORDS {
            self.retire();
            return true;
        }
        self.storage = Some(Arc::new(CatalogEntries {
            entries: merged,
            _permit: permit,
        }));
        self.merge_screens(update, batch.clear);
        self.available = true;
        self.revision += 1; // checked exhaustion above, before any new authority.
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{Batch, Output, Recipe, Record};
    use super::*;

    fn update(owner: &Arc<Credits>, clear: bool) -> RecipeUpdate {
        RecipeUpdate {
            screen: None,
            batch: Some(Arc::new(Batch {
                records: vec![Record {
                    id: 17,
                    recipe: Some(Recipe {
                        width: 1,
                        height: 1,
                        shapeless: false,
                        mirror: false,
                        priority: 0,
                        ingredients: std::array::from_fn(|_| None),
                        output: Output {
                            id: 7,
                            aux: 0,
                            count: 4,
                            block: 0,
                            empty_envelope: false,
                        },
                    }),
                }],
                clear,
                _permit: owner.reserve(512).unwrap(),
            })),
        }
    }

    #[test]
    fn catalog_and_external_handle_keep_batch_credit_until_final_drop() {
        let owner = Credits::isolated(4096);
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(1);
        let update = update(&owner, true);
        assert!(catalog.apply_with_credits(1, 1, &update, &owner));
        let handle = catalog.recipe(17).unwrap();
        drop(update);
        assert_eq!(owner.used(), 512 + size_of::<Entry>() + 128);
        catalog.begin_session(2);
        assert_eq!(owner.used(), 512);
        assert_eq!(handle.network_id(), 17);
        drop(handle);
        assert_eq!(owner.used(), 0);
    }

    #[test]
    fn cloned_catalogs_share_structural_and_batch_credit_without_shared_mutation() {
        let owner = Credits::isolated(4096);
        let mut current = RecipeCatalog::default();
        current.begin_session(1);
        let first = update(&owner, true);
        current.apply_with_credits(1, 1, &first, &owner);
        drop(first);
        let old = current.clone();
        let last_old = old.clone();
        let charge = 512 + size_of::<Entry>() + 128;
        assert_eq!(owner.used(), charge);
        assert!(Arc::ptr_eq(
            current.storage.as_ref().unwrap(),
            old.storage.as_ref().unwrap()
        ));
        let mut replacement = update(&owner, true);
        Arc::get_mut(replacement.batch.as_mut().unwrap())
            .unwrap()
            .records[0]
            .recipe
            .as_mut()
            .unwrap()
            .output
            .count = 2;
        current.apply_with_credits(1, 2, &replacement, &owner);
        drop(replacement);
        assert_eq!(owner.used(), charge * 2);
        assert_eq!(current.recipe(17).unwrap().recipe().output.count, 2);
        assert_eq!(old.recipe(17).unwrap().recipe().output.count, 4);
        drop(current);
        assert_eq!(owner.used(), charge);
        drop(old);
        assert_eq!(owner.used(), charge);
        drop(last_old);
        assert_eq!(owner.used(), 0);
    }

    #[test]
    fn refused_replacement_does_not_remint_or_retire_a_retained_clone() {
        let owner = Credits::isolated(4096);
        let mut current = RecipeCatalog::default();
        current.begin_session(1);
        let first = update(&owner, true);
        current.apply_with_credits(1, 1, &first, &owner);
        drop(first);
        let old = current.clone();
        let charge = 512 + size_of::<Entry>() + 128;
        let replacement = update(&owner, true);
        let occupied = owner.reserve(4096 - owner.used()).unwrap();
        current.apply_with_credits(1, 2, &replacement, &owner);
        assert!(!current.is_available());
        assert!(old.recipe(17).is_some());
        assert_eq!(owner.used(), 4096);
        drop(occupied);
        assert_eq!(owner.used(), charge + 512);
        drop(replacement);
        assert_eq!(owner.used(), charge);
        drop(current);
        assert_eq!(owner.used(), charge);
        drop(old);
        assert_eq!(owner.used(), 0);
    }

    #[test]
    fn merge_scratch_refusal_retires_catalog_without_unaccounted_storage() {
        let owner = Credits::isolated(4096);
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(1);
        let first = update(&owner, true);
        assert!(catalog.apply_with_credits(1, 1, &first, &owner));
        drop(first);
        let replacement = update(&owner, false);
        let occupied = owner.reserve(4096 - owner.used()).unwrap();
        assert!(catalog.apply_with_credits(1, 2, &replacement, &owner));
        assert!(!catalog.is_available());
        assert!(catalog.recipe(17).is_none());
        drop(replacement);
        drop(occupied);
        assert_eq!(owner.used(), 0);
    }

    #[test]
    fn oversized_merged_catalog_releases_reserved_scratch_on_policy_return() {
        let owner = Credits::isolated(4 * 1024 * 1024);
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(1);
        let first = update(&owner, true);
        catalog.apply_with_credits(1, 1, &first, &owner);
        drop(first);
        let charge = MAX_RECORDS * size_of::<Record>() + 512;
        let permit = owner.reserve(charge).unwrap();
        let replacement = RecipeUpdate {
            screen: None,
            batch: Some(Arc::new(Batch {
                records: (100..100 + MAX_RECORDS as u32)
                    .map(|id| Record { id, recipe: None })
                    .collect(),
                clear: false,
                _permit: permit,
            })),
        };
        // Both individually bounded batches fit. Their disjoint merge reserves
        // scratch successfully, then exceeds the retained catalog cardinality.
        assert!(catalog.apply_with_credits(1, 2, &replacement, &owner));
        assert!(!catalog.is_available());
        assert_eq!(owner.used(), charge);
        drop(replacement);
        assert_eq!(owner.used(), 0);
    }
}
