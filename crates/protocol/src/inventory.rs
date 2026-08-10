use std::sync::Arc;

use bytes::{Buf, Bytes, BytesMut};
use sha2::{Digest, Sha256};
use thiserror::Error;
use valentine::bedrock::{
    codec::{BedrockCodec, Nbt},
    version::v1_26_40::{
        ContainerClosePacket, ContainerOpenPacket, ContainerSetDataPacket, FullContainerName,
        FullContainerNameContainerName, InventoryContentPacket, InventorySlotPacket,
        ItemStackResponseInfoResult, ItemStackResponsePacket, McpePacketName,
        MobArmorEquipmentPacket, PlayerHotbarPacket,
    },
};
use valentine::protocol::wire;

use crate::item::{ArmorEquipmentEvent, NetworkItemStack};

mod request;
pub use request::{
    PLAYER_INVENTORY_SLOTS, StackRequestAction, StackRequestContainer, StackRequestSlot,
    container_close_packet, item_stack_request_packet,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryEvent {
    Authority(InventoryAuthority),
    Content(InventoryContentEvent),
    Slot(InventorySlotEvent),
    SelectedSlot(SelectedSlotEvent),
    Response(ItemStackResponseEvent),
    Open(ContainerOpenEvent),
    Close(ContainerCloseEvent),
    Data(ContainerDataEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InventoryPacketError {
    #[error("item stack request ID must be positive")]
    InvalidStackRequestId,
    #[error("item stack request amount must be positive")]
    InvalidStackRequestAmount,
    #[error("item stack request slot {slot} is invalid for {container:?}")]
    InvalidStackRequestSlot {
        container: StackRequestContainer,
        slot: u8,
    },
    #[error("item stack request network ID {0} is invalid")]
    InvalidRequestStackNetworkId(i32),
    #[error("container close window ID {0} is outside 0..=255")]
    InvalidContainerCloseWindowId(i32),
    #[error("armor equipment actor runtime ID {0} is invalid")]
    InvalidArmorRuntimeId(i64),
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
            auxvalue: i32::from_ne_bytes(self.inner.metadata.to_ne_bytes()),
            net_id_variant: (self.inner.stack_network_id != -1)
                .then_some(self.inner.stack_network_id),
            block_runtime_id: self.inner.block_runtime_id,
            user_data_buffer: self.inner.extra_data.to_vec(),
        })
    }
}

/// The single item shape 1.26.40 puts on the wire. See `crate::item`.
type ItemStackDescriptor =
    valentine::bedrock::version::v1_26_40::CerealizerNetworkItemStackDescriptorSerializedData;

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
    let container =
        container_identity_varint(packet.container_id, Some(packet.full_container_name))?;
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
    let slot = checked_slot(packet.slot)?;
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
            packet.selected_slot,
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
                        // The two custom names are one redactable string now:
                        // gophertunnel writes CustomName then FilteredCustomName
                        // (protocol/item_stack.go), which map to the unredacted
                        // and redacted halves respectively.
                        let custom_name = slot.custom_name.unredacted;
                        let filtered_custom_name = slot.custom_name.redacted.unwrap_or_default();
                        validate_response_name(&custom_name)?;
                        validate_response_name(&filtered_custom_name)?;
                        // The stack net ID is a double optional now: absent means
                        // the server did not track this slot, which the app models
                        // as -1 rather than as a rejection.
                        let item_stack_id = match slot.item_stack_net_id {
                            None => -1,
                            Some(net_id) if net_id.id >= 0 => net_id.id,
                            Some(net_id) => {
                                return Err(InventoryPacketError::InvalidStackNetworkId(net_id.id));
                            }
                        };
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

pub fn validate_item_nbt_size(bytes: usize) -> Result<(), InventoryPacketError> {
    if bytes > MAX_ITEM_NBT_BYTES {
        return Err(InventoryPacketError::ItemNbtTooLarge {
            bytes,
            max: MAX_ITEM_NBT_BYTES,
        });
    }
    Ok(())
}

