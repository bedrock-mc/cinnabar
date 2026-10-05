use thiserror::Error;
use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, AnimatePacket, BlockPos, EnumsAnimatePacketPayloadAction,
    EnumsContainerEnumName, EnumsHandSlot, EnumsInventorySourceType,
    EnumsItemReleaseInventoryTransactionActionType,
    EnumsItemUseInventoryTransactionActionType as ItemUseInventoryTransactionActionType,
    EnumsItemUseInventoryTransactionClientCooldownState as ItemUseInventoryTransactionClientCooldownState,
    EnumsItemUseInventoryTransactionPredictedResult as ItemUseInventoryTransactionClientInteractPrediction,
    EnumsItemUseInventoryTransactionTriggerType as ItemUseInventoryTransactionTriggerType,
    EnumsItemUseOnActorInventoryTransactionActionType as ItemUseOnActorInventoryTransactionActionType,
    EnumsPlayerActionType, InventoryAction, InventorySource, InventoryTransaction,
    InventoryTransactionPacket, InventoryTransactionPacketTransaction,
    ItemReleaseInventoryTransaction, ItemUseInventoryTransaction,
    ItemUseOnActorInventoryTransaction, LegacySetSlot, PlayerActionPacket,
    TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0, Vec3,
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

/// The two public protocol-2168 item-use-on-actor actions supported by this builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorUseAction {
    Attack,
    Interact,
}

/// Authoritative wire inputs for one protocol-2168 item-use-on-actor transaction.
///
/// Actor selection, reach, hit testing, abilities, and selected-item authority belong to the
/// caller. This protocol layer only validates and encodes supplied state.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorUseRequest {
    pub actor_runtime_id: u64,
    pub action: ActorUseAction,
    pub selected_slot: u8,
    pub selected_item: VerifiedNetworkItemStack,
    pub player_position: [f32; 3],
    pub hit_position: [f32; 3],
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
    #[error("relative hit position must stay within the block-local [0, 1] range")]
    RelativeHitOutOfRange,
    #[error("block runtime ID {0} exceeds protocol-2168's uint32 wire range")]
    BlockRuntimeIdOutOfRange(u64),
    #[error(transparent)]
    InvalidSelectedItem(#[from] InventoryPacketError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActorUsePacketError {
    #[error("actor runtime ID must be non-zero")]
    InvalidActorRuntimeId,
    #[error("selected hotbar slot {0} is outside 0..=8")]
    InvalidSelectedSlot(u8),
    #[error("player position must contain only finite values")]
    NonFinitePlayerPosition,
    #[error("actor hit position must contain only finite values")]
    NonFiniteHitPosition,
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
    block_use_packet(
        request,
        session,
        ItemUseInventoryTransactionActionType::Place,
    )
}

/// What fired a click-block transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemUseTrigger {
    /// The use press itself.
    PlayerInput,
    /// A repeat while the use button stays held.
    SimulationTick,
}

/// Builds a standalone click-block transaction carrying its trigger and whether the
/// local use succeeded.
pub fn click_block_transaction_packet(
    request: BlockUseRequest,
    trigger: ItemUseTrigger,
    predicted_success: bool,
) -> Result<crate::Packet, BlockUsePacketError> {
    let mut transaction =
        item_use_transaction(request, ItemUseInventoryTransactionActionType::Place)?;
    transaction.trigger_type = match trigger {
        ItemUseTrigger::PlayerInput => ItemUseInventoryTransactionTriggerType::Playerinput,
        ItemUseTrigger::SimulationTick => ItemUseInventoryTransactionTriggerType::Simulationtick,
    };
    if predicted_success {
        transaction.client_interact_prediction =
            ItemUseInventoryTransactionClientInteractPrediction::Success;
    }
    Ok(InventoryTransactionPacket {
        legacy_request_id: TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0 { id: 0 },
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
            transaction,
        )),
    }
    .into())
}

/// Builds a protocol-2168 player-input destroy-block transaction.
///
/// The packet carries no legacy slot records and no inventory actions. Its wire action is the
/// public break-block semantic (discriminant 2), prediction is `Failure`, and cooldown is `Off`.
/// Reach, mining progress, and permission checks remain caller-owned; this function only validates
/// and encodes one already-authorised block interaction.
pub fn destroy_block_packet(
    request: BlockUseRequest,
    session: &BedrockSession,
) -> Result<crate::Packet, BlockUsePacketError> {
    block_use_packet(
        request,
        session,
        ItemUseInventoryTransactionActionType::Destroy,
    )
}

fn block_use_packet(
    request: BlockUseRequest,
    session: &BedrockSession,
    action_type: ItemUseInventoryTransactionActionType,
) -> Result<crate::Packet, BlockUsePacketError> {
    // The pinned 1.26.40 item descriptor carries its user data opaquely, so
    // no session state participates in item encoding; the session parameter
    // is retained only for the public builder signature.
    let _ = session;
    let transaction = item_use_transaction(request, action_type)?;
    Ok(InventoryTransactionPacket {
        legacy_request_id: TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0 { id: 0 },
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
            transaction,
        )),
    }
    .into())
}

