use std::sync::Arc;

use bytes::BytesMut;
use sha2::{Digest, Sha256};
use thiserror::Error;
use valentine::bedrock::{
    codec::BedrockCodec,
    version::v1_26_51::{
        ContainerClosePacket, ContainerOpenPacket, ContainerSetDataPacket,
        EnumsContainerEnumName as FullContainerNameContainerName,
        EnumsItemStackNetResult as ItemStackResponseInfoResult, FullContainerName,
        InventoryContentPacket, InventorySlotPacket, ItemStackResponsePacket,
        MobArmorEquipmentPacket, PlayerEnchantOptionsPacket, PlayerHotbarPacket,
    },
};
use valentine::protocol::wire;

use crate::item::{ArmorEquipmentEvent, NetworkItemStack};

mod address;
mod container_policy;
mod creative;
pub use container_policy::{
    CONTAINER_NAME_CREATED_OUTPUT, CONTAINER_NAME_HOTBAR, ContainerWindow, LAST_CONTAINER_NAME,
    container_window,
};
pub use creative::{
    CreativeCategory, CreativeContentEvent, CreativeGroup, CreativeItem, MAX_CREATIVE_GROUPS,
    MAX_CREATIVE_ITEMS, normalize_creative_content,
};
mod client_packets;
mod legacy;
pub use legacy::{
    NormalInventoryChange, NormalInventorySource, normal_inventory_transaction_packet,
};
mod raw_scan;
pub mod recipes;
mod request;
mod transaction;
mod validation;
mod windows;
pub use address::{
    ARMOR_WINDOW_ID, CONTAINER_NAME_ARMOR, CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
    CONTAINER_NAME_CRAFT_INPUT, CONTAINER_NAME_CURSOR, CONTAINER_NAME_DYNAMIC,
    CONTAINER_NAME_INVENTORY, CONTAINER_NAME_LEVEL_ENTITY, CONTAINER_NAME_OFFHAND, CanonicalCell,
    DYNAMIC_STORAGE_WINDOW_ID, OFFHAND_WINDOW_ID, PLAYER_INVENTORY_WINDOW_ID,
    UI_INVENTORY_WINDOW_ID, is_personal_ui_inventory, personal_craft_content_indices,
    personal_craft_slot_index, project_container_cell,
};
pub use client_packets::{
    BookEdit, MAX_BOOK_PAGE_BYTES, block_pick_request_packet, book_edit_packet,
    crafter_slot_toggle_packet, lectern_update_packet,
};
pub(crate) use raw_scan::validate_raw_inventory_packet;
pub use request::mining::{MineBlockRequest, MineBlockRequestError};
pub(crate) use transaction::normalize_transaction;
pub use windows::{
    NO_CONTAINER_WINDOW_TYPE, OpenCells, UI_SLOT_COUNT, WINDOW_TYPE_ANVIL, WINDOW_TYPE_BEACON,
    WINDOW_TYPE_BLAST_FURNACE, WINDOW_TYPE_BREWING_STAND, WINDOW_TYPE_CARTOGRAPHY,
    WINDOW_TYPE_CONTAINER, WINDOW_TYPE_CRAFTER, WINDOW_TYPE_DISPENSER, WINDOW_TYPE_DROPPER,
    WINDOW_TYPE_ENCHANTMENT, WINDOW_TYPE_FURNACE, WINDOW_TYPE_GRINDSTONE, WINDOW_TYPE_HOPPER,
    WINDOW_TYPE_HORSE, WINDOW_TYPE_LECTERN, WINDOW_TYPE_LOOM, WINDOW_TYPE_SMITHING_TABLE,
    WINDOW_TYPE_SMOKER, WINDOW_TYPE_STONECUTTER, WINDOW_TYPE_WORKBENCH, WindowKind, WindowSegment,
    is_chest_like_name, is_open_window_name, is_result_preview_name, open_cell_request,
    open_name_first_cell, ui_slot_container_name, ui_slot_for_name, ui_slot_request_container,
};
mod registry_snapshot;
pub use recipes::{
    IngredientObservation, MAX_RECIPE_OBSERVATIONS, RecipeObservation, RecipeObservations,
};
pub use registry_snapshot::{RecipeRegistryError, RecipeRegistrySnapshot};
pub use request::{
    ARMOR_SLOTS, AutoCraftIngredient, CRAFTING_INPUT_SLOTS, CREATED_OUTPUT_SLOT, CraftResult,
    MAX_FILTER_STRINGS, MAX_STACK_REQUEST_ACTIONS, PLAYER_INVENTORY_SLOTS, StackItemDescriptor,
    StackRequestAction, StackRequestContainer, StackRequestSlot, container_close_packet,
    item_stack_request_batch, item_stack_request_packet, item_stack_request_packet_filtered,
    open_inventory_packet,
};
use validation::validate_item_user_data;
pub const MAX_CONTAINER_SLOTS: usize = 4_096;
pub const MAX_ITEM_NBT_BYTES: usize = 1_048_576;
pub const MAX_STACK_RESPONSES: usize = 512;
pub const MAX_RESPONSE_CONTAINERS: usize = 128;
pub const MAX_ITEM_EXTRA_BYTES: usize = 64 * 1_024;
pub const MAX_RESPONSE_NAME_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InventoryAuthority {
    Client,
    Server,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ContainerIdentity {
    pub window_id: Option<i32>,
    pub slot_type: Option<u8>,
    pub dynamic_id: Option<u32>,
}

impl ContainerIdentity {
    #[must_use]
    pub const fn window(window_id: i32) -> Self {
        Self {
            window_id: Some(window_id),
            slot_type: None,
            dynamic_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SlotIdentity {
    pub container: ContainerIdentity,
    pub slot: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryContentEvent {
    pub container: ContainerIdentity,
    pub slots: Arc<[NetworkItemStack]>,
    pub storage_item: NetworkItemStack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventorySlotEvent {
    pub identity: SlotIdentity,
    pub stack: NetworkItemStack,
    pub storage_item: Option<NetworkItemStack>,
}

/// Absolute inventory writes carried by one normal transaction. The world
/// balancing leg is not an inventory write and is never projected here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryTransactionEvent {
    pub slots: Arc<[InventorySlotEvent]>,
    pub skipped_actions: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SelectedSlotEvent {
    pub container: ContainerIdentity,
    pub slot: u8,
    pub select_slot: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StackResponseStatus {
    Accepted,
    Rejected,
    Unknown(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackResponseSlot {
    pub slot: u8,
    pub hotbar_slot: u8,
    pub count: u8,
    pub item_stack_id: i32,
    pub custom_name: Arc<str>,
    pub filtered_custom_name: Arc<str>,
    pub durability_correction: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackResponseContainer {
    pub container: ContainerIdentity,
    pub slots: Arc<[StackResponseSlot]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackResponse {
    pub status: StackResponseStatus,
    pub request_id: i32,
    pub containers: Arc<[StackResponseContainer]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemStackResponseEvent {
    pub responses: Arc<[StackResponse]>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ContainerOpenEvent {
    pub container: ContainerIdentity,
    pub window_type: i8,
    pub position: [i32; 3],
    pub runtime_entity_id: i64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ContainerCloseEvent {
    pub container: ContainerIdentity,
    pub window_type: i8,
    pub server_initiated: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ContainerDataEvent {
    pub container: ContainerIdentity,
    pub property: i32,
    pub value: i32,
}

/// One enchanting-table option the server offers for the input item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantOption {
    pub cost: u8,
    /// The option's display text in the standard galactic alphabet.
    pub name: Arc<str>,
    /// The recipe network id a selection request names.
    pub network_id: u32,
    /// `(enchantment type code, level)` pairs the option applies.
    pub enchants: Arc<[(u8, u8)]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnchantOptionsEvent {
    pub options: Arc<[EnchantOption]>,
}

/// Options one enchanting table shows at most; extras are dropped.
pub const MAX_ENCHANT_OPTIONS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryEvent {
    Recipes(recipes::RecipeUpdate),
    Authority(InventoryAuthority),
    Content(InventoryContentEvent),
    Slot(InventorySlotEvent),
    Transaction(InventoryTransactionEvent),
    SelectedSlot(SelectedSlotEvent),
    Response(ItemStackResponseEvent),
    Open(ContainerOpenEvent),
    Close(ContainerCloseEvent),
    Data(ContainerDataEvent),
    EnchantOptions(EnchantOptionsEvent),
    Creative(CreativeContentEvent),
}

impl InventoryEvent {
    /// Individual authoritative writes, in their wire order. A transaction
    /// remains one FIFO event even when it writes several inventory surfaces.
    #[must_use]
    pub fn slot_updates(&self) -> &[InventorySlotEvent] {
        match self {
            Self::Slot(slot) => std::slice::from_ref(slot),
            Self::Transaction(transaction) => &transaction.slots,
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InventoryPacketError {
    #[error("normal inventory transaction has {0} actions, outside its retention bound")]
    InvalidNormalTransactionActionCount(usize),
    #[error("item stack request ID must be a negative odd integer below -1")]
    InvalidStackRequestId,
    #[error("item stack request amount must be positive")]
    InvalidStackRequestAmount,
    #[error("item stack request has {0} actions, outside 1..=100")]
    InvalidStackRequestActionCount(usize),
    #[error("item stack request slot {slot} is invalid for {container:?}")]
    InvalidStackRequestSlot {
        container: StackRequestContainer,
        slot: u8,
    },
    #[error("item stack request network ID {0} is invalid")]
    InvalidRequestStackNetworkId(i32),
    #[error("personal inventory target runtime ID must be nonzero")]
    InvalidInventoryTargetRuntimeId,
    #[error("container close window ID {0} is outside 0..=255")]
    InvalidContainerCloseWindowId(i32),
    #[error("armor equipment actor runtime ID {0} is invalid")]
    InvalidArmorRuntimeId(u64),
    #[error("inventory slot {0} is outside 0..{MAX_CONTAINER_SLOTS}")]
    InvalidSlot(i32),
    #[error("selected hotbar slot {0} is outside 0..9")]
    InvalidSelectedSlot(i32),
    #[error("inventory content has {count} slots, exceeding {max}")]
    TooManySlots { count: usize, max: usize },
    #[error("item NBT has {bytes} bytes, exceeding {max}")]
    ItemNbtTooLarge { bytes: usize, max: usize },
    #[error("item extra data has {bytes} bytes, exceeding {max}")]
    ItemExtraTooLarge { bytes: usize, max: usize },
    #[error("stack response packet has {count} responses, exceeding {max}")]
    TooManyResponses { count: usize, max: usize },
    #[error("stack response has {count} containers, exceeding {max}")]
    TooManyResponseContainers { count: usize, max: usize },
    #[error("stack response container has {count} slots, exceeding {max}")]
    TooManyResponseSlots { count: usize, max: usize },
    #[error("stack response name has {bytes} bytes, exceeding {max}")]
    ResponseNameTooLong { bytes: usize, max: usize },
    #[error("accepted stack response has no content")]
    MissingResponseContent,
    #[error("rejected stack response unexpectedly has content")]
    UnexpectedResponseContent,
    #[error("item network ID {0} is invalid")]
    InvalidItemNetworkId(i32),
    #[error("non-empty item has an empty stack count")]
    InvalidItemCount,
    #[error("item stack network ID {0} is invalid")]
    InvalidStackNetworkId(i32),
    #[error("item stack-ID presence or kind is contradictory")]
    ContradictoryStackId,
    #[error("item NBT has unsupported version {0}; expected version 1")]
    UnsupportedItemNbtVersion(u8),
    #[error("item NBT is malformed")]
    InvalidItemNbt,
    #[error("verified item extra data is malformed or unsupported")]
    InvalidItemExtra,
    #[error("item extra string has {bytes} bytes, exceeding {max}")]
    ItemExtraStringTooLarge { bytes: usize, max: usize },
    #[error("failed to encode validated inventory packet data")]
    EncodingFailed,
    #[error("item retained-byte digest does not match")]
    DigestMismatch,
    #[error("empty item has contradictory retained fields")]
    ContradictoryEmptyItem,
    #[error("inventory packet has malformed or truncated canonical wire data")]
    MalformedWire,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedNetworkItemStack {
    inner: NetworkItemStack,
}

impl VerifiedNetworkItemStack {
    pub fn try_new(
        stack: NetworkItemStack,
        expected_digest: [u8; 32],
    ) -> Result<Self, InventoryPacketError> {
        validate_stack_shape(&stack)?;
        let actual: [u8; 32] = Sha256::digest(&stack.extra_data).into();
        if actual != stack.nbt_digest || actual != expected_digest {
            return Err(InventoryPacketError::DigestMismatch);
        }
        Ok(Self { inner: stack })
    }

    #[must_use]
    pub const fn network_id(&self) -> i32 {
        self.inner.network_id
    }

    #[must_use]
    pub const fn metadata(&self) -> u32 {
        self.inner.metadata
    }

    #[must_use]
    pub const fn stack_network_id(&self) -> i32 {
        self.inner.stack_network_id
    }

    #[must_use]
    pub const fn count(&self) -> u16 {
        self.inner.count
    }

    #[must_use]
    pub const fn nbt_digest(&self) -> [u8; 32] {
        self.inner.nbt_digest
    }

    #[must_use]
    pub const fn block_runtime_id(&self) -> i32 {
        self.inner.block_runtime_id
    }

    #[must_use]
    pub fn extra_data(&self) -> &[u8] {
        &self.inner.extra_data
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// The stack less one item, carrying `legacy_request_id` as its net id; empty when none
    /// remain, as a throw leaves it.
    #[must_use]
    pub fn less_one(&self, legacy_request_id: i32) -> Self {
        if self.inner.count <= 1 {
            return Self {
                inner: NetworkItemStack::empty(),
            };
        }
        let mut inner = self.inner.clone();
        inner.count -= 1;
        inner.stack_network_id = legacy_request_id;
        Self { inner }
    }

    #[allow(
        dead_code,
        reason = "Task 12 outbound builders consume this Task 10 verification boundary"
    )]
    pub(crate) fn into_vendor_item(
        self,
        shield_item_id: i32,
    ) -> Result<ItemStackDescriptor, InventoryPacketError> {
        // The shield ID no longer selects an extra-data shape: 1.26.40 carries
        // the user-data buffer opaquely, so it is copied through as-is. The
        // parameter is kept so callers keep threading session state.
        let _ = shield_item_id;
        if self.inner.is_empty() {
            return Ok(ItemStackDescriptor::default());
        }
        let id = i16::try_from(self.inner.network_id)
            .map_err(|_| InventoryPacketError::InvalidItemNetworkId(self.inner.network_id))?;
        Ok(ItemStackDescriptor {
            id,
            stacksize: self.inner.count,
            auxvalue: self.inner.metadata,
            net_id_variant: (self.inner.stack_network_id != -1)
                .then_some(self.inner.stack_network_id),
            block_runtime_id: u32::from_ne_bytes(self.inner.block_runtime_id.to_ne_bytes()),
            user_data_buffer: self.inner.extra_data.to_vec(),
        })
    }
}

/// The single item shape 1.26.40 puts on the wire. See `crate::item`.
type ItemStackDescriptor =
    valentine::bedrock::version::v1_26_51::CerealizerNetworkItemStackDescriptorSerializedData;

#[must_use]
pub const fn normalize_authority(server_authoritative: bool) -> InventoryEvent {
    InventoryEvent::Authority(if server_authoritative {
        InventoryAuthority::Server
    } else {
        InventoryAuthority::Client
    })
}

pub fn normalize_content(
    packet: InventoryContentPacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    validate_slot_count(packet.slots.len())?;
    let container = container_identity_varint(
        i32::from_ne_bytes(packet.container_id.to_ne_bytes()),
        Some(packet.full_container_name),
    )?;
    let slots = packet
        .slots
        .into_iter()
        .map(normalize_item_descriptor)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: Arc::from(slots),
        storage_item: normalize_item_descriptor(packet.storage_item)?,
    }))
}

pub fn normalize_slot(packet: InventorySlotPacket) -> Result<InventoryEvent, InventoryPacketError> {
    let slot = checked_slot(i32::from_ne_bytes(packet.slot.to_ne_bytes()))?;
    let container =
        container_identity_varint(i32::from(packet.container_id), packet.full_container_name)?;
    Ok(InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity { container, slot },
        stack: normalize_item_descriptor(packet.item)?,
        storage_item: packet
            .storage_item
            .map(normalize_item_descriptor)
            .transpose()?,
    }))
}

pub fn normalize_hotbar(
    packet: PlayerHotbarPacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    let slot = u8::try_from(packet.selected_slot)
        .ok()
        .filter(|slot| *slot < 9)
        .ok_or(InventoryPacketError::InvalidSelectedSlot(
            i32::from_ne_bytes(packet.selected_slot.to_ne_bytes()),
        ))?;
    Ok(InventoryEvent::SelectedSlot(SelectedSlotEvent {
        container: ContainerIdentity::window(raw_window_id(packet.container_id)?),
        slot,
        select_slot: packet.shouldselectslot,
    }))
}

pub fn normalize_response(
    packet: ItemStackResponsePacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    if packet.responses.len() > MAX_STACK_RESPONSES {
        return Err(InventoryPacketError::TooManyResponses {
            count: packet.responses.len(),
            max: MAX_STACK_RESPONSES,
        });
    }
    let mut responses = Vec::with_capacity(packet.responses.len());
    for response in packet.responses {
        let (status, containers) = match (response.result, response.containers) {
            (ItemStackResponseInfoResult::Success, Some(content)) => {
                if content.len() > MAX_RESPONSE_CONTAINERS {
                    return Err(InventoryPacketError::TooManyResponseContainers {
                        count: content.len(),
                        max: MAX_RESPONSE_CONTAINERS,
                    });
                }
                let mut containers = Vec::with_capacity(content.len());
                for container in content {
                    validate_slot_count(container.slots.len()).map_err(|error| match error {
                        InventoryPacketError::TooManySlots { count, max } => {
                            InventoryPacketError::TooManyResponseSlots { count, max }
                        }
                        other => other,
                    })?;
                    let identity = full_container_identity(container.full_container_name)?;
                    let mut slots = Vec::with_capacity(container.slots.len());
                    for slot in container.slots {
                        let custom_name = slot.custom_name.unredacted;
                        let filtered_custom_name = slot.custom_name.redacted.unwrap_or_default();
                        validate_response_name(&custom_name)?;
                        validate_response_name(&filtered_custom_name)?;
                        // An absent stack net ID means the server did not track this
                        // slot, which the app models as -1 rather than as a rejection.
                        // A well-framed odd value is data, not framing failure.
                        // The sparse response consumer checks count/id pairing
                        // and skips unusable corrections without ending play.
                        let item_stack_id = slot.item_stack_net_id.map_or(-1, |net_id| net_id.id);
                        slots.push(StackResponseSlot {
                            slot: slot.slot,
                            hotbar_slot: slot.requested_slot,
                            count: slot.amount,
                            item_stack_id,
                            custom_name: Arc::from(custom_name),
                            filtered_custom_name: Arc::from(filtered_custom_name),
                            durability_correction: slot.durability_correction,
                        });
                    }
                    containers.push(StackResponseContainer {
                        container: identity,
                        slots: Arc::from(slots),
                    });
                }
                (StackResponseStatus::Accepted, containers)
            }
            (ItemStackResponseInfoResult::Success, None) => {
                return Err(InventoryPacketError::MissingResponseContent);
            }
            (ItemStackResponseInfoResult::Error, None) => {
                (StackResponseStatus::Rejected, Vec::new())
            }
            (other, None) => (
                StackResponseStatus::Unknown(response_result_code(&other)?),
                Vec::new(),
            ),
            (_, Some(_)) => return Err(InventoryPacketError::UnexpectedResponseContent),
        };
        responses.push(StackResponse {
            status,
            request_id: response.client_request_id.id,
            containers: Arc::from(containers),
        });
    }
    Ok(InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from(responses),
    }))
}

pub fn normalize_container_open(
    packet: ContainerOpenPacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    Ok(InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(raw_window_id(packet.container_id)?),
        window_type: raw_window_type(packet.container_type)?,
        position: [packet.position.x, packet.position.y, packet.position.z],
        runtime_entity_id: packet.target_actor_id.actor_unique_id,
    }))
}

pub fn normalize_container_close(
    packet: ContainerClosePacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    Ok(InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(raw_window_id(packet.container_id)?),
        window_type: raw_window_type(packet.container_type)?,
        server_initiated: packet.server_initiated_close,
    }))
}

pub fn normalize_container_data(
    packet: ContainerSetDataPacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    Ok(InventoryEvent::Data(ContainerDataEvent {
        container: ContainerIdentity::window(raw_window_id(packet.container_id)?),
        property: packet.id,
        value: packet.value,
    }))
}

pub fn normalize_enchant_options(
    packet: PlayerEnchantOptionsPacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    let options = packet
        .options
        .into_iter()
        .take(MAX_ENCHANT_OPTIONS)
        .map(|option| {
            let mut enchants = Vec::new();
            for instance in option.enchants.item_enchants.iter().flatten() {
                let mut bytes = BytesMut::with_capacity(1);
                instance
                    .enchant_type
                    .encode(&mut bytes)
                    .map_err(|_| InventoryPacketError::EncodingFailed)?;
                enchants.push((bytes[0], instance.enchant_level));
            }
            Ok(EnchantOption {
                cost: option.cost,
                name: Arc::from(option.enchant_name),
                network_id: option.enchant_net_id.raw_id,
                enchants: Arc::from(enchants),
            })
        })
        .collect::<Result<Vec<_>, InventoryPacketError>>()?;
    Ok(InventoryEvent::EnchantOptions(EnchantOptionsEvent {
        options: Arc::from(options),
    }))
}

pub fn validate_item_nbt_size(bytes: usize) -> Result<(), InventoryPacketError> {
    if bytes > MAX_ITEM_NBT_BYTES {
        return Err(InventoryPacketError::ItemNbtTooLarge {
            bytes,
            max: MAX_ITEM_NBT_BYTES,
        });
    }
    Ok(())
}

fn validate_slot_count(count: usize) -> Result<(), InventoryPacketError> {
    if count > MAX_CONTAINER_SLOTS {
        return Err(InventoryPacketError::TooManySlots {
            count,
            max: MAX_CONTAINER_SLOTS,
        });
    }
    Ok(())
}

fn checked_slot(slot: i32) -> Result<u16, InventoryPacketError> {
    let converted = u16::try_from(slot).map_err(|_| InventoryPacketError::InvalidSlot(slot))?;
    if usize::from(converted) >= MAX_CONTAINER_SLOTS {
        return Err(InventoryPacketError::InvalidSlot(slot));
    }
    Ok(converted)
}

pub(crate) fn normalize_armor_equipment(
    packet: MobArmorEquipmentPacket,
) -> Result<ArmorEquipmentEvent, InventoryPacketError> {
    let actor_runtime_id = packet.target_runtime_id.actor_runtime_id;
    if actor_runtime_id == 0 {
        return Err(InventoryPacketError::InvalidArmorRuntimeId(
            actor_runtime_id,
        ));
    }
    Ok(ArmorEquipmentEvent {
        actor_runtime_id,
        helmet: normalize_item_descriptor(packet.head)?,
        chestplate: normalize_item_descriptor(packet.torso)?,
        leggings: normalize_item_descriptor(packet.legs)?,
        boots: normalize_item_descriptor(packet.feet)?,
        body: normalize_item_descriptor(packet.body)?,
    })
}

/// Normalises the one item descriptor 1.26.40 uses everywhere.
///
/// Protocol 1001 needed `normalize_item_v4` and `normalize_item_new` because the
/// prismarine schema modelled the armour and inventory item encodings
/// separately, each with its own way of spelling "no stack ID". BDS has a single
/// descriptor with a plain `Option`, so the contradictory-shape checks are no
/// longer representable and the two collapse into this.
fn normalize_item_descriptor(
    item: ItemStackDescriptor,
) -> Result<NetworkItemStack, InventoryPacketError> {
    validate_item_user_data(&item.user_data_buffer)?;
    if item.id == 0 {
        return Ok(NetworkItemStack::empty());
    }
    let stack_network_id = match item.net_id_variant {
        None => -1,
        Some(id) if id > 0 => id,
        Some(_) => return Err(InventoryPacketError::ContradictoryStackId),
    };
    make_stack(
        i32::from(item.id),
        i32::from_ne_bytes(item.auxvalue.to_ne_bytes()),
        stack_network_id,
        item.stacksize,
        i32::from_ne_bytes(item.block_runtime_id.to_ne_bytes()),
        item.user_data_buffer,
    )
}

fn make_stack(
    network_id: i32,
    metadata: i32,
    stack_network_id: i32,
    count: u16,
    block_runtime_id: i32,
    extra_data: Vec<u8>,
) -> Result<NetworkItemStack, InventoryPacketError> {
    if network_id == 0 {
        return Err(InventoryPacketError::InvalidItemNetworkId(network_id));
    }
    if count == 0 {
        return Err(InventoryPacketError::InvalidItemCount);
    }
    if stack_network_id == 0 || stack_network_id < -1 {
        return Err(InventoryPacketError::InvalidStackNetworkId(
            stack_network_id,
        ));
    }
    if extra_data.len() > MAX_ITEM_EXTRA_BYTES {
        return Err(InventoryPacketError::ItemExtraTooLarge {
            bytes: extra_data.len(),
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    Ok(NetworkItemStack {
        network_id,
        metadata: u32::from_ne_bytes(metadata.to_ne_bytes()),
        stack_network_id,
        count,
        nbt_digest: Sha256::digest(&extra_data).into(),
        block_runtime_id,
        extra_data: Arc::from(extra_data),
    })
}

fn validate_stack_shape(stack: &NetworkItemStack) -> Result<(), InventoryPacketError> {
    if stack.extra_data.len() > MAX_ITEM_EXTRA_BYTES {
        return Err(InventoryPacketError::ItemExtraTooLarge {
            bytes: stack.extra_data.len(),
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    if stack.is_empty() {
        if stack != &NetworkItemStack::empty() {
            return Err(InventoryPacketError::ContradictoryEmptyItem);
        }
        return Ok(());
    }
    if stack.network_id == 0 {
        return Err(InventoryPacketError::InvalidItemNetworkId(stack.network_id));
    }
    if stack.stack_network_id == 0 || stack.stack_network_id < -1 {
        return Err(InventoryPacketError::InvalidStackNetworkId(
            stack.stack_network_id,
        ));
    }
    Ok(())
}

/// Recovers the wire code behind an unrecognised item-stack response result.
fn response_result_code(result: &ItemStackResponseInfoResult) -> Result<u8, InventoryPacketError> {
    let mut bytes = BytesMut::with_capacity(1);
    result
        .encode(&mut bytes)
        .map_err(|_| InventoryPacketError::EncodingFailed)?;
    Ok(bytes[0])
}

fn container_identity_varint(
    window_id: i32,
    full: Option<FullContainerName>,
) -> Result<ContainerIdentity, InventoryPacketError> {
    let window_id = raw_window_id_varint(window_id)?;
    let mut identity = full.map_or(
        Ok(ContainerIdentity {
            window_id: None,
            slot_type: None,
            dynamic_id: None,
        }),
        full_container_identity,
    )?;
    // Live legacy player and offhand rewrites carry a mandatory
    // FullContainerName value whose zero/default shape is only a placeholder.
    // Preserve real named or dynamic descriptors, and preserve the same
    // zero/default shape everywhere else; only these two legacy window IDs
    // have an established unnamed interpretation.
    if matches!(window_id, PLAYER_INVENTORY_WINDOW_ID | OFFHAND_WINDOW_ID)
        && identity.slot_type == Some(0)
        && identity.dynamic_id.is_none()
    {
        identity.slot_type = None;
    }
    identity.window_id = Some(window_id);
    Ok(identity)
}

fn full_container_identity(
    full: FullContainerName,
) -> Result<ContainerIdentity, InventoryPacketError> {
    Ok(ContainerIdentity {
        window_id: None,
        slot_type: Some(raw_container_slot(full.container_name)?),
        dynamic_id: full.dynamic_id,
    })
}

/// 1.26.40 carries container IDs, slot types and container types as raw
/// integers rather than named enums, so the protocol-1001 helpers that
/// round-tripped an enum through its encoder just to recover the wire number
/// are now plain widenings.
fn raw_window_id(value: u8) -> Result<i32, InventoryPacketError> {
    Ok(i32::from(i8::from_ne_bytes([value])))
}

fn raw_window_id_varint(value: i32) -> Result<i32, InventoryPacketError> {
    Ok(value)
}

fn raw_container_slot(value: FullContainerNameContainerName) -> Result<u8, InventoryPacketError> {
    let mut bytes = BytesMut::with_capacity(1);
    value
        .encode(&mut bytes)
        .map_err(|_| InventoryPacketError::EncodingFailed)?;
    Ok(bytes[0])
}

fn raw_window_type(value: u8) -> Result<i8, InventoryPacketError> {
    Ok(i8::from_ne_bytes([value]))
}

fn validate_response_name(value: &str) -> Result<(), InventoryPacketError> {
    if value.len() > MAX_RESPONSE_NAME_BYTES {
        return Err(InventoryPacketError::ResponseNameTooLong {
            bytes: value.len(),
            max: MAX_RESPONSE_NAME_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_network_stack_is_consumed_into_vendor_item_without_exposing_inner_stack() {
        let packet = InventorySlotPacket {
            container_id: 0,
            slot: 0,
            full_container_name: None,
            storage_item: None,
            item: ItemStackDescriptor {
                id: 7,
                stacksize: 4,
                auxvalue: 3,
                net_id_variant: Some(13),
                block_runtime_id: 92,
                user_data_buffer: Vec::new(),
            },
        };
        let InventoryEvent::Slot(event) = normalize_slot(packet).unwrap() else {
            panic!("expected slot event")
        };
        let expected_digest = event.stack.nbt_digest;
        let verified = VerifiedNetworkItemStack::try_new(event.stack, expected_digest).unwrap();
        let vendor = verified.into_vendor_item(0).unwrap();
        assert_eq!(vendor.id, 7);
        assert_eq!(vendor.stacksize, 4);
        assert_eq!(vendor.auxvalue, 3);
        assert_eq!(vendor.net_id_variant, Some(13));
        assert_eq!(vendor.block_runtime_id, 92);
        assert!(vendor.user_data_buffer.is_empty());
    }
}
