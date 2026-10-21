use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::RecipeUnlockingRequirementUnlockingContext as WireContext;

use super::model::Batch;

/// Discovery context is metadata; only AlwaysUnlocked bypasses explicit membership.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecipeUnlockingContext {
    #[default]
    Ingredients,
    AlwaysUnlocked,
    PlayerInWater,
    PlayerHasManyItems,
    Unknown(i32),
}

impl RecipeUnlockingContext {
    /// Preserves every generated context value, including unfamiliar ones.
    pub fn from_index(index: i32) -> Self {
        match WireContext::from(index) {
            WireContext::None => Self::Ingredients,
            WireContext::Alwaysunlocked => Self::AlwaysUnlocked,
            WireContext::Playerinwater => Self::PlayerInWater,
            WireContext::Playerhasmanyitems => Self::PlayerHasManyItems,
            WireContext::Unknown(value) => Self::Unknown(value),
        }
    }
}

/// Named metadata retained even when a recipe has no supported crafting handle.
#[derive(Debug, PartialEq, Eq)]
pub struct RecipeIdentity {
    pub(super) name: String,
    pub(super) tag: String,
    pub(super) context: RecipeUnlockingContext,
    pub(super) network_id: u32,
}

impl RecipeIdentity {
    /// The exact recipe string received from the server.
    pub fn identifier(&self) -> &str {
        &self.name
    }

    /// The recipe's station tag, independent of supported screen consumers.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The discovery context, with absent requirements represented as Ingredients.
    pub const fn unlocking_context(&self) -> RecipeUnlockingContext {
        self.context
    }

    /// The network ID carried by the same named record.
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Whether this tag participates in packet-based recipe unlock resolution.
    pub(super) fn accepts_unlocks(&self) -> bool {
        !matches!(
            self.tag.as_str(),
            "cartography_table"
                | "smithing_table"
                | "stonecutter"
                | "deprecated"
                | "campfire"
                | "soul_campfire"
        )
    }
}

/// A named recipe cannot outlive the immutable batch and its string allocation credit.
#[derive(Clone, Debug)]
pub struct RecipeIdentityHandle {
    pub(super) batch: Arc<Batch>,
    pub(super) index: usize,
    order: usize,
}

impl RecipeIdentityHandle {
    /// Borrows exact metadata while retaining its charged owner.
    pub fn identity(&self) -> &RecipeIdentity {
        &self.batch.identities[self.index]
    }
}

/// Counts structural handles before allocating a merged identity view.
pub(super) fn merged_capacity(old: &[RecipeIdentityHandle], batch: &Batch) -> Option<usize> {
    old.len().checked_add(batch.identities.len())
}

/// Retains the first record for each exact tag/name pair without cloning its strings.
pub(super) fn merge(
    old: &[RecipeIdentityHandle],
    batch: &Arc<Batch>,
) -> Option<Vec<RecipeIdentityHandle>> {
    let mut identities = Vec::new();
    identities
        .try_reserve_exact(merged_capacity(old, batch)?)
        .ok()?;
    identities.extend(old.iter().cloned().enumerate().map(|(order, mut entry)| {
        entry.order = order;
        entry
    }));
    identities.extend(
        (0..batch.identities.len()).map(|index| RecipeIdentityHandle {
            batch: Arc::clone(batch),
            index,
            order: old.len() + index,
        }),
    );
    // The ordinal makes first-arrival precedence independent of the sorting algorithm.
    identities.sort_unstable_by(|a, b| {
        let a_id = a.identity();
        let b_id = b.identity();
        (&a_id.name, &a_id.tag, a.order).cmp(&(&b_id.name, &b_id.tag, b.order))
    });
    identities.dedup_by(|a, b| {
        let a = a.identity();
        let b = b.identity();
        a.tag == b.tag && a.name == b.name
    });
    (identities.len() <= super::budget::MAX_RECORDS).then_some(identities)
}