/// Validates one block-use request and encodes it as the shared item-use
/// transaction body used by both the standalone `InventoryTransaction` packet
/// and the transaction embedded in `PlayerAuthInput`.
/// The action list is always empty and the hand is always the main hand.
pub(crate) fn item_use_transaction(
    request: BlockUseRequest,
    action_type: ItemUseInventoryTransactionActionType,
) -> Result<ItemUseInventoryTransaction, BlockUsePacketError> {
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
    if !request
        .relative_hit
        .into_iter()
        .all(|component| (0.0..=1.0).contains(&component))
    {
        return Err(BlockUsePacketError::RelativeHitOutOfRange);
    }
    let wire_runtime_id = u32::try_from(request.block_runtime_id)
        .map_err(|_| BlockUsePacketError::BlockRuntimeIdOutOfRange(request.block_runtime_id))?;
    let target_block_id = wire_runtime_id;
    let [x, y, z] = request.block_position;
    let [from_x, from_y, from_z] = request.player_position;
    let [click_x, click_y, click_z] = request.relative_hit;
    // The 1.26.40 item descriptor never consults session state (see
    // `VerifiedNetworkItemStack::into_vendor_item`), so no shield or session
    // identity is fabricated here.
    let item = request.selected_item.into_vendor_item(0)?;

    Ok(ItemUseInventoryTransaction {
        actions: InventoryTransaction {
            actions: Vec::new(),
        },
        action_type,
        trigger_type: ItemUseInventoryTransactionTriggerType::Playerinput,
        position: BlockPos { x, y, z },
        face: request.face,
        slot: i32::from(request.selected_slot),
        hand: EnumsHandSlot::Mainhand,
        item,
        from_position: Vec3 {
            x: from_x,
            y: from_y,
            z: from_z,
        },
        click_position: Vec3 {
            x: click_x,
            y: click_y,
            z: click_z,
        },
        target_block_id,
        client_interact_prediction: ItemUseInventoryTransactionClientInteractPrediction::Failure,
        client_cooldown_state: ItemUseInventoryTransactionClientCooldownState::Off,
    })
}

/// The held stack and pose one air use or release reports.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldItemRequest {
    pub selected_slot: u8,
    pub selected_item: VerifiedNetworkItemStack,
    pub player_position: [f32; 3],
}

/// The selected slot's stack before and after an air use changed it locally (a throw).
#[derive(Debug, Clone, PartialEq)]
pub struct PredictedSlotChange {
    /// The negative client legacy request id `to` carries; unused when `to` is empty.
    pub legacy_request_id: i32,
    pub from: VerifiedNetworkItemStack,
    pub to: VerifiedNetworkItemStack,
}

fn held_item_parts(request: &HeldItemRequest) -> Result<(i32, Vec3), BlockUsePacketError> {
    if request.selected_slot >= 9 {
        return Err(BlockUsePacketError::InvalidSelectedSlot(
            request.selected_slot,
        ));
    }
    if !request.player_position.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFinitePlayerPosition);
    }
    let [x, y, z] = request.player_position;
    Ok((i32::from(request.selected_slot), Vec3 { x, y, z }))
}

/// Builds the click-air transaction vanilla's `GameMode::baseUseItem` sends: zero block and
/// click positions, face 255, unset trigger and a failure prediction. A `change` becomes the
/// inventory action and legacy set-slot request vanilla records while the use runs.
pub fn click_air_packet(
    request: HeldItemRequest,
    change: Option<PredictedSlotChange>,
) -> Result<crate::Packet, BlockUsePacketError> {
    let (slot, from_position) = held_item_parts(&request)?;
    let item = request.selected_item.into_vendor_item(0)?;
    let mut legacy_request_id = 0;
    let mut legacy_set_item_slots = None;
    let mut actions = Vec::new();
    if let Some(change) = change {
        // `setPlayerContainer` stamps and records only a non-empty result.
        if !change.to.is_empty() && change.legacy_request_id < 0 {
            legacy_request_id = change.legacy_request_id;
            legacy_set_item_slots = Some(vec![LegacySetSlot {
                container_enum: EnumsContainerEnumName::Inventorycontainer,
                slots: vec![request.selected_slot],
            }]);
        }
        actions.push(InventoryAction {
            source: InventorySource {
                source_type: EnumsInventorySourceType::Containerinventory,
                container_id: Some(0),
                bit_flags: None,
            },
            slot: u32::from(request.selected_slot),
            from_item: change.from.into_vendor_item(0)?,
            to_item: change.to.into_vendor_item(0)?,
        });
    }
    Ok(InventoryTransactionPacket {
        legacy_request_id: TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0 {
            id: legacy_request_id,
        },
        legacy_set_item_slots,
        transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
            ItemUseInventoryTransaction {
                actions: InventoryTransaction { actions },
                action_type: ItemUseInventoryTransactionActionType::Use,
                trigger_type: ItemUseInventoryTransactionTriggerType::Unknown,
                position: BlockPos { x: 0, y: 0, z: 0 },
                face: u8::MAX,
                slot,
                hand: EnumsHandSlot::Mainhand,
                item,
                from_position,
                click_position: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                target_block_id: 0,
                client_interact_prediction:
                    ItemUseInventoryTransactionClientInteractPrediction::Failure,
                client_cooldown_state: ItemUseInventoryTransactionClientCooldownState::Off,
            },
        )),
    }
    .into())
}

