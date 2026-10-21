use std::{mem::size_of, sync::Arc};

use super::super::{
    RecipeCatalog, RecipeIdentity, RecipeUnlockingContext,
    budget::{Credits, MAX_RECORDS, Permit},
};
use super::{RecipeUnlockOperation, RecipeUnlockUpdate};

#[derive(Clone, Debug)]
struct Entry {
    name: Arc<NameOwner>,
    newly: bool,
}

#[derive(Debug)]
struct NameOwner {
    value: String,
    _permit: Permit,
}

impl NameOwner {
    /// Retains just the exact name, with credit independent of its source recipe batch.
    fn new(name: &str, credits: &Arc<Credits>) -> Option<Arc<Self>> {
        let bytes = size_of::<Self>().checked_add(name.len())?.checked_add(64)?;
        let permit = credits.reserve(bytes)?;
        let mut value = String::new();
        value.try_reserve_exact(name.len()).ok()?;
        value.push_str(name);
        Some(Arc::new(Self {
            value,
            _permit: permit,
        }))
    }
}

#[derive(Debug)]
struct Membership {
    entries: Vec<Entry>,
    _permit: Permit,
}

/// Explicit name membership survives catalog replacement and shares immutable snapshot credit.
#[derive(Clone, Debug, Default)]
pub struct RecipeUnlockState {
    membership: Option<Arc<Membership>>,
}

impl RecipeUnlockState {
    /// Whether this exact name has received a resolvable explicit unlock.
    pub fn contains(&self, name: &str) -> bool {
        self.entries()
            .binary_search_by(|entry| entry.name.value.as_str().cmp(name))
            .is_ok()
    }

    /// Whether a New update marked this exact name since its last removal or clear.
    pub fn is_newly_unlocked(&self, name: &str) -> bool {
        self.entries()
            .binary_search_by(|entry| entry.name.value.as_str().cmp(name))
            .is_ok_and(|index| self.entries()[index].newly)
    }

    /// Tests context and names only; callers own rule and authority-availability gates.
    pub fn is_unlocked(&self, recipe: &RecipeIdentity) -> bool {
        recipe.unlocking_context() == RecipeUnlockingContext::AlwaysUnlocked
            || self.contains(recipe.identifier())
    }

    /// Retires explicit authority, including its newly-unlocked bookkeeping.
    pub fn clear(&mut self) {
        self.membership = None;
    }

    /// Resolves additions against the catalog at this update's ordered application point.
    pub fn apply(&mut self, catalog: &RecipeCatalog, update: &RecipeUnlockUpdate) {
        self.apply_with_credits(catalog, update, &Credits::shared());
    }

    /// Borrows the one sorted set shared by membership and new-item bookkeeping.
    fn entries(&self) -> &[Entry] {
        self.membership
            .as_ref()
            .map_or(&[], |membership| &membership.entries)
    }

    /// Reserves copy-on-write structural storage before retaining independently credited names.
    pub(super) fn apply_with_credits(
        &mut self,
        catalog: &RecipeCatalog,
        update: &RecipeUnlockUpdate,
        credits: &Arc<Credits>,
    ) {
        match update.operation {
            RecipeUnlockOperation::Empty => return,
            RecipeUnlockOperation::Clear => {
                self.clear();
                return;
            }
            _ => {}
        }
        let Some(names) = &update.names else {
            self.clear();
            return;
        };
        if names.values.is_empty() {
            return;
        }
        let old = self.entries();
        let capacity = old.len().saturating_add(names.values.len());
        let charge = capacity
            .checked_mul(size_of::<Entry>())
            .and_then(|bytes| bytes.checked_add(128));
        let Some(permit) = charge.and_then(|bytes| credits.reserve(bytes)) else {
            self.refuse();
            return;
        };
        let mut entries = Vec::new();
        if entries.try_reserve_exact(capacity).is_err() {
            self.refuse();
            return;
        }
        entries.extend(old.iter().cloned());
        let mut skipped = 0usize;
        if update.operation == RecipeUnlockOperation::Remove {
            entries.retain(|entry| {
                names
                    .values
                    .binary_search_by(|name| name.as_str().cmp(&entry.name.value))
                    .is_err()
            });
        } else {
            for name in &names.values {
                let Some(identity) = catalog.resolve_unlock_identifier(name) else {
                    skipped += 1;
                    continue;
                };
                let newly = update.operation == RecipeUnlockOperation::New;
                match old.binary_search_by(|entry| entry.name.value.cmp(name)) {
                    Ok(index) => entries[index].newly |= newly,
                    Err(_) => {
                        let Some(name) = NameOwner::new(identity.identity().identifier(), credits)
                        else {
                            self.refuse();
                            return;
                        };
                        entries.push(Entry { name, newly });
                    }
                }
            }
            entries.sort_unstable_by(|a, b| a.name.value.cmp(&b.name.value));
        }
        if skipped != 0 {
            tracing::warn!(target: "cinnabar::inventory_requests", skipped,
                "skipped unknown or ambiguous recipe unlock identifiers");
        }
        if entries.len() > MAX_RECORDS {
            self.refuse();
            return;
        }
        self.membership = (!entries.is_empty()).then(|| {
            Arc::new(Membership {
                entries,
                _permit: permit,
            })
        });
    }

    /// Drops uncertain authority when an update exceeds the shared retention policy.
    fn refuse(&mut self) {
        tracing::warn!(target: "cinnabar::inventory_requests", skipped = 1, "recipe unlock authority exceeded retention policy");
        self.clear();
    }
}
