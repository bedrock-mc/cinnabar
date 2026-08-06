use std::{collections::HashSet, sync::Arc};

use bytes::{Buf, Bytes, BytesMut};
use sha2::{Digest, Sha256};
use thiserror::Error;
use valentine::bedrock::{
    codec::{BedrockCodec, BedrockSized, Nbt},
    version::v1_26_40::{
        ActorRuntimeId, AnimateEntityPacket, AnimatePacket, AnimatePacketAction, ItemDataItemVersion,
        ItemRegistryPacket, MobEquipmentPacket,
    },
};

/// The single item shape 1.26.40 puts on the wire.
///
/// Protocol 1001 modelled three separate item encodings (`Item`, `ItemNew`,
/// `ItemV4`) plus a `ShieldItemId`-discriminated extra-data union, because the
/// prismarine schema described each call site independently. BDS has one
/// descriptor whose trailing user data is an opaque length-prefixed buffer, so
/// the shield ID is no longer needed to decode an item.
type ItemStackDescriptor =
    valentine::bedrock::version::v1_26_40::CerealizerNetworkItemStackDescriptorSerializedData;

/// Number of hotbar slots on the vanilla survival hotbar.
pub const HOTBAR_SLOT_COUNT: u8 = 9;

/// Builds the vanilla outbound packet announcing a local hotbar-slot selection.
///
/// The vanilla Bedrock client owns hotbar-slot selection locally and notifies the server with a
/// `MobEquipment` packet against the inventory window (`PlayerHotbar` is server->client and is not
/// what a client sends). Servers validate only the 0-8 slot range; the held item is reconciled if
/// it disagrees (Dragonfly's `VerifySlot` re-syncs rather than disconnecting), so an empty item is
/// safe when inventory contents are not tracked. `runtime_id` must be the local player's
/// StartGame-assigned runtime id — servers reject a foreign runtime id on this packet.
#[must_use]
pub fn select_hotbar_slot_packet(runtime_id: u64, slot: u8) -> crate::Packet {
    let slot = slot.min(HOTBAR_SLOT_COUNT - 1);
    MobEquipmentPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as i64,
        },
        item: ItemStackDescriptor::default(),
        slot,
        selected_slot: slot,
        // The inventory container. 1.26.40 carries the raw ID rather than a
        // named WindowId enum.
        container_id: 0,
    }
    .into()
}

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

/// Reads the vanilla `Damage` integer from a stack's retained extra data.
///
/// The extra blob is the validated wire encoding of the item user data; it is
/// decoded through the same generated types that produced it and the fixed
/// little-endian NBT is walked only at the root level, where the vanilla
/// client stores durability damage. Anything malformed or absent reads as
/// `None` — presentation simply skips the durability bar.
#[must_use]
pub fn item_stack_damage(stack: &NetworkItemStack) -> Option<u32> {
    if stack.extra_data.is_empty() {
        return None;
    }
    let nbt = decode_extra_nbt(&stack.extra_data)?;
    root_damage_tag(&nbt)
}

/// Extracts the root NBT compound from an item's user-data buffer.
///
/// 1.26.40 hands the buffer over verbatim instead of modelling its interior, so
/// the leading header is read here. gophertunnel's `Writer.itemUserData`
/// (`minecraft/protocol/writer.go`) writes an `int16` that is `-1` when a
/// compound follows and `0` when none does; when it is `-1` a `uint8` version of
/// `1` follows and then the compound in *fixed* little-endian NBT. The
/// `canPlaceOn` / `canBreak` lists and the shield blocking tick trail the
/// compound, which is why only the root level is walked.
fn decode_extra_nbt(extra: &[u8]) -> Option<Bytes> {
    const HEADER_LEN: usize = 3;
    let header = extra.get(..HEADER_LEN)?;
    let marker = i16::from_le_bytes([header[0], header[1]]);
    if marker != -1 || header[2] != 1 {
        return None;
    }
    Some(Bytes::copy_from_slice(&extra[HEADER_LEN..]))
}