pub(crate) fn validate_raw_inventory_packet(
    raw: &jolyne::raw::RawPacket,
) -> Result<(), InventoryPacketError> {
    let mut body = raw.body().clone();
    match raw.id {
        McpePacketName::InventoryContentPacket => {
            read_var_i32(&mut body)?;
            let count = read_count(&mut body)?;
            validate_slot_count(count)?;
            for _ in 0..count {
                scan_item_descriptor(&mut body)?;
            }
            scan_full_container(&mut body)?;
            scan_item_descriptor(&mut body)?;
        }
        McpePacketName::InventorySlotPacket => {
            // The container ID is a plain byte in 1.26.40, not a varint.
            take_u8(&mut body)?;
            read_var_i32(&mut body)?;
            if read_presence(&mut body)? {
                scan_full_container(&mut body)?;
            }
            if read_presence(&mut body)? {
                scan_item_descriptor(&mut body)?;
            }
            scan_item_descriptor(&mut body)?;
        }
        McpePacketName::ItemStackResponsePacket => scan_stack_responses(&mut body)?,
        _ => {}
    }
    Ok(())
}

fn scan_stack_responses(body: &mut Bytes) -> Result<(), InventoryPacketError> {
    let response_count = read_count(body)?;
    if response_count > MAX_STACK_RESPONSES {
        return Err(InventoryPacketError::TooManyResponses {
            count: response_count,
            max: MAX_STACK_RESPONSES,
        });
    }
    for _ in 0..response_count {
        take_u8(body)?;
        read_var_i32(body)?;
        // The generated DoubleOptionalFunc shape carries its constant outer
        // flag and then the actual optional-list presence byte.
        read_presence(body)?;
        if !read_presence(body)? {
            continue;
        }
        let container_count = read_count(body)?;
        if container_count > MAX_RESPONSE_CONTAINERS {
            return Err(InventoryPacketError::TooManyResponseContainers {
                count: container_count,
                max: MAX_RESPONSE_CONTAINERS,
            });
        }
        for _ in 0..container_count {
            scan_full_container(body)?;
            let slot_count = read_count(body)?;
            if slot_count > MAX_CONTAINER_SLOTS {
                return Err(InventoryPacketError::TooManyResponseSlots {
                    count: slot_count,
                    max: MAX_CONTAINER_SLOTS,
                });
            }
            for _ in 0..slot_count {
                // requested_slot, slot, amount
                take_bytes(body, 3)?;
                // The stack net ID is another DoubleOptionalFunc: consume its
                // constant outer flag before the actual optional presence.
                read_presence(body)?;
                if read_presence(body)? {
                    read_var_i32(body)?;
                }
                // The two custom names are one redactable string, but both halves
                // are unconditional adjacent strings on the wire.
                scan_response_name(body)?;
                scan_response_name(body)?;
                read_var_i32(body)?;
            }
        }
    }
    Ok(())
}

fn scan_response_name(body: &mut Bytes) -> Result<(), InventoryPacketError> {
    let length = read_count(body)?;
    if length > MAX_RESPONSE_NAME_BYTES {
        return Err(InventoryPacketError::ResponseNameTooLong {
            bytes: length,
            max: MAX_RESPONSE_NAME_BYTES,
        });
    }
    take_bytes(body, length)
}

/// Walks one item descriptor without materialising it.
///
/// Protocol 1001 needed a scanner per item encoding; 1.26.40 has one shape. The
/// layout is `id: i16 LE`, `stacksize: u16 LE`, `auxvalue` varint, an optional
/// net ID (presence byte then one zigzag varint -- the old model wrote two
/// varints here for its `empty`/`id` pair), `block_runtime_id` varint, and the
/// length-prefixed user-data buffer.
fn scan_item_descriptor(body: &mut Bytes) -> Result<(), InventoryPacketError> {
    take_bytes(body, 4)?;
    read_var_i32(body)?;
    if read_presence(body)? {
        read_var_i32(body)?;
    }
    read_var_i32(body)?;
    scan_item_extra(body)
}

