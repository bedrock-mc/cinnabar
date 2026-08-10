use thiserror::Error;
use valentine::bedrock::version::v1_26_40::{
    BlockPos, InventoryTransaction, InventoryTransactionPacket,
    InventoryTransactionPacketTransaction, ItemUseInventoryTransaction,
    ItemUseInventoryTransactionActionType, ItemUseInventoryTransactionClientCooldownState,
    ItemUseInventoryTransactionClientInteractPrediction, ItemUseInventoryTransactionTriggerType,
    TypedClientNetIdStructItemStackLegacyRequestIdTagInt32T0, Vec3,
};

use crate::{BedrockSession, InventoryPacketError, VerifiedNetworkItemStack};

/// All authoritative state needed to encode one protocol-2168 click-block transaction.
///
/// Reach, ray selection, block identity, and selected-item authority belong to the caller. This
/// type deliberately does not infer or mutate any of them.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockUseRequest {
    pub block_position: [i32; 3],
    pub face: u8,
    pub selected_slot: u8,
    pub selected_item: VerifiedNetworkItemStack,
    pub player_position: [f32; 3],
    pub relative_hit: [f32; 3],
    pub block_runtime_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockUsePacketError {
    #[error("block face {0} is outside 0..=5")]
    InvalidFace(u8),
    #[error("selected hotbar slot {0} is outside 0..=8")]
    InvalidSelectedSlot(u8),
    #[error("player position must contain only finite values")]
    NonFinitePlayerPosition,
    #[error("relative hit position must contain only finite values")]
    NonFiniteRelativeHit,
    #[error("block runtime ID {0} exceeds protocol-2168's uint32 wire range")]
    BlockRuntimeIdOutOfRange(u64),
    #[error(transparent)]
    InvalidSelectedItem(#[from] InventoryPacketError),
}

/// Builds a protocol-2168 player-input click-block transaction.
///
/// The packet carries no legacy slot records and no inventory actions. The required transaction
/// and action presence markers are set, prediction is `Failure`, and cooldown is `Off`, matching
/// the pinned public wire fixture. Finite relative-hit components are constrained to the block's
/// local `[0, 1]` coordinate range.
pub fn click_block_packet(
    request: BlockUseRequest,
    session: &BedrockSession,
) -> Result<crate::Packet, BlockUsePacketError> {
    if request.face > 5 {
        return Err(BlockUsePacketError::InvalidFace(request.face));
    }
    if request.selected_slot >= 9 {
        return Err(BlockUsePacketError::InvalidSelectedSlot(
            request.selected_slot,
        ));
    }
    if !request.player_position.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFinitePlayerPosition);
    }
    if !request.relative_hit.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFiniteRelativeHit);
    }
    let wire_runtime_id = u32::try_from(request.block_runtime_id)
        .map_err(|_| BlockUsePacketError::BlockRuntimeIdOutOfRange(request.block_runtime_id))?;
    // Valentine exposes the varuint32 carrier as `i32`; preserve all 32 wire bits.
    let target_block_id = i32::from_ne_bytes(wire_runtime_id.to_ne_bytes());
    let [x, y, z] = request.block_position;
    let [from_x, from_y, from_z] = request.player_position;
    let [click_x, click_y, click_z] = request.relative_hit;
    let item = request
        .selected_item
        .into_vendor_item(session.shield_item_id)?;

    Ok(InventoryTransactionPacket {
        legacy_request_id: TypedClientNetIdStructItemStackLegacyRequestIdTagInt32T0 { id: 0 },
        legacy_set_item_slots: None,
        constant_2: true,
        transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
            ItemUseInventoryTransaction {
                actions: InventoryTransaction {
                    constant_0: true,
                    actions: Vec::new(),
                },
                // The generated spelling `Place` is the protocol-2168 discriminant that the
                // pinned public implementation calls click-block.
                action_type: ItemUseInventoryTransactionActionType::Place,
                trigger_type: ItemUseInventoryTransactionTriggerType::PlayerInput,
                position: BlockPos { x, y, z },
                face: request.face,
                slot: i32::from(request.selected_slot),
                item,
                from_position: Vec3 {
                    x: from_x,
                    y: from_y,
                    z: from_z,
                },
                click_position: Vec3 {
                    x: click_x.clamp(0.0, 1.0),
                    y: click_y.clamp(0.0, 1.0),
                    z: click_z.clamp(0.0, 1.0),
                },
                target_block_id,
                client_interact_prediction:
                    ItemUseInventoryTransactionClientInteractPrediction::Failure,
                client_cooldown_state: ItemUseInventoryTransactionClientCooldownState::Off,
            },
        )),
    }
    .into())
}
