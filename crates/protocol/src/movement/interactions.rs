//! Interaction payloads carried inside one protocol-2168 `PlayerAuthInput`.
//!
//! The current client no longer sends block destruction through
//! `PlayerAction` packets: the start, abort, continue, and local prediction of
//! a destroy travel as the block-action list of the movement tick in which
//! they happened (`PerformBlockActions`), and the creative instant destroy
//! travels as the embedded break-block item-use transaction
//! (`PerformItemInteraction`). Block placement is a standalone transaction.
//! This module owns those bounded payloads; the movement snapshot itself stays
//! a pure movement record.

use bedrock_protocol_1_26_51::enums::{ItemUseInventoryTransactionActionType, PlayerActionType};
use bedrock_protocol_1_26_51::types::{
    BlockPos, ItemStackLegacyRequestID, PackedItemUseLegacyInventoryTransaction,
    PlayerBlockActionData,
};
use bedrock_protocol_1_26_51::wire;
use thiserror::Error;

use crate::interaction::{BlockUsePacketError, BlockUseRequest, item_use_transaction};

/// Maximum block actions one movement tick may carry.
///
/// A vanilla tick produces at most a handful (an abort plus a start when the
/// target changes, or a start followed by an instant prediction); eight keeps
/// pathological input from growing the packet without bound.
pub const MAX_BLOCK_ACTIONS_PER_INPUT: usize = 8;

/// The block-action kinds a client may place inside `PlayerAuthInput`.
///
/// Server-authoritative completion is [`Self::PredictDestroy`]; client-authoritative
/// completion is [`Self::StopDestroy`] beside an item-use destroy transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockActionKind {
    /// Wire value 0: the player started destroying the block.
    StartDestroy,
    /// Wire value 1: the player stopped before the block was destroyed. Its
    /// face slot carries the whole-percent progress reached, `0..=100`.
    AbortDestroy,
    /// Wire value 2: a client-authoritative destroy completed.
    StopDestroy,
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
            Self::StopDestroy => 2,
            Self::CrackBlock => 18,
            Self::PredictDestroy => 26,
            Self::ContinueDestroy => 27,
        }
    }

    const fn vendor(self) -> PlayerActionType {
        match self {
            Self::StartDestroy => PlayerActionType::Startdestroyblock,
            Self::AbortDestroy => PlayerActionType::Abortdestroyblock,
            Self::StopDestroy => PlayerActionType::Stopdestroyblock,
            Self::CrackBlock => PlayerActionType::Crackblock,
            Self::PredictDestroy => PlayerActionType::Predictdestroyblock,
            Self::ContinueDestroy => PlayerActionType::Continuedestroyblock,
        }
    }
}

/// One block action: the kind, the absolute block position, and the face
/// (`0` down, `1` up, `2` north, `3` south, `4` west, `5` east), except that an
/// abort carries its progress percent there.
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
                let face_limit = match action.kind {
                    BlockActionKind::AbortDestroy => 100,
                    _ => 5,
                };
                if action.face > face_limit {
                    return Err(InteractionEncodeError::InvalidBlockActionFace(action.face));
                }
                let [x, y, z] = action.position;
                Ok(PlayerBlockActionData {
                    player_action_type: action.kind.vendor(),
                    position: BlockPos {
                        x: wire::ZigZag32(x),
                        y: wire::ZigZag32(y),
                        z: wire::ZigZag32(z),
                    },
                    facing: wire::ZigZag32(i32::from(action.face)),
                })
            })
            .collect()
    }
}

/// The one item-use transaction attached to a movement tick.
///
/// The client uses `Destroy`; `Use` stays encodable for wire fixtures.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockItemInteraction {
    Use(BlockUseRequest),
    Destroy(BlockUseRequest),
}

/// Interaction payloads attached to one movement tick.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerAuthInputInteractions {
    /// Destroy-family block actions in the order they happened.
    pub block_actions: BlockActions,
    /// At most one mutually exclusive block item-use transaction.
    pub block_interaction: Option<BlockItemInteraction>,
}

impl PlayerAuthInputInteractions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.block_actions.is_empty() && self.block_interaction.is_none()
    }
}

/// Invalid interaction state that cannot be represented on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InteractionEncodeError {
    #[error("block action face {0} is outside its kind's range")]
    InvalidBlockActionFace(u8),
    #[error("embedded break-block transaction is invalid: {0}")]
    InvalidBlockDestroy(#[from] BlockUsePacketError),
    #[error("embedded block-use transaction is invalid: {0}")]
    InvalidBlockUse(BlockUsePacketError),
    #[error(
        "PlayerAuthInput interaction flags were asserted without a matching payload (or the reverse)"
    )]
    InconsistentInteractionFlags,
}

pub(super) fn packed_block_interaction(
    interaction: BlockItemInteraction,
) -> Result<PackedItemUseLegacyInventoryTransaction, InteractionEncodeError> {
    let transaction = match interaction {
        BlockItemInteraction::Use(request) => {
            item_use_transaction(request, ItemUseInventoryTransactionActionType::Place)
                .map_err(InteractionEncodeError::InvalidBlockUse)?
        }
        BlockItemInteraction::Destroy(request) => {
            item_use_transaction(request, ItemUseInventoryTransactionActionType::Destroy)
                .map_err(InteractionEncodeError::InvalidBlockDestroy)?
        }
    };
    Ok(PackedItemUseLegacyInventoryTransaction {
        legacy_request_id: ItemStackLegacyRequestID(0),
        legacy_set_item_slots: None,
        item_use_transaction: transaction,
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
        assert_eq!(BlockActionKind::StopDestroy.wire_value(), 2);
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

    #[test]
    fn abort_carries_progress_percent_in_the_face_slot() {
        let mut actions = BlockActions::new();
        actions
            .push(action(BlockActionKind::AbortDestroy, 100))
            .unwrap();
        assert_eq!(actions.vendor().unwrap()[0].facing.0, 100);
        actions.clear();
        actions
            .push(action(BlockActionKind::AbortDestroy, 101))
            .unwrap();
        assert_eq!(
            actions.vendor().unwrap_err(),
            InteractionEncodeError::InvalidBlockActionFace(101)
        );
    }
}
