use bedrock_protocol_1_26_51::enums::{
    AnimateAction, ContainerEnumName, HandSlot, InteractAction, InventorySourceType,
    ItemReleaseInventoryTransactionActionType, ItemUseInventoryTransactionActionType,
    ItemUseInventoryTransactionClientCooldownState,
    ItemUseInventoryTransactionPredictedResult as ItemUseInventoryTransactionClientInteractPrediction,
    ItemUseInventoryTransactionTriggerType, ItemUseOnActorInventoryTransactionActionType,
    PlayerActionType,
};
use bedrock_protocol_1_26_51::packets::{
    self, Animate, Interact, InventoryTransaction, PlayerAction,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, BlockPos, InventoryAction, InventorySource, InventoryTransactionData,
    InventoryTransactionPacketData, ItemStackLegacyRequestID, ItemUseInventoryTransaction,
    LegacySetSlot,
};
use bedrock_protocol_1_26_51::wire;
use thiserror::Error;

use crate::{InventoryPacketError, VerifiedNetworkItemStack};

/// The logical hand using an item, independent of the selected hotbar slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemUseHand {
    Mainhand,
    Offhand,
}

impl ItemUseHand {
    /// Selects the transaction slot, validating the hotbar only for the main hand.
    fn transaction_slot(self, selected_slot: u8) -> Result<u8, BlockUsePacketError> {
        match self {
            Self::Mainhand if selected_slot >= crate::HOTBAR_SLOT_COUNT => {
                Err(BlockUsePacketError::InvalidSelectedSlot(selected_slot))
            }
            Self::Mainhand => Ok(selected_slot),
            Self::Offhand => Ok(0),
        }
    }

    /// Converts the logical hand to the packet codec's hand enum.
    const fn vendor(self) -> HandSlot {
        match self {
            Self::Mainhand => HandSlot::Mainhand,
            Self::Offhand => HandSlot::Offhand,
        }
    }
}

/// All authoritative state needed to encode one click-block transaction.
///
/// Reach, ray selection, block identity, and selected-item authority belong to the caller. This
/// type deliberately does not infer or mutate any of them.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockUseRequest {
    pub hand: ItemUseHand,
    pub block_position: [i32; 3],
    pub face: u8,
    /// The selected hotbar slot; offhand transactions use slot zero instead.
    pub selected_slot: u8,
    /// The authoritative stack in `hand`, supplied by the caller.
    pub selected_item: VerifiedNetworkItemStack,
    pub player_position: [f32; 3],
    pub relative_hit: [f32; 3],
    pub block_runtime_id: u64,
}

/// The two public Bedrock item-use-on-actor actions supported by this builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorUseAction {
    Attack,
    Interact,
}

/// Authoritative wire inputs for one Bedrock item-use-on-actor transaction.
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
    #[error("predicted offhand slot changes are not implemented")]
    UnsupportedOffhandPrediction,
    #[error("offhand item release is not implemented")]
    UnsupportedOffhandRelease,
    #[error("player position must contain only finite values")]
    NonFinitePlayerPosition,
    #[error("relative hit position must contain only finite values")]
    NonFiniteRelativeHit,
    #[error("relative hit position must be within the unit block for destruction")]
    RelativeHitOutOfRange,
    #[error("block runtime ID {0} exceeds the uint32 wire range")]
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

