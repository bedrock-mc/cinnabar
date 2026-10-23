use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use bedrock_protocol_1_26_51::enums::{
    AnimateAction as AnimatePacketAction, ItemVersion as ItemDataItemVersion,
};
use bedrock_protocol_1_26_51::packets::{
    Animate as AnimatePacket, AnimateEntity as AnimateEntityPacket,
    ItemRegistry as ItemRegistryPacket, MobEquipment as MobEquipmentPacket,
};
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bedrock_protocol_1_26_51::wire;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[cfg(test)]
mod animation_tests;
mod attack;
mod banner;
mod components;
mod digger;
mod display;
mod equipment;
mod facts;
mod shooter;
pub use equipment::{held_items_match, offhand_equipment_packet, select_hotbar_slot_packet};
mod icons;
pub use facts::{ItemStackFacts, ItemStackFactsCache};
mod nbt_edit;
mod nbt_equal;
mod registry_properties;
mod user_data;

pub use attack::{ItemAttackCooldown, ItemAttackTiming, KineticWeaponTiming};
pub use banner::{ItemBannerPattern, item_banner_patterns, item_banner_type};
pub use components::{FoodProperties, ItemComponents, ItemUseCooldown};
pub use digger::{DiggerBlock, DiggerSpeed, DiggerStates, ItemDigger};
pub use display::{
    ItemBook, ItemDisplay, MAX_BOOK_PAGES, item_book, item_bundle_id, item_display,
    item_has_enchantment_list, item_id_aux,
};
pub(crate) use icons::item_icon_keys;
pub use nbt_edit::{
    ItemAnvilData, ItemNbtEdit, book_init_extra, book_needs_init, edit_item_extra, item_anvil_data,
    item_custom_name, item_filtered_custom_name, item_has_trim, item_repair_cost,
};
pub use shooter::ItemShooter;
pub use user_data::{ItemBlockList, item_block_list, item_user_data_equal};

/// The single item shape 1.26.40 puts on the wire.
///
/// Protocol 1001 modelled three separate item encodings (`Item`, `ItemNew`,
/// `ItemV4`) plus a `ShieldItemId`-discriminated extra-data union, because the
/// prismarine schema described each call site independently. BDS has one
/// descriptor whose trailing user data is an opaque length-prefixed buffer, so
/// the shield ID is no longer needed to decode an item.
type ItemStackDescriptor =
    bedrock_protocol_1_26_51::types::NetworkItemStackDescriptorSerializedData;

/// Number of hotbar slots on the vanilla survival hotbar.
pub const HOTBAR_SLOT_COUNT: u8 = 9;

/// Packs a held-use particle event from the negotiated item ID and signed auxiliary value.
pub fn item_use_progress_packet(runtime_id: u64, item_id: i16, auxiliary: i16) -> crate::Packet {
    crate::Packet::new(bedrock_protocol_1_26_51::packets::ActorEvent {
        target_runtime_id: ActorRuntimeID(runtime_id),
        event_id: bedrock_protocol_1_26_51::enums::ActorEventType::Feed,
        data: wire::ZigZag32((i32::from(item_id as u16) << 16) | i32::from(auxiliary)),
        fire_at_position: None,
    })
}

pub const MAX_ITEM_REGISTRY_ENTRIES: usize = 16_384;
pub const MAX_ITEM_EXTRA_BYTES: usize = 64 * 1024;
pub const MAX_ANIMATE_ENTITY_IDS: usize = 256;
pub const MAX_ACTION_IDENTIFIER_BYTES: usize = 256;
pub const MAX_ANIMATION_IDENTIFIER_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub enum ActorHandedness {
    Left,
    Right,
}

/// SHA-256 of no bytes: the digest of a stack without NBT.
const EMPTY_NBT_DIGEST: [u8; 32] = [
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
    0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
];

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
        /// Shared so an empty stack never allocates.
        static NO_EXTRA_DATA: std::sync::LazyLock<Arc<[u8]>> =
            std::sync::LazyLock::new(|| Arc::from([]));
        Self {
            network_id: 0,
            metadata: 0,
            stack_network_id: -1,
            count: 0,
            nbt_digest: EMPTY_NBT_DIGEST,
            block_runtime_id: 0,
            extra_data: Arc::clone(&NO_EXTRA_DATA),
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
    item_extra_damage(&stack.extra_data)
}

