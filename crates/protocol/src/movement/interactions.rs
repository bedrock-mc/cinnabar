//! Interaction payloads carried inside one protocol-2168 `PlayerAuthInput`.
//!
//! The current client no longer sends block destruction through
//! `PlayerAction` packets: the start, abort, continue, and local prediction of
//! a destroy travel as the block-action list of the movement tick in which
//! they happened (`PerformBlockActions`), and the creative instant destroy
//! travels as the embedded break-block item-use transaction
//! (`PerformItemInteraction`). This module owns those bounded payloads; the
//! movement snapshot itself stays a pure movement record.

use thiserror::Error;
use valentine::bedrock::version::v1_26_44::{
    BlockPos, EnumsItemUseInventoryTransactionActionType, EnumsPlayerActionType,
    PackedItemUseLegacyInventoryTransaction, PlayerBlockActionData,
    TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0,
};

use crate::interaction::{BlockUsePacketError, BlockUseRequest, item_use_transaction};

/// Maximum block actions one movement tick may carry.
///
/// A vanilla tick produces at most a handful (an abort plus a start when the
/// target changes, or a start followed by an instant prediction); eight keeps
/// pathological input from growing the packet without bound.
pub const MAX_BLOCK_ACTIONS_PER_INPUT: usize = 8;

/// The block-action kinds a client may place inside `PlayerAuthInput`.
///
/// This bounded API exposes the destroy actions used by the intended mining
/// workflow, with completion represented by [`Self::PredictDestroy`]. The wire
/// codec can also represent `StopDestroyBlock`; this API does not expose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockActionKind {
    /// Wire value 0: the player started destroying the block.
    StartDestroy,
    /// Wire value 1: the player stopped before the block was destroyed.
    AbortDestroy,
    /// Wire value 18: the destroy continues on the same block this tick.
    CrackBlock,
    /// Wire value 26: the client predicts that its destroy completed.
    PredictDestroy,
    /// Wire value 27: the destroy moved to a new block without a release.
    ContinueDestroy,
}

impl BlockActionKind {
    /// The pinned protocol-2168 `PlayerActionType` value.
    #[must_use]
    pub const fn wire_value(self) -> i32 {
        match self {
            Self::StartDestroy => 0,
            Self::AbortDestroy => 1,
            Self::CrackBlock => 18,
            Self::PredictDestroy => 26,
            Self::ContinueDestroy => 27,
        }
    }

    const fn vendor(self) -> EnumsPlayerActionType {
        match self {
            Self::StartDestroy => EnumsPlayerActionType::StartDestroyBlock,
            Self::AbortDestroy => EnumsPlayerActionType::AbortDestroyBlock,
            Self::CrackBlock => EnumsPlayerActionType::CrackBlock,
            Self::PredictDestroy => EnumsPlayerActionType::PredictDestroyBlock,
            Self::ContinueDestroy => EnumsPlayerActionType::ContinueDestroyBlock,
        }
    }
}

/// One block action: the kind, the absolute block position, and the face
/// (`0` down, `1` up, `2` north, `3` south, `4` west, `5` east).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockAction {
    pub kind: BlockActionKind,
    pub position: [i32; 3],
    pub face: u8,
}

/// The block-action list is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("PlayerAuthInput block-action list holds at most {MAX_BLOCK_ACTIONS_PER_INPUT} actions")]
pub struct BlockActionsFull;

/// Bounded, order-preserving block-action list for one movement tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockActions {
    actions: [Option<BlockAction>; MAX_BLOCK_ACTIONS_PER_INPUT],
    len: u8,
}

impl BlockActions {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            actions: [None; MAX_BLOCK_ACTIONS_PER_INPUT],
            len: 0,
        }
    }

    /// Appends one action, refusing (without mutation) when the list is full.
    pub fn push(&mut self, action: BlockAction) -> Result<(), BlockActionsFull> {
        let index = usize::from(self.len);
        let slot = self.actions.get_mut(index).ok_or(BlockActionsFull)?;
        *slot = Some(action);
        self.len += 1;
        Ok(())
    }

    /// Appends every action of `other` atomically: either all fit or the
    /// list is left untouched.
    pub fn append(&mut self, other: &Self) -> Result<(), BlockActionsFull> {
        if self.len() + other.len() > MAX_BLOCK_ACTIONS_PER_INPUT {
            return Err(BlockActionsFull);
        }
        for action in other.iter() {
            self.push(*action)?;
        }
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &BlockAction> {
        self.actions[..usize::from(self.len)]
            .iter()
            .filter_map(Option::as_ref)
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Removes and returns every retained action.
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    pub(super) fn vendor(&self) -> Result<Vec<PlayerBlockActionData>, InteractionEncodeError> {
        self.iter()
            .map(|action| {
                if action.face > 5 {
                    return Err(InteractionEncodeError::InvalidBlockActionFace(action.face));
                }
                let [x, y, z] = action.position;
                Ok(PlayerBlockActionData {
                    player_action_type: action.kind.vendor(),
                    position: BlockPos { x, y, z },
                    facing: i32::from(action.face),
                })
            })
            .collect()
    }
}

/// Interaction payloads attached to one movement tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerAuthInputInteractions {
    /// Destroy-family block actions in the order they happened.
    pub block_actions: BlockActions,
    /// The creative instant destroy of one block, carried as the embedded
    /// break-block item-use transaction.
    pub block_destroy: Option<BlockUseRequest>,
}