/// Builds a Bedrock player-input click-block transaction.
///
/// The packet carries no legacy slot records and no inventory actions. The required transaction
/// and action presence markers are set, prediction is `Failure`, and cooldown is `Off`, matching
/// the pinned public wire fixture. Click coordinates retain the held action's hit point.
pub fn click_block_packet(request: BlockUseRequest) -> Result<crate::Packet, BlockUsePacketError> {
    block_use_packet(request, ItemUseInventoryTransactionActionType::Place)
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
/// Offhand requests require `change` to be absent until offhand prediction is implemented.
pub fn click_block_transaction_packet(
    request: BlockUseRequest,
    trigger: ItemUseTrigger,
    predicted_success: bool,
    change: Option<PredictedSlotChange>,
) -> Result<crate::Packet, BlockUsePacketError> {
    let slot = request.selected_slot;
    let hand = request.hand;
    let mut transaction =
        item_use_transaction(request, ItemUseInventoryTransactionActionType::Place)?;
    transaction.trigger_type = match trigger {
        ItemUseTrigger::PlayerInput => ItemUseInventoryTransactionTriggerType::PlayerInput,
        ItemUseTrigger::SimulationTick => ItemUseInventoryTransactionTriggerType::SimulationTick,
    };
    if predicted_success {
        transaction.client_interact_prediction =
            ItemUseInventoryTransactionClientInteractPrediction::Success;
    }
    let SlotChangeParts {
        legacy_request_id,
        legacy_set_item_slots,
        actions,
    } = slot_change_parts(hand, slot, change)?;
    transaction.actions.actions = actions;
    Ok(crate::Packet::new(InventoryTransaction {
        legacy_request_id: ItemStackLegacyRequestID(legacy_request_id),
        legacy_set_item_slots,
        transaction: InventoryTransactionPacketData::ItemUseInventoryTransaction(Box::new(
            transaction,
        )),
    }))
}

/// Builds a Bedrock destroy-block transaction.
///
/// The packet carries no legacy slot records and no inventory actions. Its wire action is the
/// public break-block semantic (discriminant 2), prediction is `Failure`, and cooldown is `Off`.
/// The trigger is `Unknown`; click coordinates and target block runtime ID are zero.
/// Reach, mining progress, and permission checks remain caller-owned; this function only validates
/// and encodes one already-authorised block interaction.
pub fn destroy_block_packet(
    request: BlockUseRequest,
) -> Result<crate::Packet, BlockUsePacketError> {
    block_use_packet(request, ItemUseInventoryTransactionActionType::Destroy)
}

fn block_use_packet(
    request: BlockUseRequest,
    action_type: ItemUseInventoryTransactionActionType,
) -> Result<crate::Packet, BlockUsePacketError> {
    let transaction = item_use_transaction(request, action_type)?;
    Ok(crate::Packet::new(InventoryTransaction {
        legacy_request_id: ItemStackLegacyRequestID(0),
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketData::ItemUseInventoryTransaction(Box::new(
            transaction,
        )),
    }))
}

/// Validates one block-use request and encodes it as the shared item-use
/// transaction body used by both the standalone `InventoryTransaction` packet
/// and the transaction embedded in `PlayerAuthInput`.
/// The action list is empty. The caller supplies the hand's authoritative stack.
pub(crate) fn item_use_transaction(
    request: BlockUseRequest,
    action_type: ItemUseInventoryTransactionActionType,
) -> Result<ItemUseInventoryTransaction, BlockUsePacketError> {
    if request.face > 5 {
        return Err(BlockUsePacketError::InvalidFace(request.face));
    }
    let slot = request.hand.transaction_slot(request.selected_slot)?;
    if !request.player_position.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFinitePlayerPosition);
    }
    if !request.relative_hit.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFiniteRelativeHit);
    }
    if action_type == ItemUseInventoryTransactionActionType::Destroy
        && !request
            .relative_hit
            .into_iter()
            .all(|axis| (0.0..=1.0).contains(&axis))
    {
        return Err(BlockUsePacketError::RelativeHitOutOfRange);
    }
    let wire_runtime_id = u32::try_from(request.block_runtime_id)
        .map_err(|_| BlockUsePacketError::BlockRuntimeIdOutOfRange(request.block_runtime_id))?;
    let item = request.selected_item.into_transaction_vendor_item()?;
    let destroying = action_type == ItemUseInventoryTransactionActionType::Destroy;

    Ok(ItemUseInventoryTransaction {
        actions: InventoryTransactionData {
            actions: Vec::new(),
        },
        action_type,
        trigger_type: if destroying {
            ItemUseInventoryTransactionTriggerType::Unknown
        } else {
            ItemUseInventoryTransactionTriggerType::PlayerInput
        },
        position: block_pos(request.block_position),
        face: wire::U8(request.face),
        slot: wire::ZigZag32(i32::from(slot)),
        hand: request.hand.vendor(),
        item,
        from_position: request.player_position.into(),
        click_position: if destroying {
            [0.0; 3].into()
        } else {
            request.relative_hit.into()
        },
        target_block_id: wire::VarUInt(if destroying { 0 } else { wire_runtime_id }),
        client_interact_prediction: ItemUseInventoryTransactionClientInteractPrediction::Failure,
        client_cooldown_state: ItemUseInventoryTransactionClientCooldownState::Off,
    })
}