/// Builds the release-item transaction `GameMode::releaseUsingItem` sends when the use button
/// goes up; a use that runs out completes without a packet from the client.
pub fn release_item_packet(request: HeldItemRequest) -> Result<crate::Packet, BlockUsePacketError> {
    let (slot, from_position) = held_item_parts(&request)?;
    let item = request.selected_item.into_vendor_item(0)?;
    Ok(InventoryTransactionPacket {
        legacy_request_id: TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0 { id: 0 },
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(
            Box::new(ItemReleaseInventoryTransaction {
                actions: InventoryTransaction {
                    actions: Vec::new(),
                },
                action_type: EnumsItemReleaseInventoryTransactionActionType::Release,
                slot,
                item,
                from_position,
            }),
        ),
    }
    .into())
}

/// Builds a protocol-2168 attack or interact transaction for an already-selected actor.
///
/// The packet carries a zero legacy request ID, no legacy slots, and no inventory actions. Both
/// required presence markers are set. Runtime IDs preserve the complete public `u64` varlong wire
/// domain; zero is rejected because it cannot identify a target actor.
pub fn use_actor_packet(
    request: ActorUseRequest,
    session: &BedrockSession,
) -> Result<crate::Packet, ActorUsePacketError> {
    if request.actor_runtime_id == 0 {
        return Err(ActorUsePacketError::InvalidActorRuntimeId);
    }
    if request.selected_slot >= 9 {
        return Err(ActorUsePacketError::InvalidSelectedSlot(
            request.selected_slot,
        ));
    }
    if !request.player_position.into_iter().all(f32::is_finite) {
        return Err(ActorUsePacketError::NonFinitePlayerPosition);
    }
    if !request.hit_position.into_iter().all(f32::is_finite) {
        return Err(ActorUsePacketError::NonFiniteHitPosition);
    }

    let actor_runtime_id = request.actor_runtime_id;
    let action_type = match request.action {
        ActorUseAction::Attack => ItemUseOnActorInventoryTransactionActionType::Attack,
        ActorUseAction::Interact => ItemUseOnActorInventoryTransactionActionType::Interact,
    };
    let item = request
        .selected_item
        .into_vendor_item(session.shield_item_id)?;
    let [from_x, from_y, from_z] = request.player_position;
    let [hit_x, hit_y, hit_z] = request.hit_position;

    Ok(InventoryTransactionPacket {
        legacy_request_id: TypedClientNetIdstructItemStackLegacyRequestIdTagint32T0 { id: 0 },
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
            Box::new(ItemUseOnActorInventoryTransaction {
                actions: InventoryTransaction {
                    actions: Vec::new(),
                },
                runtime_id: ActorRuntimeId { actor_runtime_id },
                action_type,
                slot: i32::from(request.selected_slot),
                item,
                from_position: Vec3 {
                    x: from_x,
                    y: from_y,
                    z: from_z,
                },
                hit_position: Vec3 {
                    x: hit_x,
                    y: hit_y,
                    z: hit_z,
                },
            }),
        ),
    }
    .into())
}

/// Why the local arm swung, as carried by an outbound swing animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwingSource {
    Build,
    Mine,
    Interact,
    Attack,
    ThrowItem,
}

impl SwingSource {
    // Vanilla binds the capitalised names; gophertunnel writes lowercase and reads either.
    const fn wire_name(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Mine => "Mine",
            Self::Interact => "Interact",
            Self::Attack => "Attack",
            Self::ThrowItem => "ThrowItem",
        }
    }
}

/// Builds the PlayerAction that asks the server to wake the local player.
#[must_use]
pub fn stop_sleeping_packet(local_runtime_id: u64) -> crate::Packet {
    PlayerActionPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: local_runtime_id,
        },
        action: EnumsPlayerActionType::Stopsleeping,
        block_position: BlockPos { x: 0, y: 0, z: 0 },
        result_pos: BlockPos { x: 0, y: 0, z: 0 },
        face: 0,
    }
    .into()
}

/// Builds the local player's arm-swing animation packet.
#[must_use]
pub fn swing_arm_packet(local_runtime_id: u64, source: SwingSource) -> crate::Packet {
    AnimatePacket {
        action: EnumsAnimatePacketPayloadAction::Swing,
        target_actor_runtime_id: ActorRuntimeId {
            actor_runtime_id: local_runtime_id,
        },
        data: 0.0,
        swing_source: Some(source.wire_name().to_owned()),
    }
    .into()
}
