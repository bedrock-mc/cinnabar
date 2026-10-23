//! Player discovery policy, separate from ingredient matching and output grouping.
use super::CraftingAuthority;
use protocol::RecipeHandle;

/// Compared by the book listing cache; see [`CraftingAuthority::book_discovery_key`].
pub(crate) type BookDiscoveryKey = (u64, Option<protocol::CraftingRules>, Option<bool>);

impl CraftingAuthority {
    /// A complete rule frontier and required component are prerequisites for manual matching.
    pub(crate) fn manual_recipes_available(&self) -> bool {
        self.crafting_rules().is_some_and(|rules| {
            !rules.requires_manual_unlock() || self.unlock_component == Some(true)
        })
    }

    /// Applies discovery to this exact manual recipe before competing outputs are compared.
    pub(crate) fn manual_recipe_allowed(&self, recipe: &RecipeHandle) -> bool {
        self.crafting_rules().is_some_and(|rules| {
            !rules.requires_manual_unlock() || self.explicit_recipe_unlocked(recipe)
        })
    }

    /// Book discovery has a legacy bypass only while the component is known to be absent.
    pub(crate) fn book_recipe_unlocked(&self, recipe: &RecipeHandle) -> bool {
        self.crafting_rules().is_some_and(|rules| {
            !rules.unlocking_enabled()
                || self.unlock_component == Some(false)
                || (self.unlock_component == Some(true) && self.explicit_recipe_unlocked(recipe))
        })
    }

    /// A retained name grant or AlwaysUnlocked context authorizes only its own recipe.
    fn explicit_recipe_unlocked(&self, recipe: &RecipeHandle) -> bool {
        recipe
            .identity()
            .is_some_and(|identity| self.unlocks.is_unlocked(identity))
    }

    /// Rejects old handles when a catalog replacement reuses their numeric IDs.
    pub(crate) fn contains_recipe(&self, recipe: &RecipeHandle) -> bool {
        self.catalog()
            .and_then(|catalog| catalog.recipe(recipe.network_id()))
            .is_some_and(|current| std::ptr::eq(current.recipe(), recipe.recipe()))
    }

    /// Invalidates presentation after discovery, rules, cells or authority change.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Everything book discovery reads besides the catalog: the presentation revision, plus the
    /// rules and unlock component, which session retirement resets without advancing it.
    pub(crate) fn book_discovery_key(&self) -> BookDiscoveryKey {
        (self.revision, self.crafting_rules(), self.unlock_component)
    }
}
