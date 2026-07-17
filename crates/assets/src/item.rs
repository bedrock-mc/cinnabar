use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::AssetError;

pub const MAX_ITEM_VISUALS: usize = 16_384;
pub const MAX_ITEM_VISUAL_ALIASES: usize = 65_536;
pub const MAX_ITEM_IDENTIFIER_BYTES: usize = 256;
pub const MAX_BLOCK_VISUALS: usize = 65_536;

const MAX_ITEM_DISPLAY_SCALAR: f32 = 1_048_576.0;

/// Immutable identity for one normalized item stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemStackIdentity {
    pub network_id: i32,
    pub metadata: u32,
    pub stack_network_id: i32,
    pub count: u16,
    pub nbt_digest: [u8; 32],
}

impl ItemStackIdentity {
    /// Validates a nonempty identity and canonicalizes every empty stack.
    pub const fn validate(self) -> Result<Self, ItemStackIdentityError> {
        if self.count == 0 {
            return Ok(Self::empty());
        }
        if self.network_id < 0 {
            return Err(ItemStackIdentityError::NegativeNetworkId {
                network_id: self.network_id,
            });
        }
        Ok(self)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.count == 0
    }

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            network_id: 0,
            metadata: 0,
            stack_network_id: -1,
            count: 0,
            nbt_digest: [0; 32],
        }
    }
}

/// Validation failure for a nonempty item-stack identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ItemStackIdentityError {
    #[error("nonempty item stack has negative network ID {network_id}")]
    NegativeNetworkId { network_id: i32 },
}

/// Dense index into the compiled item-visual catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ItemVisualId(pub u32);

/// Dense index into the compiled block-visual catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct BlockVisualId(pub u32);

/// Canonical finite scalar retained by item display transforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ItemDisplayScalar(u32);

impl ItemDisplayScalar {
    #[must_use]
    pub fn new(value: f32) -> Option<Self> {
        if !value.is_finite() || value.abs() > MAX_ITEM_DISPLAY_SCALAR {
            return None;
        }
        Some(Self(if value == 0.0 { 0 } else { value.to_bits() }))
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        f32::from_bits(self.0)
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    fn is_canonical(self) -> bool {
        Self::new(self.get()) == Some(self)
    }
}

/// Fixed first-person, third-person, or dropped-item presentation transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDisplayTransform {
    pub translation: [ItemDisplayScalar; 3],
    pub rotation: [ItemDisplayScalar; 3],
    pub scale: [ItemDisplayScalar; 3],
}

impl ItemDisplayTransform {
    #[must_use]
    pub const fn identity() -> Self {
        let zero = ItemDisplayScalar(0);
        let one = ItemDisplayScalar(1.0_f32.to_bits());
        Self {
            translation: [zero; 3],
            rotation: [zero; 3],
            scale: [one; 3],
        }
    }

    fn is_canonical(&self) -> bool {
        self.translation
            .iter()
            .chain(&self.rotation)
            .chain(&self.scale)
            .all(|value| value.is_canonical())
    }
}

/// One dense item visual definition retained by the entity carrier.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemVisualDefinition {
    pub identifier: Box<str>,
    pub texture_source: u32,
    pub first_person: ItemDisplayTransform,
    pub third_person: ItemDisplayTransform,
    pub dropped: ItemDisplayTransform,
    pub block_visual: Option<BlockVisualId>,
}

/// Canonical identifier alias to a dense item visual.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemVisualAlias {
    pub identifier: Box<str>,
    pub visual: ItemVisualId,
}

pub(crate) fn validate_item_visuals(
    visuals: &[ItemVisualDefinition],
    aliases: &[ItemVisualAlias],
    sources: usize,
    block_visual_count: usize,
) -> Result<(), AssetError> {
    if visuals.len() > MAX_ITEM_VISUALS
        || aliases.len() > MAX_ITEM_VISUAL_ALIASES
        || block_visual_count > MAX_BLOCK_VISUALS
    {
        return Err(invalid("item visual or alias count exceeds bound"));
    }
    let mut previous_visual: Option<&str> = None;
    for visual in visuals {
        validate_item_identifier(&visual.identifier)?;
        if previous_visual.is_some_and(|previous| previous >= visual.identifier.as_ref())
            || visual.texture_source as usize >= sources
            || !visual.first_person.is_canonical()
            || !visual.third_person.is_canonical()
            || !visual.dropped.is_canonical()
            || visual
                .block_visual
                .is_some_and(|index| index.0 as usize >= block_visual_count)
        {
            return Err(invalid("invalid or unordered item visual"));
        }
        previous_visual = Some(&visual.identifier);
    }
    let mut previous_alias: Option<&str> = None;
    for alias in aliases {
        validate_item_identifier(&alias.identifier)?;
        if previous_alias.is_some_and(|previous| previous >= alias.identifier.as_ref())
            || alias.visual.0 as usize >= visuals.len()
        {
            return Err(invalid("invalid or unordered item visual alias"));
        }
        previous_alias = Some(&alias.identifier);
    }
    Ok(())
}

fn validate_item_identifier(identifier: &str) -> Result<(), AssetError> {
    if identifier.is_empty()
        || identifier.len() > MAX_ITEM_IDENTIFIER_BYTES
        || identifier.chars().any(char::is_control)
    {
        return Err(invalid(
            "item visual identifier is empty or exceeds its bound",
        ));
    }
    Ok(())
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

/// Immutable route from an item identity to its render representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemVisualRoute {
    Compiled(ItemVisualId),
    BlockItem(BlockVisualId),
    EmptyHand,
    Missing,
}

/// Stable icon reference into one compiled asset set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemIconRef {
    pub asset_identity: [u8; 32],
    pub texture_page: u16,
    pub uv: [u16; 4],
}

/// Shared local and remote presentation phase for an item action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemActionPhase {
    Idle,
    Windup {
        elapsed_ticks: u16,
    },
    Active {
        elapsed_ticks: u16,
    },
    Recover {
        elapsed_ticks: u16,
    },
    UseHeld {
        elapsed_ticks: u16,
        duration_ticks: u16,
    },
    Cancelled,
}