/// [`item_stack_damage`] over a stack's raw extra data.
#[must_use]
pub fn item_extra_damage(extra_data: &[u8]) -> Option<u32> {
    item_extra_damage_value(extra_data).and_then(|value| u32::try_from(value).ok())
}

/// Reads item damage as a signed 16-bit value from its retained integer `Damage` tag.
#[must_use]
pub fn item_extra_damage_value(extra_data: &[u8]) -> Option<i32> {
    item_extra_damage_integer(extra_data).map(|damage| i32::from(damage as i16))
}

/// Reads the stored root `Damage` integer without applying item damage conversion.
#[must_use]
pub fn item_extra_damage_integer(extra_data: &[u8]) -> Option<i32> {
    root_damage_tag(decode_extra_nbt(extra_data)?)
}

/// Whether a stack's extra data carries a true root `Unbreakable` byte.
#[must_use]
pub fn item_extra_unbreakable(extra_data: &[u8]) -> bool {
    decode_extra_nbt(extra_data).is_some_and(|nbt| {
        let mut cursor = nbt;
        root_tag(&mut cursor, 1, b"Unbreakable")
            .is_some_and(|mut value| read_u8(&mut value).is_some_and(|byte| byte != 0))
    })
}

/// Reads the typed item-lock byte, preserving unknown values for special-removal checks.
#[must_use]
pub fn item_extra_lock(extra_data: &[u8]) -> Option<u8> {
    let mut cursor = decode_extra_nbt(extra_data)?;
    let mut value = root_tag(&mut cursor, 1, b"minecraft:item_lock")?;
    read_u8(&mut value)
}

/// Whether retained item data locks the item into its current slot.
#[must_use]
pub fn item_extra_slot_locked(extra_data: &[u8]) -> bool {
    item_extra_lock(extra_data) == Some(1)
}

/// Reads one enchantment's level from the root `ench` list of a stack's extra data.
///
/// Each entry is a compound of short `id` and short `lvl`; malformed data reads as `None`.
#[must_use]
pub fn item_enchantment_level(extra_data: &[u8], enchantment_id: i16) -> Option<u8> {
    item_enchantment_level_value(extra_data, enchantment_id)
        .and_then(|level| u8::try_from(level).ok())
}

/// Reads the full signed-short level of the first matching typed enchantment entry.
#[must_use]
pub fn item_enchantment_level_value(extra_data: &[u8], enchantment_id: i16) -> Option<i16> {
    let nbt = decode_extra_nbt(extra_data)?;
    let mut cursor = nbt;
    let mut list = root_tag(&mut cursor, 9, b"ench")?;
    if read_u8(&mut list)? != 10 {
        return None;
    }
    let count = usize::try_from(read_i32_le(&mut list)?).ok()?;
    for _ in 0..count {
        let (mut id, mut level) = (None, None);
        loop {
            let tag = read_u8(&mut list)?;
            if tag == 0 {
                break;
            }
            let name_len = usize::from(read_u16_le(&mut list)?);
            let name = list.get(..name_len)?;
            list = list.get(name_len..)?;
            if tag == 2 && (name == b"id" || name == b"lvl") {
                let value = i16::from_le_bytes(list.get(..2)?.try_into().ok()?);
                *if name == b"id" { &mut id } else { &mut level } = Some(value);
            }
            skip_le_payload(&mut list, tag, 1)?;
        }
        if id == Some(enchantment_id) {
            return level;
        }
    }
    None
}

/// Reads the root `customColor` int (dyed leather RGB) from a stack's extra data, masked to 24 bits.
#[must_use]
pub fn item_custom_color(extra_data: &[u8]) -> Option<u32> {
    let nbt = decode_extra_nbt(extra_data)?;
    let mut cursor = nbt;
    let mut payload = root_tag(&mut cursor, 3, b"customColor")?;
    Some(read_i32_le(&mut payload)? as u32 & 0x00ff_ffff)
}