fn scan_item_extra(body: &mut Bytes) -> Result<(), InventoryPacketError> {
    let bytes = read_count(body)?;
    if bytes > MAX_ITEM_EXTRA_BYTES {
        return Err(InventoryPacketError::ItemExtraTooLarge {
            bytes,
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    take_bytes(body, bytes)
}

fn scan_full_container(body: &mut Bytes) -> Result<(), InventoryPacketError> {
    take_u8(body)?;
    if read_presence(body)? {
        take_bytes(body, 4)?;
    }
    Ok(())
}

fn read_presence(body: &mut Bytes) -> Result<bool, InventoryPacketError> {
    Ok(take_u8(body)? != 0)
}

fn take_u8(body: &mut Bytes) -> Result<u8, InventoryPacketError> {
    if !body.has_remaining() {
        return Err(InventoryPacketError::MalformedWire);
    }
    Ok(body.get_u8())
}

fn take_bytes(body: &mut Bytes, bytes: usize) -> Result<(), InventoryPacketError> {
    if body.remaining() < bytes {
        return Err(InventoryPacketError::MalformedWire);
    }
    body.advance(bytes);
    Ok(())
}

fn read_count(body: &mut Bytes) -> Result<usize, InventoryPacketError> {
    let value = read_var_i32(body)?;
    usize::try_from(value).map_err(|_| InventoryPacketError::MalformedWire)
}

fn read_var_i32(body: &mut Bytes) -> Result<i32, InventoryPacketError> {
    wire::read_var_u32(body)
        .map(|value| i32::from_ne_bytes(value.to_ne_bytes()))
        .map_err(|_| InventoryPacketError::MalformedWire)
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
    let actor_runtime_id = u64::try_from(packet.target_runtime_id.actor_runtime_id)
        .ok()
        .filter(|id| *id != 0)
        .ok_or(InventoryPacketError::InvalidArmorRuntimeId(
            packet.target_runtime_id.actor_runtime_id,
        ))?;
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

/// Bounds an item's user-data buffer and checks the compound it may carry.
///
/// 1.26.40 hands this over as opaque bytes, so the header is read exactly as
/// gophertunnel's `Writer.itemUserData` writes it
/// (`minecraft/protocol/writer.go`): an `int16` of `-1` introduces a `uint8`
/// version and a fixed little-endian compound, `0` means no compound. The
/// trailing canPlaceOn/canBreak lists and shield blocking tick are carried
/// through verbatim and never re-encoded field-by-field, which is what made the
/// protocol-1001 per-string length checks necessary.
fn validate_item_user_data(extra: &[u8]) -> Result<(), InventoryPacketError> {
    if extra.len() > MAX_ITEM_NBT_BYTES {
        return Err(InventoryPacketError::ItemExtraTooLarge {
            bytes: extra.len(),
            max: MAX_ITEM_NBT_BYTES,
        });
    }
    if extra.is_empty() {
        return Ok(());
    }
    let header = extra
        .get(..2)
        .ok_or(InventoryPacketError::InvalidItemExtra)?;
    match i16::from_le_bytes([header[0], header[1]]) {
        0 => Ok(()),
        -1 => {
            let version = *extra.get(2).ok_or(InventoryPacketError::InvalidItemExtra)?;
            if version != 1 {
                return Err(InventoryPacketError::UnsupportedItemNbtVersion(version));
            }
            // Only the compound is validated; the lists that follow it in the
            // same buffer mean trailing bytes are expected here.
            let mut bytes = Bytes::copy_from_slice(&extra[3..]);
            Nbt::decode_little_endian(&mut bytes)
                .map_err(|_| InventoryPacketError::InvalidItemExtra)?;
            Ok(())
        }
        _ => Err(InventoryPacketError::InvalidItemExtra),
    }
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
    let mut identity = full.map_or(
        Ok(ContainerIdentity {
            window_id: None,
            slot_type: None,
            dynamic_id: None,
        }),
        full_container_identity,
    )?;
    identity.window_id = Some(raw_window_id_varint(window_id)?);
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