/// Walks one fixed little-endian NBT compound root for an integer `Damage`.
fn root_damage_tag(nbt: &[u8]) -> Option<u32> {
    let mut cursor = nbt;
    if read_u8(&mut cursor)? != 10 {
        return None;
    }
    skip_le_string(&mut cursor)?;
    loop {
        let tag = read_u8(&mut cursor)?;
        if tag == 0 {
            return None;
        }
        let name_len = usize::from(read_u16_le(&mut cursor)?);
        let name = cursor.get(..name_len)?;
        let is_damage = tag == 3 && name == b"Damage";
        cursor = cursor.get(name_len..)?;
        if is_damage {
            let value = i32::from_le_bytes(cursor.get(..4)?.try_into().ok()?);
            return u32::try_from(value).ok();
        }
        skip_le_payload(&mut cursor, tag, 0)?;
    }
}

fn read_u8(cursor: &mut &[u8]) -> Option<u8> {
    let value = *cursor.first()?;
    *cursor = cursor.get(1..)?;
    Some(value)
}

fn read_u16_le(cursor: &mut &[u8]) -> Option<u16> {
    let value = u16::from_le_bytes(cursor.get(..2)?.try_into().ok()?);
    *cursor = cursor.get(2..)?;
    Some(value)
}

fn read_i32_le(cursor: &mut &[u8]) -> Option<i32> {
    let value = i32::from_le_bytes(cursor.get(..4)?.try_into().ok()?);
    *cursor = cursor.get(4..)?;
    Some(value)
}

fn skip_le_string(cursor: &mut &[u8]) -> Option<()> {
    let length = usize::from(read_u16_le(cursor)?);
    *cursor = cursor.get(length..)?;
    Some(())
}

const MAX_ITEM_NBT_WALK_DEPTH: u8 = 16;