/// Reads the `Name` of the root `chargedItem` compound a loaded crossbow carries; `None` when the
/// stack is not charged or the compound names no projectile.
#[must_use]
pub fn item_charged_projectile(extra_data: &[u8]) -> Option<std::sync::Arc<str>> {
    let nbt = decode_extra_nbt(extra_data)?;
    let mut cursor = nbt;
    let mut payload = root_tag(&mut cursor, 10, b"chargedItem")?;
    loop {
        let entry = read_u8(&mut payload)?;
        if entry == 0 {
            return None;
        }
        let name_len = usize::from(read_u16_le(&mut payload)?);
        let name = payload.get(..name_len)?;
        payload = payload.get(name_len..)?;
        if entry == 8 && name == b"Name" {
            let len = usize::from(read_u16_le(&mut payload)?);
            let value = std::str::from_utf8(payload.get(..len)?).ok()?;
            return Some(std::sync::Arc::from(value));
        }
        skip_le_payload(&mut payload, entry, 1)?;
    }
}

/// Positions `cursor` at the payload of the named root tag of type `tag`.
fn root_tag<'a>(cursor: &mut &'a [u8], tag: u8, wanted: &[u8]) -> Option<&'a [u8]> {
    if read_u8(cursor)? != 10 {
        return None;
    }
    skip_le_string(cursor)?;
    compound_field(cursor, tag, wanted)
}

/// Finds one typed entry in a compound body, leaving its payload borrowed.
fn compound_field<'a>(cursor: &mut &'a [u8], tag: u8, wanted: &[u8]) -> Option<&'a [u8]> {
    loop {
        let entry = read_u8(cursor)?;
        if entry == 0 {
            return None;
        }
        let name_len = usize::from(read_u16_le(cursor)?);
        let name = cursor.get(..name_len)?;
        *cursor = cursor.get(name_len..)?;
        if entry == tag && name == wanted {
            return Some(cursor);
        }
        skip_le_payload(cursor, entry, 0)?;
    }
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
fn decode_extra_nbt(extra: &[u8]) -> Option<&[u8]> {
    const HEADER_LEN: usize = 3;
    let header = extra.get(..HEADER_LEN)?;
    let marker = i16::from_le_bytes([header[0], header[1]]);
    if marker != -1 || header[2] != 1 {
        return None;
    }
    Some(&extra[HEADER_LEN..])
}

