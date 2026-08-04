//! Server-authoritative entity interaction packets.

use thiserror::Error;
use valentine::bedrock::version::v1_26_30::{
    Action as PlayerAction, BlockCoordinates, InventoryTransactionPacket, ItemV4,
    ItemV4NetIdVariant, ItemV4NetIdVariantType, Transaction, TransactionLegacy,
    TransactionTransactionData, TransactionTransactionDataItemUseOnEntity,
    TransactionTransactionDataItemUseOnEntityActionType, TransactionTransactionType,
};

use crate::{InventoryPacketError, NetworkItemStack, Packet, VerifiedNetworkItemStack};

/// The action encoded by a protocol-1001 `UseItemOnEntity` transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityInteractionAction {
    Interact,
    Attack,
}

#[derive(Debug, Error)]
pub enum CombatPacketError {
    #[error("entity runtime ID {0} is outside the positive signed wire range")]
    InvalidRuntimeId(u64),

    #[error("hotbar slot {0} is outside the vanilla 0..9 range")]
    InvalidHotbarSlot(u8),

    #[error("combat packet contains a non-finite position")]
    NonFinitePosition,

    #[error("held item is not a verified protocol-1001 stack: {0}")]
    InvalidHeldItem(#[source] InventoryPacketError),

    #[error("held item network ID {0} is outside the signed i16 wire range")]
    InvalidItemNetworkId(i32),
}

/// Builds the server-authoritative Bedrock entity interaction transaction.
///
/// The transaction contains the held item and the click point relative to the
/// target's base coordinate. It does not apply local damage, knockback, or
/// inventory changes; those remain server responses.
pub fn use_item_on_entity_packet(
    target_runtime_id: u64,
    action: EntityInteractionAction,
    hotbar_slot: u8,
    held_item: &NetworkItemStack,
    player_position: [f32; 3],
    click_position: [f32; 3],
) -> Result<Packet, CombatPacketError> {
    let entity_runtime_id = signed_runtime_id(target_runtime_id)?;
    if hotbar_slot >= crate::HOTBAR_SLOT_COUNT {
        return Err(CombatPacketError::InvalidHotbarSlot(hotbar_slot));
    }
    if !player_position
        .into_iter()
        .chain(click_position)
        .all(f32::is_finite)
    {
        return Err(CombatPacketError::NonFinitePosition);
    }

    let held_item = item_v4(held_item)?;
    let action_type = match action {
        EntityInteractionAction::Interact => {
            TransactionTransactionDataItemUseOnEntityActionType::Interact
        }
        EntityInteractionAction::Attack => {
            TransactionTransactionDataItemUseOnEntityActionType::Attack
        }
    };
    let transaction = Transaction {
        legacy: TransactionLegacy::default(),
        transaction_type: Some(TransactionTransactionType::ItemUseOnEntity),
        actions: Some(Vec::new()),
        transaction_data: Some(TransactionTransactionData::ItemUseOnEntity(Box::new(
            TransactionTransactionDataItemUseOnEntity {
                entity_runtime_id,
                action_type,
                hotbar_slot: i32::from(hotbar_slot),
                held_item,
                player_pos: vec3(player_position),
                click_pos: vec3(click_position),
            },
        ))),
    };
    Ok(InventoryTransactionPacket { transaction }.into())
}

/// Builds the server-visible action used when an attack press does not hit an
/// entity. A missed swing is still not a local hit or damage prediction.
pub fn missed_swing_packet(runtime_id: u64) -> Result<Packet, CombatPacketError> {
    Ok(valentine::bedrock::version::v1_26_30::PlayerActionPacket {
        runtime_entity_id: signed_runtime_id(runtime_id)?,
        action: PlayerAction::MissedSwing,
        position: BlockCoordinates::default(),
        result_position: BlockCoordinates::default(),
        face: 0,
    }
    .into())
}

fn signed_runtime_id(runtime_id: u64) -> Result<i64, CombatPacketError> {
    let signed = i64::try_from(runtime_id)
        .ok()
        .filter(|runtime_id| *runtime_id > 0)
        .ok_or(CombatPacketError::InvalidRuntimeId(runtime_id))?;
    Ok(signed)
}

fn item_v4(stack: &NetworkItemStack) -> Result<ItemV4, CombatPacketError> {
    let verified = VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
        .map_err(CombatPacketError::InvalidHeldItem)?;
    let network_id = i16::try_from(verified.network_id())
        .map_err(|_| CombatPacketError::InvalidItemNetworkId(verified.network_id()))?;
    let net_id_variant = (verified.stack_network_id() != -1).then_some(ItemV4NetIdVariant {
        type_: ItemV4NetIdVariantType::ItemStackNetId,
        id: verified.stack_network_id(),
    });
    Ok(ItemV4 {
        network_id,
        count: verified.count(),
        metadata: i32::from_ne_bytes(verified.metadata().to_ne_bytes()),
        net_id_variant,
        block_runtime_id: verified.block_runtime_id(),
        extra_data: verified.extra_data().to_vec(),
    })
}

fn vec3(value: [f32; 3]) -> valentine::bedrock::version::v1_26_30::Vec3F {
    valentine::bedrock::version::v1_26_30::Vec3F {
        x: value[0],
        y: value[1],
        z: value[2],
    }
}