/// The held stack and pose one air use or release reports.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldItemRequest {
    pub hand: ItemUseHand,
    /// The selected hotbar slot; offhand transactions use slot zero instead.
    pub selected_slot: u8,
    /// The authoritative stack in `hand`, supplied by the caller.
    pub selected_item: VerifiedNetworkItemStack,
    pub player_position: [f32; 3],
}

/// The selected slot's stack before and after a locally admitted throw or placement.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictedSlotChange {
    /// The negative client legacy request id `to` carries; unused when `to` is empty.
    pub legacy_request_id: i32,
    pub from: VerifiedNetworkItemStack,
    pub to: VerifiedNetworkItemStack,
}

/// Builds the inventory delta BDS requires before accepting `BookEdit` for a
/// fresh writable book.
///
/// Vanilla initialises a new book & quill with an air use whose `NewItem`
/// carries an empty `pages` list; without it BDS drops later edits and a
/// reopened book reads empty. `None` when the stack is empty or already
/// carries `pages`.
#[must_use]
pub fn writable_book_init_change(
    from: &VerifiedNetworkItemStack,
    legacy_request_id: i32,
) -> Option<PredictedSlotChange> {
    if from.is_empty() || !crate::book_needs_init(from.extra_data()) {
        return None;
    }
    let extra = crate::book_init_extra(from.extra_data())?;
    let digest: [u8; 32] = {
        use sha2::Digest;
        sha2::Sha256::digest(&extra).into()
    };
    let stack = crate::NetworkItemStack {
        network_id: from.network_id(),
        metadata: from.metadata(),
        stack_network_id: from.stack_network_id(),
        count: from.count(),
        nbt_digest: digest,
        block_runtime_id: from.block_runtime_id(),
        extra_data: extra,
    };
    let to = VerifiedNetworkItemStack::try_new(stack, digest).ok()?;
    Some(PredictedSlotChange {
        legacy_request_id,
        from: from.clone(),
        to,
    })
}

/// The wire form of an absolute block position.
fn block_pos([x, y, z]: [i32; 3]) -> BlockPos {
    BlockPos {
        x: wire::ZigZag32(x),
        y: wire::ZigZag32(y),
        z: wire::ZigZag32(z),
    }
}

/// Validates the held pose and resolves the slot for the requested hand.
fn held_item_parts(
    request: &HeldItemRequest,
) -> Result<(wire::ZigZag32, [f32; 3]), BlockUsePacketError> {
    let slot = request.hand.transaction_slot(request.selected_slot)?;
    if !request.player_position.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFinitePlayerPosition);
    }
    Ok((wire::ZigZag32(i32::from(slot)), request.player_position))
}

struct SlotChangeParts {
    legacy_request_id: i32,
    legacy_set_item_slots: Option<Vec<LegacySetSlot>>,
    actions: Vec<InventoryAction>,
}

