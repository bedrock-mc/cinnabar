//! Explicit recipe-name authority, independent of recipe admission and presentation.
mod state;
mod wire;

use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::PacketType;

use super::budget::Permit;

pub use state::RecipeUnlockState;
pub use wire::decode_recipe_unlocks;
pub(crate) use wire::normalize_recipe_unlocks;

/// The server's operation, preserving unfamiliar values for additive handling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeUnlockOperation {
    Empty,
    Initial,
    New,
    Remove,
    Clear,
    Unknown(u32),
}

impl RecipeUnlockOperation {
    /// Converts through the generated codec's single source of numeric values.
    pub fn from_index(index: u32) -> Self {
        match PacketType::from(index) {
            PacketType::Empty => Self::Empty,
            PacketType::Initiallyunlockedrecipes => Self::Initial,
            PacketType::Newlyunlockedrecipes => Self::New,
            PacketType::Removeunlockedrecipes => Self::Remove,
            PacketType::Removeallunlockedrecipes => Self::Clear,
            PacketType::Unknown(value) => Self::Unknown(value),
        }
    }
}

#[derive(Debug)]
struct Names {
    values: Vec<String>,
    _permit: Permit,
}

/// A bounded packet update; copies share its exact strings and lifetime credit.
#[derive(Clone, Debug)]
pub struct RecipeUnlockUpdate {
    operation: RecipeUnlockOperation,
    names: Option<Arc<Names>>,
}

impl RecipeUnlockUpdate {
    /// The operation received, including an unfamiliar numeric discriminator.
    pub const fn operation(&self) -> RecipeUnlockOperation {
        self.operation
    }

    /// Whether allocation or traversal policy prevented complete retention.
    pub fn is_unavailable(&self) -> bool {
        self.names.is_none()
    }

    /// Exact names retained by this update; duplicates have set semantics.
    pub fn identifiers(&self) -> impl Iterator<Item = &str> {
        self.names
            .iter()
            .flat_map(|names| names.values.iter().map(String::as_str))
    }
}

impl PartialEq for RecipeUnlockUpdate {
    /// Compares packet meaning without comparing allocation owners.
    fn eq(&self, other: &Self) -> bool {
        self.operation == other.operation
            && match (&self.names, &other.names) {
                (Some(a), Some(b)) => a.values == b.values,
                (None, None) => true,
                _ => false,
            }
    }
}
impl Eq for RecipeUnlockUpdate {}

#[cfg(test)]
mod tests;