/// Walks one fixed little-endian NBT compound root for an integer `Damage`.
fn root_damage_tag(nbt: &[u8]) -> Option<i32> {
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
            return Some(value);
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

const MAX_ITEM_NBT_WALK_DEPTH: u32 = wire::MAX_NBT_DEPTH;

fn skip_le_payload(cursor: &mut &[u8], tag: u8, depth: u32) -> Option<()> {
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
            if count != 0 && depth == MAX_ITEM_NBT_WALK_DEPTH {
                return None;
            }
            let width = match element {
                1 => Some(1usize),
                2 => Some(2),
                3 | 5 => Some(4),
                4 | 6 => Some(8),
                _ => None,
            };
            if let Some(width) = width {
                *cursor = cursor.get(count.checked_mul(width)?..)?;
                return Some(());
            }
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
    /// Client-facing components parsed once from the validated registry payload.
    pub components: Option<Arc<ItemComponents>>,
    pub identifier: Arc<str>,
    pub network_id: i32,
    pub component_based: bool,
    pub version: ItemRegistryVersion,
    pub component_digest: [u8; 32],
    /// Exact positive stack capacity retained from the negotiated component path.
    ///
    /// This is evidence only: it neither supplies a fallback nor binds a later
    /// inventory decision to the registry or session generation.
    pub negotiated_max_stack_size: Option<u8>,
    /// Whether the component payload is the exact canonical empty compound.
    pub canonical_empty_component_data: bool,
    /// Item tags a component-based entry declares for itself.
    pub item_tags: Arc<[Arc<str>]>,
}

/// Returns the retail-positive item registry for the pinned Bedrock protocol.
///
/// The compact table contains only entries referenced by the default creative
/// inventory. Numeric network IDs come from the matching ItemRegistry packet;
/// omitted entries remain unsupported gaps and are never renumbered.
#[must_use]
pub fn vanilla_item_registry() -> Arc<[ItemRegistryEntry]> {
    const RETAIL_ITEMS: &str = include_str!("../data/retail_items_1_26_50.tsv");

    let mut entries = Vec::with_capacity(RETAIL_ITEMS.lines().count());
    for line in RETAIL_ITEMS.lines() {
        let (network_id, identifier) = line
            .split_once('\t')
            .expect("retail item row must contain an ID and identifier");
        let network_id = network_id
            .parse::<i32>()
            .expect("retail item ID must be an i32");
        entries.push(ItemRegistryEntry {
            identifier: Arc::from(identifier),
            network_id,
            component_based: false,
            version: ItemRegistryVersion::Legacy,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
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
        next_state: Arc<str>,
        stop_expression: Arc<str>,
        stop_expression_version: i32,
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
    #[error("item registry contains duplicate network ID")]
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
    #[error("item packet has malformed or truncated canonical wire data")]
    MalformedWire,
    #[error("failed to encode validated item data")]
    ItemEncodingFailed,
    #[error("actor runtime ID {0} is invalid")]
    InvalidRuntimeId(i64),
    #[error("equipment container ID {0} is unknown")]
    UnknownEquipmentContainer(u8),
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
pub(crate) fn normalize_item(
    item: ItemStackDescriptor,
) -> Result<NetworkItemStack, ItemPacketError> {
    validate_item_user_data(&item.user_data_buffer)?;
    if item.id.0 == 0 {
        return Ok(NetworkItemStack::empty());
    }
    // An absent net ID is the "no stack tracking" case, which the app models as
    // -1. gophertunnel leaves StackNetworkID at 0 when the bool is unset; the
    // distinction is preserved here rather than collapsing the two.
    let stack_network_id = match item.net_id_variant {
        None => -1,
        Some(wire::ZigZag32(stack_id)) if stack_id > 0 => stack_id,
        Some(_) => return Err(ItemPacketError::ContradictoryStackId),
    };
    make_stack(
        i32::from(item.id.0),
        i32::from_ne_bytes(item.aux_value.0.to_ne_bytes()),
        stack_network_id,
        item.stack_size.0,
        i32::from_ne_bytes(item.block_runtime_id.0.to_ne_bytes()),
        &item.user_data_buffer,
    )
}

fn make_stack(
    network_id: i32,
    metadata: i32,
    stack_network_id: i32,
    count: u16,
    block_runtime_id: i32,
    extra: &[u8],
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
        nbt_digest: Sha256::digest(extra).into(),
        block_runtime_id,
        extra_data: Arc::from(extra),
    })
}

/// Encodes `value` through a writer capped at [`MAX_ITEM_EXTRA_BYTES`].
fn encode_extra<T: wire::Encode>(value: &T) -> Result<Vec<u8>, ItemPacketError> {
    let mut bytes = Vec::new();
    match value.encode(&mut wire::Writer::with_limit(
        &mut bytes,
        MAX_ITEM_EXTRA_BYTES,
    )) {
        Ok(()) => Ok(bytes),
        Err(wire::EncodeError::OutputLimitExceeded { needed, .. }) => {
            Err(ItemPacketError::ItemExtraTooLarge {
                bytes: needed,
                max: MAX_ITEM_EXTRA_BYTES,
            })
        }
        Err(_) => Err(ItemPacketError::ItemEncodingFailed),
    }
}

/// Bounds an item's user-data buffer and checks the compound it may carry.
///
/// The generated decoder hands this over as opaque bytes, so the header
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
    let header = extra.get(..2).ok_or(ItemPacketError::InvalidItemNbt)?;
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
            wire::Reader::new(&extra[3..])
                .read_nbt(wire::NbtVariant::Persistent)
                .map_err(|_| ItemPacketError::InvalidItemNbt)?;
            Ok(())
        }
        _ => Err(ItemPacketError::InvalidItemNbt),
    }
}