/// Records the inventory delta and the legacy request for a changed, nonempty result.
fn slot_change_parts(
    hand: ItemUseHand,
    slot: u8,
    change: Option<PredictedSlotChange>,
) -> Result<SlotChangeParts, BlockUsePacketError> {
    if hand == ItemUseHand::Offhand && change.is_some() {
        return Err(BlockUsePacketError::UnsupportedOffhandPrediction);
    }
    let mut legacy_request_id = 0;
    let mut legacy_set_item_slots = None;
    let mut actions = Vec::new();
    if let Some(change) = change {
        // Vanilla stamps and records only a non-empty result.
        if !change.to.is_empty() && change.legacy_request_id < 0 {
            legacy_request_id = change.legacy_request_id;
            legacy_set_item_slots = Some(vec![LegacySetSlot {
                container_enum: ContainerEnumName::Inventorycontainer,
                slots: vec![slot].into(),
            }]);
        }
        actions.push(InventoryAction {
            source: InventorySource {
                source_type: InventorySourceType::ContainerInventory,
                container_id: Some(wire::I8(0)),
                bit_flags: None,
            },
            slot: wire::VarUInt(u32::from(slot)),
            from_item: change.from.into_transaction_vendor_item()?,
            to_item: change.to.into_transaction_vendor_item()?,
        });
    }
    Ok(SlotChangeParts {
        legacy_request_id,
        legacy_set_item_slots,
        actions,
    })
}

/// Builds the click-air transaction vanilla sends on an air use: zero block and
/// click positions, face 255, unset trigger and a failure prediction. A `change` becomes the
/// inventory action and legacy set-slot request vanilla records while the use runs.
/// Offhand requests require `change` to be absent until offhand prediction is implemented.
pub fn click_air_packet(
    request: HeldItemRequest,
    change: Option<PredictedSlotChange>,
) -> Result<crate::Packet, BlockUsePacketError> {
    let (slot, from_position) = held_item_parts(&request)?;
    let item = request.selected_item.into_transaction_vendor_item()?;
    let SlotChangeParts {
        legacy_request_id,
        legacy_set_item_slots,
        actions,
    } = slot_change_parts(request.hand, request.selected_slot, change)?;
    Ok(crate::Packet::new(InventoryTransaction {
        legacy_request_id: ItemStackLegacyRequestID(legacy_request_id),
        legacy_set_item_slots,
        transaction: InventoryTransactionPacketData::ItemUseInventoryTransaction(Box::new(
            ItemUseInventoryTransaction {
                actions: InventoryTransactionData { actions },
                action_type: ItemUseInventoryTransactionActionType::Use,
                trigger_type: ItemUseInventoryTransactionTriggerType::Unknown,
                position: block_pos([0; 3]),
                face: wire::U8(u8::MAX),
                slot,
                hand: request.hand.vendor(),
                item,
                from_position: from_position.into(),
                click_position: [0.0; 3].into(),
                target_block_id: wire::VarUInt(0),
                client_interact_prediction:
                    ItemUseInventoryTransactionClientInteractPrediction::Failure,
                client_cooldown_state: ItemUseInventoryTransactionClientCooldownState::Off,
            },
        )),
    }))
}

/// Reports an item-directed attack, including its world-space aim and current cooldown.
pub fn use_item_as_attack_packet(
    request: HeldItemRequest,
    direction: [f32; 3],
    on_cooldown: bool,
) -> Result<crate::Packet, BlockUsePacketError> {
    if !direction.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFiniteRelativeHit);
    }
    let click =
        std::array::from_fn::<_, 3, _>(|axis| request.player_position[axis] + direction[axis]);
    if !click.into_iter().all(f32::is_finite) {
        return Err(BlockUsePacketError::NonFiniteRelativeHit);
    }
    let mut packet = click_air_packet(request, None)?;
    let packets::Packet::InventoryTransaction(transaction) = &mut *packet.data else {
        unreachable!("click-air builder always produces an inventory transaction");
    };
    let InventoryTransactionPacketData::ItemUseInventoryTransaction(transaction) =
        &mut transaction.transaction
    else {
        unreachable!("click-air builder always produces an item-use transaction");
    };
    transaction.action_type = ItemUseInventoryTransactionActionType::UseAsAttack;
    transaction.face = wire::U8(0);
    transaction.click_position = click.into();
    transaction.client_cooldown_state = if on_cooldown {
        ItemUseInventoryTransactionClientCooldownState::On
    } else {
        ItemUseInventoryTransactionClientCooldownState::Off
    };
    Ok(packet)
}

