use thiserror::Error;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ItemVisualId(pub u32);

/// Dense index into the compiled block-visual catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockVisualId(pub u32);

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