fn validate_registry_nbt(nbt: &wire::NetworkNbt) -> Result<(), ItemPacketError> {
    let mut reader = wire::Reader::new(&nbt.0);
    reader
        .read_nbt(wire::NbtVariant::Network)
        .map_err(|_| ItemPacketError::InvalidItemNbt)?;
    if !reader.is_empty() {
        return Err(ItemPacketError::InvalidItemNbt);
    }
    Ok(())
}

pub(crate) fn normalize_item_registry(
    packet: ItemRegistryPacket,
) -> Result<ItemRegistryEvent, ItemPacketError> {
    if packet.item_data.len() > MAX_ITEM_REGISTRY_ENTRIES {
        return Err(ItemPacketError::TooManyRegistryEntries {
            count: packet.item_data.len(),
            max: MAX_ITEM_REGISTRY_ENTRIES,
        });
    }
    let mut network_ids = HashMap::with_capacity(packet.item_data.len());
    let mut entries = Vec::with_capacity(packet.item_data.len());
    for item in packet.item_data {
        if item.item_name.len() > MAX_ACTION_IDENTIFIER_BYTES {
            return Err(ItemPacketError::ItemIdentifierTooLong {
                bytes: item.item_name.len(),
                max: MAX_ACTION_IDENTIFIER_BYTES,
            });
        }
        let network_id = i32::from(item.item_id.0);
        validate_registry_nbt(&item.item_component_data)?;
        let component_bytes = encode_extra(&item.item_component_data)?;
        let version = match item.item_version {
            ItemDataItemVersion::Legacy => ItemRegistryVersion::Legacy,
            ItemDataItemVersion::Datadriven => ItemRegistryVersion::DataDriven,
            ItemDataItemVersion::None => ItemRegistryVersion::None,
            ItemDataItemVersion::Unknown(value) => ItemRegistryVersion::Unknown(value),
        };
        let canonical_empty_component_data =
            component_bytes == registry_properties::CANONICAL_EMPTY_COMPONENT_DATA;
        let negotiated_max_stack_size = if matches!(version, ItemRegistryVersion::Unknown(_)) {
            None
        } else {
            registry_properties::negotiated_max_stack_size(&component_bytes)
        };
        let entry = ItemRegistryEntry {
            identifier: Arc::from(item.item_name.to_string_lossy()),
            network_id,
            component_based: item.is_component_based,
            version,
            components: components::parse_components(&component_bytes).map(Arc::new),
            component_digest: Sha256::digest(&component_bytes).into(),
            negotiated_max_stack_size,
            canonical_empty_component_data,
            item_tags: if item.is_component_based {
                registry_properties::declared_item_tags(&component_bytes).into()
            } else {
                Arc::from([])
            },
        };
        if let Some(&previous) = network_ids.get(&network_id) {
            let current: &mut ItemRegistryEntry = &mut entries[previous];
            if *current == entry {
                continue;
            }
            if current.identifier != entry.identifier
                || current.version != entry.version
                || current.component_based != entry.component_based
                || (!current.canonical_empty_component_data
                    && !entry.canonical_empty_component_data)
            {
                return Err(ItemPacketError::DuplicateRegistryEntry);
            }
            if current.canonical_empty_component_data {
                *current = entry;
            }
        } else {
            network_ids.insert(network_id, entries.len());
            entries.push(entry);
        }
    }
    Ok(ItemRegistryEvent {
        entries: Arc::from(entries),
    })
}

