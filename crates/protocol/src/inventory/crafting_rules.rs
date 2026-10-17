//! Sparse crafting rules and their session defaults.

/// Server changes to recipe discovery and limited manual crafting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CraftingRules {
    pub recipes_unlock: Option<bool>,
    pub limited_crafting: Option<bool>,
}

impl CraftingRules {
    pub const DEFAULT_RECIPES_UNLOCK: bool = true;
    pub const DEFAULT_LIMITED_CRAFTING: bool = false;

    /// Whether recipe discovery authority is enabled, including its startup default.
    pub fn unlocking_enabled(self) -> bool {
        self.recipes_unlock.unwrap_or(Self::DEFAULT_RECIPES_UNLOCK)
    }

    /// Whether manual crafting is limited to discovered recipes.
    pub fn limited_crafting_enabled(self) -> bool {
        self.limited_crafting
            .unwrap_or(Self::DEFAULT_LIMITED_CRAFTING)
    }

    /// Manual recipe selection requires discovery only when both rules are enabled.
    pub fn requires_manual_unlock(self) -> bool {
        self.unlocking_enabled() && self.limited_crafting_enabled()
    }

    /// Fills absent startup rules from their registered defaults.
    pub fn resolved(self) -> Self {
        Self {
            recipes_unlock: Some(self.unlocking_enabled()),
            limited_crafting: Some(self.limited_crafting_enabled()),
        }
    }

    /// Applies only fields declared by a later rule update.
    pub fn apply(&mut self, update: Self) {
        *self = Self {
            recipes_unlock: update.recipes_unlock.or(self.recipes_unlock),
            limited_crafting: update.limited_crafting.or(self.limited_crafting),
        };
    }

    /// Whether this packet declares either crafting rule.
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }

    /// Whether both fields have authoritative values after bootstrap or later recovery.
    pub fn is_complete(self) -> bool {
        matches!(
            self,
            Self {
                recipes_unlock: Some(_),
                limited_crafting: Some(_)
            }
        )
    }
}