/// Native action aim rotation runs for actor attacks and held-item releases.
#[must_use]
pub fn is_aim_assist_rotation_action(packet: &crate::Packet) -> bool {
    let packets::Packet::InventoryTransaction(packet) = &*packet.data else {
        return false;
    };
    match &packet.transaction {
        InventoryTransactionPacketData::ItemReleaseInventoryTransaction { action_type, .. } => {
            *action_type == ItemReleaseInventoryTransactionActionType::Release
        }
        InventoryTransactionPacketData::ItemUseOnActorInventoryTransaction {
            action_type, ..
        } => *action_type == ItemUseOnActorInventoryTransactionActionType::Attack,
        InventoryTransactionPacketData::ItemUseInventoryTransaction(transaction) => {
            transaction.action_type == ItemUseInventoryTransactionActionType::UseAsAttack
        }
        _ => false,
    }
}

/// Builds the release-item transaction vanilla sends when the use button
/// goes up; a use that runs out completes without a packet from the client.
/// Only main-hand releases are currently supported.
pub fn release_item_packet(request: HeldItemRequest) -> Result<crate::Packet, BlockUsePacketError> {
    if request.hand == ItemUseHand::Offhand {
        return Err(BlockUsePacketError::UnsupportedOffhandRelease);
    }
    let (slot, from_position) = held_item_parts(&request)?;
    let item = request.selected_item.into_transaction_vendor_item()?;
    Ok(crate::Packet::new(InventoryTransaction {
        legacy_request_id: ItemStackLegacyRequestID(0),
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketData::ItemReleaseInventoryTransaction {
            actions: InventoryTransactionData {
                actions: Vec::new(),
            },
            action_type: ItemReleaseInventoryTransactionActionType::Release,
            slot,
            item,
            from_position: from_position.into(),
        },
    }))
}

/// Builds a Bedrock attack or interact transaction for an already-selected actor.
///
/// The packet carries a zero legacy request ID, no legacy slots, and no inventory actions. Both
/// required presence markers are set. Runtime IDs preserve the complete public `u64` varlong wire
/// domain; zero is rejected because it cannot identify a target actor.
pub fn use_actor_packet(request: ActorUseRequest) -> Result<crate::Packet, ActorUsePacketError> {
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
    let item = request.selected_item.into_transaction_vendor_item()?;

    Ok(crate::Packet::new(InventoryTransaction {
        legacy_request_id: ItemStackLegacyRequestID(0),
        legacy_set_item_slots: None,
        transaction: InventoryTransactionPacketData::ItemUseOnActorInventoryTransaction {
            actions: InventoryTransactionData {
                actions: Vec::new(),
            },
            runtime_id: ActorRuntimeID(actor_runtime_id),
            action_type,
            slot: wire::ZigZag32(i32::from(request.selected_slot)),
            item,
            from_position: request.player_position.into(),
            hit_position: request.hit_position.into(),
        },
    }))
}

/// Why the local arm swung, as carried by an outbound swing animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwingSource {
    Build,
    Mine,
    Interact,
    Attack,
    UseItem,
    ThrowItem,
    DropItem,
}