fn skip_le_payload(cursor: &mut &[u8], tag: u8, depth: u8) -> Option<()> {
    if depth > MAX_ITEM_NBT_WALK_DEPTH {
        return None;
    }
    match tag {
        1 => *cursor = cursor.get(1..)?,
        2 => *cursor = cursor.get(2..)?,
        3 | 5 => *cursor = cursor.get(4..)?,
        4 | 6 => *cursor = cursor.get(8..)?,
        7 => {
            let length = usize::try_from(read_i32_le(cursor)?).ok()?;
            *cursor = cursor.get(length..)?;
        }
        8 => skip_le_string(cursor)?,
        9 => {
            let element = read_u8(cursor)?;
            let count = usize::try_from(read_i32_le(cursor)?).ok()?;
            for _ in 0..count {
                skip_le_payload(cursor, element, depth.checked_add(1)?)?;
            }
        }
        10 => loop {
            let entry = read_u8(cursor)?;
            if entry == 0 {
                break;
            }
            skip_le_string(cursor)?;
            skip_le_payload(cursor, entry, depth.checked_add(1)?)?;
        },
        11 => {
            let count = usize::try_from(read_i32_le(cursor)?).ok()?;
            *cursor = cursor.get(count.checked_mul(4)?..)?;
        }
        12 => {
            let count = usize::try_from(read_i32_le(cursor)?).ok()?;
            *cursor = cursor.get(count.checked_mul(8)?..)?;
        }
        _ => return None,
    }
    Some(())
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

/// Returns the generated vanilla item registry for the pinned Bedrock
/// protocol. Bedrock servers normally send only custom/data-driven item
/// entries; the vanilla client already knows this built-in table and merges
/// the server packet over it.
#[must_use]
pub fn vanilla_item_registry() -> Arc<[ItemRegistryEntry]> {
    // Content registries stay on v1_26_30: the Endstone-derived 1.26.40 crate is
    // wire-only and generates no items.rs table. See the note in world.rs.
    const GENERATED_ITEMS: &str =
        include_str!("../vendor/valentine/bedrock_versions/v1_26_30/src/items.rs");

    let mut entries = Vec::new();
    let mut pending_id = None;
    for line in GENERATED_ITEMS.lines().map(str::trim) {
        if let Some(value) = line
            .strip_prefix("const ID: i32 = ")
            .and_then(|value| value.strip_suffix(';'))
            .and_then(|value| value.parse::<i32>().ok())
        {
            pending_id = Some(value);
            continue;
        }
        let Some(identifier) = line
            .strip_prefix("const STRING_ID: &'static str = \"")
            .and_then(|value| value.strip_suffix("\";"))
        else {
            continue;
        };
        let Some(network_id) = pending_id.take() else {
            continue;
        };
        entries.push(ItemRegistryEntry {
            identifier: Arc::from(identifier),
            network_id,
            component_based: false,
            version: ItemRegistryVersion::Legacy,
            component_digest: [0; 32],
        });
    }
    Arc::from(entries)
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

/// One MobArmorEquipment update: the actor's five authoritative armor stacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmorEquipmentEvent {
    pub actor_runtime_id: u64,
    pub helmet: NetworkItemStack,
    pub chestplate: NetworkItemStack,
    pub leggings: NetworkItemStack,
    pub boots: NetworkItemStack,
    pub body: NetworkItemStack,
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

/// Normalises the one item descriptor 1.26.40 uses everywhere.
///
/// Protocol 1001 needed a normaliser per encoding (`Item`, `ItemNew`) because
/// the schema modelled them separately, and the "contradictory stack id" checks
/// existed to reject prismarine shapes that could describe a present item and an
/// absent one at once. BDS has a single descriptor whose optional net ID is a
/// plain `Option`, so those contradictions are no longer representable and the
/// two normalisers collapse into this one.
///
/// gophertunnel's `Reader.ItemInstance` (`minecraft/protocol/reader.go`) reads
/// every field unconditionally rather than short-circuiting on a zero ID, so an
/// empty stack is still a fully-formed descriptor and is recognised by its ID
/// alone.
pub(crate) fn normalize_item(item: ItemStackDescriptor) -> Result<NetworkItemStack, ItemPacketError> {
    validate_item_user_data(&item.user_data_buffer)?;
    if item.id == 0 {
        return Ok(NetworkItemStack::empty());
    }
    // An absent net ID is the "no stack tracking" case, which the app models as
    // -1. gophertunnel leaves StackNetworkID at 0 when the bool is unset; the
    // distinction is preserved here rather than collapsing the two.
    let stack_network_id = match item.net_id_variant {
        None => -1,
        Some(stack_id) if stack_id > 0 => stack_id,
        Some(_) => return Err(ItemPacketError::ContradictoryStackId),
    };
    make_stack(
        i32::from(item.id),
        item.auxvalue,
        stack_network_id,
        item.stacksize,
        item.block_runtime_id,
        item.user_data_buffer,
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

/// Bounds an item's user-data buffer and checks the compound it may carry.
///
/// The generated 1.26.40 decoder hands this over as opaque bytes, so the header
/// is interpreted here exactly as gophertunnel's `Writer.itemUserData` writes it
/// (`minecraft/protocol/writer.go`): an `int16` of `-1` introduces a `uint8`
/// version and a fixed little-endian compound, and `0` means no compound.
/// Anything else is malformed. The trailing `canPlaceOn` / `canBreak` lists and
/// the shield blocking tick are not re-validated: they are carried through
/// verbatim and never re-encoded field-by-field, which is what made the
/// protocol-1001 per-string length checks necessary.
fn validate_item_user_data(extra: &[u8]) -> Result<(), ItemPacketError> {
    if extra.len() > MAX_ITEM_EXTRA_BYTES {
        return Err(ItemPacketError::ItemExtraTooLarge {
            bytes: extra.len(),
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    if extra.is_empty() {
        return Ok(());
    }
    let header = extra
        .get(..2)
        .ok_or(ItemPacketError::InvalidItemNbt)?;
    match i16::from_le_bytes([header[0], header[1]]) {
        0 => Ok(()),
        -1 => {
            let version = *extra.get(2).ok_or(ItemPacketError::InvalidItemNbt)?;
            if version != 1 {
                return Err(ItemPacketError::UnsupportedItemNbtVersion(version));
            }
            // Only the compound is validated. The canPlaceOn/canBreak lists and
            // the shield blocking tick follow it in the same buffer, so unlike
            // the standalone NBT checks trailing bytes here are expected.
            let mut bytes = Bytes::copy_from_slice(&extra[3..]);
            Nbt::decode_little_endian(&mut bytes).map_err(|_| ItemPacketError::InvalidItemNbt)?;
            Ok(())
        }
        _ => Err(ItemPacketError::InvalidItemNbt),
    }
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
    if packet.item_data.len() > MAX_ITEM_REGISTRY_ENTRIES {
        return Err(ItemPacketError::TooManyRegistryEntries {
            count: packet.item_data.len(),
            max: MAX_ITEM_REGISTRY_ENTRIES,
        });
    }
    let mut identifiers = HashSet::with_capacity(packet.item_data.len());
    let mut network_ids = HashSet::with_capacity(packet.item_data.len());
    let mut entries = Vec::with_capacity(packet.item_data.len());
    for item in packet.item_data {
        if item.item_name.len() > MAX_ACTION_IDENTIFIER_BYTES {
            return Err(ItemPacketError::ItemIdentifierTooLong {
                bytes: item.item_name.len(),
                max: MAX_ACTION_IDENTIFIER_BYTES,
            });
        }
        let network_id = i32::from(item.item_id);
        if !identifiers.insert(item.item_name.clone()) || !network_ids.insert(network_id) {
            return Err(ItemPacketError::DuplicateRegistryEntry);
        }
        validate_registry_nbt(&item.item_component_data)?;
        let component_bytes = encode_extra(&item.item_component_data)?;
        let version = match item.item_version {
            ItemDataItemVersion::Legacy => ItemRegistryVersion::Legacy,
            ItemDataItemVersion::DataDriven => ItemRegistryVersion::DataDriven,
            ItemDataItemVersion::None => ItemRegistryVersion::None,
            ItemDataItemVersion::Unknown(value) => ItemRegistryVersion::Unknown(value),
        };
        entries.push(ItemRegistryEntry {
            identifier: Arc::from(item.item_name),
            network_id,
            component_based: item.is_component_based,
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
    let actor_runtime_id = runtime_id(packet.target_runtime_id.actor_runtime_id)?;
    normalize_equipment_parts(
        actor_runtime_id,
        normalize_item(packet.item)?,
        packet.slot,
        packet.selected_slot,
        packet.container_id,
    )
}

pub(crate) fn normalize_empty_equipment(
    actor_runtime_id: u64,
    inventory_slot: u8,
    selected_slot: u8,
    container_id: u8,
) -> Result<EquipmentEvent, ItemPacketError> {
    if actor_runtime_id == 0 {
        return Err(ItemPacketError::InvalidRuntimeId(0));
    }
    normalize_equipment_parts(
        actor_runtime_id,
        NetworkItemStack::empty(),
        inventory_slot,
        selected_slot,
        container_id,
    )
}

fn normalize_equipment_parts(
    actor_runtime_id: u64,
    stack: NetworkItemStack,
    inventory_slot: u8,
    selected_slot: u8,
    container_id: u8,
) -> Result<EquipmentEvent, ItemPacketError> {
    // Handedness comes from the window, not the slot. Servers send non-hotbar
    // or sentinel slot values (e.g. 0xFF) that the client never reads, so the
    // raw slots are retained verbatim rather than rejected.
    let (window_id, handedness) = window_id(container_id);
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
    let kind = match packet.action {
        AnimatePacketAction::Swing => ActorActionKind::SwingArm,
        AnimatePacketAction::WakeUp => ActorActionKind::Wake,
        AnimatePacketAction::CriticalHit => ActorActionKind::CriticalHit,
        AnimatePacketAction::MagicCriticalHit => ActorActionKind::MagicCriticalHit,
        AnimatePacketAction::Unknown(128u8) => ActorActionKind::RowRight,
        AnimatePacketAction::Unknown(129u8) => ActorActionKind::RowLeft,
        AnimatePacketAction::NoAction => ActorActionKind::Ignored { action_id: 0 },
        AnimatePacketAction::Unknown(action_id) => ActorActionKind::Ignored { action_id },
    };
    Ok(ItemActorEvent::Action(ActorActionEvent {
        actor_runtime_ids: Arc::from([runtime_id(packet.target_actor_runtime_id.actor_runtime_id)?]),
        kind,
        data: packet.data,
        swing_source: packet.swing_source.map(Arc::from),
    }))
}

pub(crate) fn normalize_animate_entity(
    packet: AnimateEntityPacket,
) -> Result<ItemActorEvent, ItemPacketError> {
    if packet.m_runtime_ids.is_empty()
        || packet.m_runtime_ids.len() > MAX_ANIMATE_ENTITY_IDS
    {
        return Err(ItemPacketError::InvalidAnimationTargetCount {
            count: packet.m_runtime_ids.len(),
            max: MAX_ANIMATE_ENTITY_IDS,
        });
    }
    validate_text(
        "animation",
        &packet.m_animation,
        MAX_ANIMATION_IDENTIFIER_BYTES,
    )?;
    validate_text(
        "controller",
        &packet.m_controller,
        MAX_ACTION_IDENTIFIER_BYTES,
    )?;
    validate_text(
        "next state",
        &packet.m_next_state,
        MAX_ACTION_IDENTIFIER_BYTES,
    )?;
    validate_text(
        "stop condition",
        &packet.m_stop_expression,
        MAX_ACTION_IDENTIFIER_BYTES,
    )?;
    if !packet.m_blend_out_time.is_finite() {
        return Err(ItemPacketError::NonFiniteActionField("blend_out_time"));
    }
    let mut seen = HashSet::with_capacity(packet.m_runtime_ids.len());
    let actor_runtime_ids = packet
        .m_runtime_ids
        .into_iter()
        .map(|id| runtime_id(id.actor_runtime_id))
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
            animation: Arc::from(packet.m_animation),
            controller: Arc::from(packet.m_controller),
        },
        data: packet.m_blend_out_time,
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

/// Maps a raw container ID to its wire value and the hand it implies.
///
/// Protocol 1001 modelled this as a named `WindowId` enum, so every container
/// had to be enumerated just to get the wire number back. 1.26.40 carries a raw
/// `u8` (gophertunnel's `MobEquipment.WindowID`), so only the two containers
/// that actually imply handedness need naming.
fn window_id(container_id: u8) -> (u8, Option<ActorHandedness>) {
    const INVENTORY: u8 = 0;
    // The offhand container ID, matching gophertunnel's ContainerIDOffhand.
    const OFFHAND: u8 = 119;
    const HOTBAR: u8 = 122;

    let handedness = match container_id {
        INVENTORY | HOTBAR => Some(ActorHandedness::Right),
        OFFHAND => Some(ActorHandedness::Left),
        _ => None,
    };
    (container_id, handedness)
}

#[cfg(test)]
mod hotbar_tests {
    use valentine::bedrock::version::v1_26_40::McpePacketData;

    use super::*;

    #[test]
    fn select_hotbar_slot_packet_builds_a_mob_equipment_selection() {
        let McpePacketData::MobEquipmentPacket(packet) = select_hotbar_slot_packet(4242, 3).data
        else {
            panic!("hotbar selection must build a MobEquipment packet, not PlayerHotbar");
        };
        assert_eq!(packet.target_runtime_id.actor_runtime_id, 4242);
        assert_eq!(packet.slot, 3);
        assert_eq!(packet.selected_slot, 3);
        assert_eq!(packet.container_id, 0);
        // Inventory contents are not tracked, so the held item is empty (air); servers reconcile.
        assert_eq!(packet.item, ItemStackDescriptor::default());
    }

    #[test]
    fn select_hotbar_slot_packet_clamps_out_of_range_slots() {
        let McpePacketData::MobEquipmentPacket(packet) = select_hotbar_slot_packet(1, 200).data
        else {
            panic!("hotbar selection must build a MobEquipment packet");
        };
        assert_eq!(packet.slot, HOTBAR_SLOT_COUNT - 1);
        assert_eq!(packet.selected_slot, HOTBAR_SLOT_COUNT - 1);
    }
}