pub(crate) fn normalize_equipment(
    packet: MobEquipmentPacket,
) -> Result<EquipmentEvent, ItemPacketError> {
    let actor_runtime_id = runtime_id_signed(packet.target_runtime_id.0)?;
    normalize_equipment_parts(
        actor_runtime_id,
        normalize_item(packet.item)?,
        packet.slot.0,
        packet.selected_slot.0,
        packet.container_id.0,
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
    let (window_id, handedness) = window_id(container_id)?;
    Ok(EquipmentEvent {
        actor_runtime_id,
        stack,
        inventory_slot: i32::from(inventory_slot),
        selected_slot,
        window_id,
        handedness,
    })
}

/// Sends an actor's attack event through the same swing normalization as Animate packets.
pub(crate) fn normalize_actor_attack(
    runtime_id: ActorRuntimeID,
) -> Result<ItemActorEvent, ItemPacketError> {
    normalize_animate(AnimatePacket {
        action: AnimatePacketAction::Swing,
        target_actor_runtime_id: runtime_id,
        ..Default::default()
    })
}

pub(crate) fn normalize_animate(packet: AnimatePacket) -> Result<ItemActorEvent, ItemPacketError> {
    if !packet.data.0.is_finite() {
        return Err(ItemPacketError::NonFiniteActionField("data"));
    }
    if let Some(source) = &packet.swing_source {
        validate_text("swing source", source, MAX_ACTION_IDENTIFIER_BYTES)?;
    }
    let kind = match packet.action {
        AnimatePacketAction::Swing => ActorActionKind::SwingArm,
        AnimatePacketAction::Wakeup => ActorActionKind::Wake,
        AnimatePacketAction::Criticalhit => ActorActionKind::CriticalHit,
        AnimatePacketAction::Magiccriticalhit => ActorActionKind::MagicCriticalHit,
        AnimatePacketAction::Unknown(128u8) => ActorActionKind::RowRight,
        AnimatePacketAction::Unknown(129u8) => ActorActionKind::RowLeft,
        AnimatePacketAction::Noaction => ActorActionKind::Ignored { action_id: 0 },
        AnimatePacketAction::Unknown(action_id) => ActorActionKind::Ignored { action_id },
    };
    Ok(ItemActorEvent::Action(ActorActionEvent {
        actor_runtime_ids: Arc::from([runtime_id_signed(packet.target_actor_runtime_id.0)?]),
        kind,
        data: packet.data.0,
        swing_source: packet
            .swing_source
            .map(|source| Arc::from(source.to_string_lossy())),
    }))
}

pub(crate) fn normalize_animate_entity(
    packet: AnimateEntityPacket,
) -> Result<ItemActorEvent, ItemPacketError> {
    if packet.m_runtime_ids.is_empty() || packet.m_runtime_ids.len() > MAX_ANIMATE_ENTITY_IDS {
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
    if !packet.m_blend_out_time.0.is_finite() {
        return Err(ItemPacketError::NonFiniteActionField("blend_out_time"));
    }
    let mut seen = HashSet::with_capacity(packet.m_runtime_ids.len());
    let actor_runtime_ids = packet
        .m_runtime_ids
        .into_iter()
        .map(|id| runtime_id_signed(id.0))
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
            animation: Arc::from(packet.m_animation.to_string_lossy()),
            controller: Arc::from(packet.m_controller.to_string_lossy()),
            next_state: Arc::from(packet.m_next_state.to_string_lossy()),
            stop_expression: Arc::from(packet.m_stop_expression.to_string_lossy()),
            stop_expression_version: packet.m_stop_expression_version.0,
        },
        data: packet.m_blend_out_time.0,
        swing_source: None,
    }))
}

fn runtime_id_signed(value: u64) -> Result<u64, ItemPacketError> {
    if value == 0 {
        return Err(ItemPacketError::InvalidRuntimeId(
            i64::try_from(value).unwrap_or(i64::MAX),
        ));
    }
    Ok(value)
}

fn validate_text(field: &'static str, text: &wire::Str, max: usize) -> Result<(), ItemPacketError> {
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
/// `u8` (gophertunnel's `MobEquipment.WindowID`), so only the three recognized
/// hand containers need naming.
fn window_id(container_id: u8) -> Result<(u8, Option<ActorHandedness>), ItemPacketError> {
    const INVENTORY: u8 = 0;
    // The offhand container ID, matching gophertunnel's ContainerIDOffhand.
    const OFFHAND: u8 = 119;
    const HOTBAR: u8 = 122;

    let handedness = match container_id {
        INVENTORY | HOTBAR => ActorHandedness::Right,
        OFFHAND => ActorHandedness::Left,
        _ => return Err(ItemPacketError::UnknownEquipmentContainer(container_id)),
    };
    Ok((container_id, Some(handedness)))
}

#[cfg(test)]
mod hotbar_tests;
