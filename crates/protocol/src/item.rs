use std::{collections::HashSet, sync::Arc};

use bytes::{Buf, BytesMut};
use sha2::{Digest, Sha256};
use thiserror::Error;
use valentine::bedrock::{
    codec::{BedrockCodec, BedrockSized, Nbt},
    version::v1_26_30::{
        AnimateEntityPacket, AnimatePacket, AnimatePacketActionId, Item, ItemContentExtra,
        ItemExtraDataWithBlockingTick, ItemExtraDataWithoutBlockingTick,
        ItemExtraDataWithoutBlockingTickNbt, ItemNew, ItemNewExtra, ItemRegistryPacket,
        ItemstatesItemVersion, MobEquipmentPacket, WindowId,
    },
};

pub const MAX_ITEM_REGISTRY_ENTRIES: usize = 16_384;
pub const MAX_ITEM_EXTRA_BYTES: usize = 64 * 1024;
pub const MAX_ANIMATE_ENTITY_IDS: usize = 256;
pub const MAX_ACTION_IDENTIFIER_BYTES: usize = 256;
pub const MAX_ANIMATION_IDENTIFIER_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActorHandedness {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkItemStack {
    pub network_id: i32,
    pub metadata: u32,
    pub stack_network_id: i32,
    pub count: u16,
    pub nbt_digest: [u8; 32],
    pub block_runtime_id: i32,
    pub extra_data: Arc<[u8]>,
}

impl Default for NetworkItemStack {
    fn default() -> Self {
        Self::empty()
    }
}

impl NetworkItemStack {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            network_id: 0,
            metadata: 0,
            stack_network_id: -1,
            count: 0,
            nbt_digest: Sha256::digest([]).into(),
            block_runtime_id: 0,
            extra_data: Arc::from([]),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.network_id == 0 || self.count == 0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ItemRegistryVersion {
    Legacy,
    DataDriven,
    None,
    Unknown(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRegistryEntry {
    pub identifier: Arc<str>,
    pub network_id: i32,
    pub component_based: bool,
    pub version: ItemRegistryVersion,
    pub component_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRegistryEvent {
    pub entries: Arc<[ItemRegistryEntry]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipmentEvent {
    pub actor_runtime_id: u64,
    pub stack: NetworkItemStack,
    pub inventory_slot: i32,
    pub selected_slot: u8,
    pub window_id: u8,
    pub handedness: Option<ActorHandedness>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorActionKind {
    SwingArm,
    Wake,
    CriticalHit,
    MagicCriticalHit,
    RowRight,
    RowLeft,
    Custom {
        animation: Arc<str>,
        controller: Arc<str>,
    },
    Ignored {
        action_id: u8,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorActionEvent {
    pub actor_runtime_ids: Arc<[u64]>,
    pub kind: ActorActionKind,
    pub data: f32,
    pub swing_source: Option<Arc<str>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemActorEvent {
    Registry(ItemRegistryEvent),
    Action(ActorActionEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ItemPacketError {
    #[error("item registry has {count} entries, exceeding {max}")]
    TooManyRegistryEntries { count: usize, max: usize },
    #[error("item identifier has {bytes} UTF-8 bytes, exceeding {max}")]
    ItemIdentifierTooLong { bytes: usize, max: usize },
    #[error("item registry contains duplicate identifier or network ID")]
    DuplicateRegistryEntry,
    #[error("item network ID {0} is invalid")]
    InvalidItemNetworkId(i32),
    #[error("non-empty item network ID has an empty stack count")]
    InvalidItemCount,
    #[error("item stack network ID {0} is invalid")]
    InvalidStackNetworkId(i32),
    #[error("item stack-ID presence marker is contradictory")]
    ContradictoryStackId,
    #[error("item extra data has {bytes} bytes, exceeding {max}")]
    ItemExtraTooLarge { bytes: usize, max: usize },
    #[error("item extra string has {bytes} UTF-8 bytes, exceeding {max}")]
    ItemExtraStringTooLarge { bytes: usize, max: usize },
    #[error("item NBT has unsupported version {0}; expected version 1")]
    UnsupportedItemNbtVersion(u8),
    #[error("item NBT is malformed")]
    InvalidItemNbt,
    #[error("failed to encode validated item data")]
    ItemEncodingFailed,
    #[error("actor runtime ID {0} is invalid")]
    InvalidRuntimeId(i64),
    #[error("selected hotbar slot {0} is outside 0..9")]
    InvalidSelectedSlot(u8),
    #[error("inventory slot {inventory} contradicts selected slot {selected}")]
    ContradictorySlots { inventory: u8, selected: u8 },
    #[error("animation target count {count} is outside 1..={max}")]
    InvalidAnimationTargetCount { count: usize, max: usize },
    #[error("animation target runtime ID {0} occurs more than once")]
    DuplicateAnimationTarget(u64),
    #[error("{field} has {bytes} UTF-8 bytes, exceeding {max}")]
    ActionTextTooLong {
        field: &'static str,
        bytes: usize,
        max: usize,
    },
    #[error("action has non-finite {0}")]
    NonFiniteActionField(&'static str),
}

pub(crate) fn normalize_item(item: Item) -> Result<NetworkItemStack, ItemPacketError> {
    let Some(content) = item.content else {
        if item.network_id == 0 {
            return Ok(NetworkItemStack::empty());
        }
        return Err(ItemPacketError::ContradictoryStackId);
    };
    if item.network_id == 0 {
        return Err(ItemPacketError::ContradictoryStackId);
    }
    let stack_network_id = match (content.has_stack_id, content.stack_id) {
        (0, None) => -1,
        (0, Some(_)) => return Err(ItemPacketError::ContradictoryStackId),
        (1, Some(stack_id)) if stack_id > 0 => stack_id,
        (1, Some(_)) | (2.., Some(_)) => return Err(ItemPacketError::ContradictoryStackId),
        (_, None) => return Err(ItemPacketError::ContradictoryStackId),
    };
    let extra = match &content.extra {
        ItemContentExtra::Default(extra) => {
            validate_extra_without_blocking(extra)?;
            encode_extra(extra)?
        }
        ItemContentExtra::ShieldItemId(extra) => {
            validate_extra_with_blocking(extra)?;
            encode_extra(extra)?
        }
    };
    make_stack(
        item.network_id,
        content.metadata,
        stack_network_id,
        content.count,
        content.block_runtime_id,
        extra,
    )
}

fn normalize_item_new(item: ItemNew) -> Result<NetworkItemStack, ItemPacketError> {
    match &item.extra {
        ItemNewExtra::Default(extra) => validate_extra_without_blocking(extra)?,
        ItemNewExtra::ShieldItemId(extra) => validate_extra_with_blocking(extra)?,
    }
    if item.network_id == 0 {
        if item.count == 0
            && item.metadata == 0
            && item.stack_id.is_none()
            && item.block_runtime_id == 0
            && matches!(
                &item.extra,
                ItemNewExtra::Default(extra)
                    if extra == &ItemExtraDataWithoutBlockingTick::default()
            )
        {
            return Ok(NetworkItemStack::empty());
        }
        return Err(ItemPacketError::ContradictoryStackId);
    }
    let stack_network_id = match item.stack_id {
        None => -1,
        Some(stack_id) if stack_id.empty == 0 && stack_id.id > 0 => stack_id.id,
        Some(_) => return Err(ItemPacketError::ContradictoryStackId),
    };
    let extra = match &item.extra {
        ItemNewExtra::Default(extra) => encode_extra(extra)?,
        ItemNewExtra::ShieldItemId(extra) => encode_extra(extra)?,
    };
    make_stack(
        i32::from(item.network_id),
        item.metadata,
        stack_network_id,
        item.count,
        item.block_runtime_id,
        extra,
    )
}

fn make_stack(
    network_id: i32,
    metadata: i32,
    stack_network_id: i32,
    count: u16,
    block_runtime_id: i32,
    extra: Vec<u8>,
) -> Result<NetworkItemStack, ItemPacketError> {
    if network_id == 0 {
        return Err(ItemPacketError::InvalidItemNetworkId(network_id));
    }
    if count == 0 {
        return Err(ItemPacketError::InvalidItemCount);
    }
    if stack_network_id == 0 || stack_network_id < -1 {
        return Err(ItemPacketError::InvalidStackNetworkId(stack_network_id));
    }
    let metadata = u32::from_ne_bytes(metadata.to_ne_bytes());
    if extra.len() > MAX_ITEM_EXTRA_BYTES {
        return Err(ItemPacketError::ItemExtraTooLarge {
            bytes: extra.len(),
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    Ok(NetworkItemStack {
        network_id,
        metadata,
        stack_network_id,
        count,
        nbt_digest: Sha256::digest(&extra).into(),
        block_runtime_id,
        extra_data: Arc::from(extra),
    })
}

fn encode_extra<T>(value: &T) -> Result<Vec<u8>, ItemPacketError>
where
    T: BedrockCodec<Args = ()> + BedrockSized,
{
    let encoded_size = value.encoded_size();
    if encoded_size > MAX_ITEM_EXTRA_BYTES {
        return Err(ItemPacketError::ItemExtraTooLarge {
            bytes: encoded_size,
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    let mut bytes = BytesMut::with_capacity(encoded_size);
    value
        .encode(&mut bytes)
        .map_err(|_| ItemPacketError::ItemEncodingFailed)?;
    if bytes.len() > MAX_ITEM_EXTRA_BYTES {
        return Err(ItemPacketError::ItemExtraTooLarge {
            bytes: bytes.len(),
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    Ok(bytes.to_vec())
}

fn validate_extra_without_blocking(
    extra: &ItemExtraDataWithoutBlockingTick,
) -> Result<(), ItemPacketError> {
    validate_extra_fields(extra.nbt.as_ref(), &extra.can_place_on, &extra.can_destroy)
}

fn validate_extra_with_blocking(
    extra: &ItemExtraDataWithBlockingTick,
) -> Result<(), ItemPacketError> {
    validate_extra_fields(extra.nbt.as_ref(), &extra.can_place_on, &extra.can_destroy)
}

fn validate_extra_fields(
    nbt: Option<&ItemExtraDataWithoutBlockingTickNbt>,
    can_place_on: &[String],
    can_destroy: &[String],
) -> Result<(), ItemPacketError> {
    if let Some(nbt) = nbt {
        if nbt.version != 1 {
            return Err(ItemPacketError::UnsupportedItemNbtVersion(nbt.version));
        }
        validate_item_extra_nbt(&nbt.nbt)?;
    }
    for value in can_place_on.iter().chain(can_destroy) {
        if value.len() > i16::MAX as usize {
            return Err(ItemPacketError::ItemExtraStringTooLarge {
                bytes: value.len(),
                max: i16::MAX as usize,
            });
        }
    }
    Ok(())
}

fn validate_item_extra_nbt(nbt: &Nbt) -> Result<(), ItemPacketError> {
    let mut bytes = nbt.0.clone();
    Nbt::decode_little_endian(&mut bytes).map_err(|_| ItemPacketError::InvalidItemNbt)?;
    if bytes.has_remaining() {
        return Err(ItemPacketError::InvalidItemNbt);
    }
    Ok(())
}

fn validate_registry_nbt(nbt: &Nbt) -> Result<(), ItemPacketError> {
    let mut bytes = nbt.0.clone();
    Nbt::decode(&mut bytes, ()).map_err(|_| ItemPacketError::InvalidItemNbt)?;
    if bytes.has_remaining() {
        return Err(ItemPacketError::InvalidItemNbt);
    }
    Ok(())
}

pub(crate) fn normalize_item_registry(
    packet: ItemRegistryPacket,
) -> Result<ItemActorEvent, ItemPacketError> {
    if packet.itemstates.len() > MAX_ITEM_REGISTRY_ENTRIES {
        return Err(ItemPacketError::TooManyRegistryEntries {
            count: packet.itemstates.len(),
            max: MAX_ITEM_REGISTRY_ENTRIES,
        });
    }
    let mut identifiers = HashSet::with_capacity(packet.itemstates.len());
    let mut network_ids = HashSet::with_capacity(packet.itemstates.len());
    let mut entries = Vec::with_capacity(packet.itemstates.len());
    for item in packet.itemstates {
        if item.name.len() > MAX_ACTION_IDENTIFIER_BYTES {
            return Err(ItemPacketError::ItemIdentifierTooLong {
                bytes: item.name.len(),
                max: MAX_ACTION_IDENTIFIER_BYTES,
            });
        }
        let network_id = i32::from(item.runtime_id);
        if !identifiers.insert(item.name.clone()) || !network_ids.insert(network_id) {
            return Err(ItemPacketError::DuplicateRegistryEntry);
        }
        validate_registry_nbt(&item.nbt)?;
        let component_bytes = encode_extra(&item.nbt)?;
        let version = match item.version {
            ItemstatesItemVersion::Legacy => ItemRegistryVersion::Legacy,
            ItemstatesItemVersion::DataDriven => ItemRegistryVersion::DataDriven,
            ItemstatesItemVersion::None => ItemRegistryVersion::None,
            ItemstatesItemVersion::Unknown(value) => ItemRegistryVersion::Unknown(value),
        };
        entries.push(ItemRegistryEntry {
            identifier: Arc::from(item.name),
            network_id,
            component_based: item.component_based,
            version,
            component_digest: Sha256::digest(component_bytes).into(),
        });
    }
    Ok(ItemActorEvent::Registry(ItemRegistryEvent {
        entries: Arc::from(entries),
    }))
}

pub(crate) fn normalize_equipment(
    packet: MobEquipmentPacket,
) -> Result<EquipmentEvent, ItemPacketError> {
    let actor_runtime_id = runtime_id(packet.runtime_entity_id)?;
    normalize_equipment_parts(
        actor_runtime_id,
        normalize_item_new(packet.item)?,
        packet.slot,
        packet.selected_slot,
        packet.window_id,
    )
}

pub(crate) fn normalize_empty_equipment(
    actor_runtime_id: u64,
    inventory_slot: u8,
    selected_slot: u8,
    window: WindowId,
) -> Result<EquipmentEvent, ItemPacketError> {
    if actor_runtime_id == 0 {
        return Err(ItemPacketError::InvalidRuntimeId(0));
    }
    normalize_equipment_parts(
        actor_runtime_id,
        NetworkItemStack::empty(),
        inventory_slot,
        selected_slot,
        window,
    )
}

fn normalize_equipment_parts(
    actor_runtime_id: u64,
    stack: NetworkItemStack,
    inventory_slot: u8,
    selected_slot: u8,
    window: WindowId,
) -> Result<EquipmentEvent, ItemPacketError> {
    if selected_slot >= 9 {
        return Err(ItemPacketError::InvalidSelectedSlot(selected_slot));
    }
    if inventory_slot != selected_slot {
        return Err(ItemPacketError::ContradictorySlots {
            inventory: inventory_slot,
            selected: selected_slot,
        });
    }
    let (window_id, handedness) = window_id(window);
    Ok(EquipmentEvent {
        actor_runtime_id,
        stack,
        inventory_slot: i32::from(inventory_slot),
        selected_slot,
        window_id,
        handedness,
    })
}

pub(crate) fn normalize_animate(packet: AnimatePacket) -> Result<ItemActorEvent, ItemPacketError> {
    if !packet.data.is_finite() {
        return Err(ItemPacketError::NonFiniteActionField("data"));
    }
    if let Some(source) = &packet.swing_source {
        validate_text("swing source", source, MAX_ACTION_IDENTIFIER_BYTES)?;
    }
    let kind = match packet.action_id {
        AnimatePacketActionId::SwingArm => ActorActionKind::SwingArm,
        AnimatePacketActionId::WakeUp => ActorActionKind::Wake,
        AnimatePacketActionId::CriticalHit => ActorActionKind::CriticalHit,
        AnimatePacketActionId::MagicCriticalHit => ActorActionKind::MagicCriticalHit,
        AnimatePacketActionId::UnknownValue(128) => ActorActionKind::RowRight,
        AnimatePacketActionId::UnknownValue(129) => ActorActionKind::RowLeft,
        AnimatePacketActionId::None => ActorActionKind::Ignored { action_id: 0 },
        AnimatePacketActionId::Unknown => ActorActionKind::Ignored { action_id: 2 },
        AnimatePacketActionId::UnknownValue(action_id) => ActorActionKind::Ignored { action_id },
    };
    Ok(ItemActorEvent::Action(ActorActionEvent {
        actor_runtime_ids: Arc::from([runtime_id(packet.runtime_entity_id)?]),
        kind,
        data: packet.data,
        swing_source: packet.swing_source.map(Arc::from),
    }))
}

pub(crate) fn normalize_animate_entity(
    packet: AnimateEntityPacket,
) -> Result<ItemActorEvent, ItemPacketError> {
    if packet.runtime_entity_ids.is_empty()
        || packet.runtime_entity_ids.len() > MAX_ANIMATE_ENTITY_IDS
    {
        return Err(ItemPacketError::InvalidAnimationTargetCount {
            count: packet.runtime_entity_ids.len(),
            max: MAX_ANIMATE_ENTITY_IDS,
        });
    }
    validate_text(
        "animation",
        &packet.animation,
        MAX_ANIMATION_IDENTIFIER_BYTES,
    )?;
    validate_text(
        "controller",
        &packet.controller,
        MAX_ACTION_IDENTIFIER_BYTES,
    )?;
    validate_text(
        "next state",
        &packet.next_state,
        MAX_ACTION_IDENTIFIER_BYTES,
    )?;
    validate_text(
        "stop condition",
        &packet.stop_condition,
        MAX_ACTION_IDENTIFIER_BYTES,
    )?;
    if !packet.blend_out_time.is_finite() {
        return Err(ItemPacketError::NonFiniteActionField("blend_out_time"));
    }
    let mut seen = HashSet::with_capacity(packet.runtime_entity_ids.len());
    let actor_runtime_ids = packet
        .runtime_entity_ids
        .into_iter()
        .map(runtime_id)
        .map(|result| {
            let id = result?;
            if !seen.insert(id) {
                return Err(ItemPacketError::DuplicateAnimationTarget(id));
            }
            Ok(id)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ItemActorEvent::Action(ActorActionEvent {
        actor_runtime_ids: Arc::from(actor_runtime_ids),
        kind: ActorActionKind::Custom {
            animation: Arc::from(packet.animation),
            controller: Arc::from(packet.controller),
        },
        data: packet.blend_out_time,
        swing_source: None,
    }))
}

fn runtime_id(value: i64) -> Result<u64, ItemPacketError> {
    let runtime_id = u64::from_ne_bytes(value.to_ne_bytes());
    (runtime_id != 0)
        .then_some(runtime_id)
        .ok_or(ItemPacketError::InvalidRuntimeId(value))
}

fn validate_text(field: &'static str, text: &str, max: usize) -> Result<(), ItemPacketError> {
    if text.len() > max {
        return Err(ItemPacketError::ActionTextTooLong {
            field,
            bytes: text.len(),
            max,
        });
    }
    Ok(())
}

fn window_id(window: WindowId) -> (u8, Option<ActorHandedness>) {
    let (wire, handedness): (i8, Option<ActorHandedness>) = match window {
        WindowId::DropContents => (-100, None),
        WindowId::Beacon => (-24, None),
        WindowId::TradingOutput => (-23, None),
        WindowId::TradingUseInputs => (-22, None),
        WindowId::TradingInput2 => (-21, None),
        WindowId::TradingInput1 => (-20, None),
        WindowId::EnchantOutput => (-17, None),
        WindowId::EnchantMaterial => (-16, None),
        WindowId::EnchantInput => (-15, None),
        WindowId::AnvilOutput => (-13, None),
        WindowId::AnvilResult => (-12, None),
        WindowId::AnvilMaterial => (-11, None),
        WindowId::ContainerInput => (-10, None),
        WindowId::CraftingUseIngredient => (-5, None),
        WindowId::CraftingResult => (-4, None),
        WindowId::CraftingRemoveIngredient => (-3, None),
        WindowId::CraftingAddIngredient => (-2, None),
        WindowId::None => (-1, None),
        WindowId::Inventory => (0, Some(ActorHandedness::Right)),
        WindowId::First => (1, None),
        WindowId::Last => (100, None),
        WindowId::Offhand => (119, Some(ActorHandedness::Left)),
        WindowId::Armor => (120, None),
        WindowId::Creative => (121, None),
        WindowId::Hotbar => (122, Some(ActorHandedness::Right)),
        WindowId::FixedInventory => (123, None),
        WindowId::Ui => (124, None),
        WindowId::Unknown(value) => (value, None),
    };
    (u8::from_ne_bytes(wire.to_ne_bytes()), handedness)
}