impl SwingSource {
    // The vanilla client sends the lowercase name; servers that match it exactly drop the
    // connection on any other casing.
    const fn wire_name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Mine => "mine",
            Self::Interact => "interact",
            Self::Attack => "attack",
            Self::UseItem => "useitem",
            Self::ThrowItem => "throwitem",
            Self::DropItem => "dropitem",
        }
    }
}

/// Starts the first successful held block use with its clicked and resulting cells.
#[must_use]
pub fn start_item_use_on_packet(
    local_runtime_id: u64,
    clicked: [i32; 3],
    destination: [i32; 3],
    face: u8,
) -> crate::Packet {
    crate::Packet::new(PlayerAction {
        player_runtime_id: ActorRuntimeID(local_runtime_id),
        action: PlayerActionType::Startitemuseon,
        block_position: block_pos(clicked),
        result_pos: block_pos(destination),
        face: wire::ZigZag32(i32::from(face)),
    })
}

/// Ends a successful held block use at its last resulting cell.
#[must_use]
pub fn stop_item_use_on_packet(local_runtime_id: u64, destination: [i32; 3]) -> crate::Packet {
    crate::Packet::new(PlayerAction {
        player_runtime_id: ActorRuntimeID(local_runtime_id),
        action: PlayerActionType::Stopitemuseon,
        block_position: block_pos(destination),
        result_pos: block_pos([0; 3]),
        face: wire::ZigZag32(0),
    })
}

/// Requests that the server destroy the targeted block as a creative player action.
#[must_use]
pub fn creative_destroy_block_packet(
    local_runtime_id: u64,
    block_position: [i32; 3],
    face: u8,
) -> crate::Packet {
    crate::Packet::new(PlayerAction {
        player_runtime_id: ActorRuntimeID(local_runtime_id),
        action: PlayerActionType::Creativedestroyblock,
        block_position: block_pos(block_position),
        result_pos: block_pos([0; 3]),
        face: wire::ZigZag32(i32::from(face)),
    })
}

/// Builds the PlayerAction that asks the server to wake the local player.
#[must_use]
pub fn stop_sleeping_packet(local_runtime_id: u64) -> crate::Packet {
    crate::Packet::new(PlayerAction {
        player_runtime_id: ActorRuntimeID(local_runtime_id),
        action: PlayerActionType::Stopsleeping,
        block_position: block_pos([0; 3]),
        result_pos: block_pos([0; 3]),
        face: wire::ZigZag32(0),
    })
}

/// Requests release from a vehicle using the rider's current native actor position.
pub fn stop_riding_packet(
    mount_runtime_id: u64,
    player_position: [f32; 3],
) -> Result<crate::Packet, ActorUsePacketError> {
    if mount_runtime_id == 0 {
        return Err(ActorUsePacketError::InvalidActorRuntimeId);
    }
    if !player_position.into_iter().all(f32::is_finite) {
        return Err(ActorUsePacketError::NonFinitePlayerPosition);
    }
    Ok(crate::Packet::new(Interact {
        action: InteractAction::Stopriding,
        target_runtime_id: ActorRuntimeID(mount_runtime_id),
        position: Some(player_position.into()),
    }))
}

/// Builds an entity-hover update from a retained world-space pick, or a positionless clear.
#[must_use]
pub fn entity_hover_packet(target: Option<(u64, [f32; 3])>) -> crate::Packet {
    crate::Packet::new(Interact {
        action: InteractAction::Interactupdate,
        target_runtime_id: ActorRuntimeID(target.map_or(0, |(id, _)| id)),
        position: target.map(|(_, point)| point.into()),
    })
}

/// Builds the local player's arm-swing animation packet.
#[must_use]
pub fn swing_arm_packet(local_runtime_id: u64, source: SwingSource) -> crate::Packet {
    crate::Packet::new(Animate {
        action: AnimateAction::Swing,
        target_actor_runtime_id: ActorRuntimeID(local_runtime_id),
        data: wire::F32LE(0.0),
        swing_source: Some(wire::Str::from_static(source.wire_name())),
    })
}