impl PlayerAuthInputInteractions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.block_actions.is_empty() && self.block_destroy.is_none()
    }
}

/// Invalid interaction state that cannot be represented on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InteractionEncodeError {
    #[error("block action face {0} is outside 0..=5")]
    InvalidBlockActionFace(u8),
    #[error("embedded break-block transaction is invalid: {0}")]
    InvalidBlockDestroy(#[from] BlockUsePacketError),
    #[error(
        "PlayerAuthInput interaction flags were asserted without a matching payload (or the reverse)"
    )]
    InconsistentInteractionFlags,
}

pub(super) fn packed_block_destroy(
    request: BlockUseRequest,
) -> Result<PackedItemUseLegacyInventoryTransaction, InteractionEncodeError> {
    let transaction = item_use_transaction(
        request,
        EnumsItemUseInventoryTransactionActionType::Destroy,
        // The embedded carrier writes the action list through a second
        // optional layer; a break carries no inventory actions, so the inner
        // layer is absent exactly like the pinned public bytes.
        None,
    )?;
    Ok(PackedItemUseLegacyInventoryTransaction {
        legacy_request_id: TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0 { id: 0 },
        legacy_set_item_slots: None,
        item_use_transaction: Some(transaction),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(kind: BlockActionKind, face: u8) -> BlockAction {
        BlockAction {
            kind,
            position: [1, -2, 3],
            face,
        }
    }

    #[test]
    fn wire_values_are_the_pinned_destroy_family() {
        assert_eq!(BlockActionKind::StartDestroy.wire_value(), 0);
        assert_eq!(BlockActionKind::AbortDestroy.wire_value(), 1);
        assert_eq!(BlockActionKind::CrackBlock.wire_value(), 18);
        assert_eq!(BlockActionKind::PredictDestroy.wire_value(), 26);
        assert_eq!(BlockActionKind::ContinueDestroy.wire_value(), 27);
    }

    #[test]
    fn list_is_bounded_and_order_preserving() {
        let mut actions = BlockActions::new();
        for index in 0..MAX_BLOCK_ACTIONS_PER_INPUT {
            actions
                .push(action(BlockActionKind::CrackBlock, index as u8 % 6))
                .unwrap();
        }
        assert_eq!(actions.len(), MAX_BLOCK_ACTIONS_PER_INPUT);
        assert_eq!(
            actions.push(action(BlockActionKind::AbortDestroy, 0)),
            Err(BlockActionsFull)
        );
        assert_eq!(actions.len(), MAX_BLOCK_ACTIONS_PER_INPUT);
        let faces = actions.iter().map(|action| action.face).collect::<Vec<_>>();
        assert_eq!(faces, vec![0, 1, 2, 3, 4, 5, 0, 1]);
        let taken = actions.take();
        assert!(actions.is_empty());
        assert_eq!(taken.len(), MAX_BLOCK_ACTIONS_PER_INPUT);
    }

    #[test]
    fn append_is_atomic() {
        let mut left = BlockActions::new();
        for _ in 0..6 {
            left.push(action(BlockActionKind::CrackBlock, 1)).unwrap();
        }
        let mut right = BlockActions::new();
        for _ in 0..3 {
            right
                .push(action(BlockActionKind::StartDestroy, 2))
                .unwrap();
        }
        assert_eq!(left.append(&right), Err(BlockActionsFull));
        assert_eq!(left.len(), 6);
        let mut small = BlockActions::new();
        small
            .push(action(BlockActionKind::StartDestroy, 2))
            .unwrap();
        left.append(&small).unwrap();
        assert_eq!(left.len(), 7);
        assert_eq!(
            left.iter().last().unwrap().kind,
            BlockActionKind::StartDestroy
        );
    }

    #[test]
    fn faces_outside_the_cube_fail_closed() {
        let mut actions = BlockActions::new();
        actions
            .push(action(BlockActionKind::StartDestroy, 6))
            .unwrap();
        assert_eq!(
            actions.vendor().unwrap_err(),
            InteractionEncodeError::InvalidBlockActionFace(6)
        );
    }
}
