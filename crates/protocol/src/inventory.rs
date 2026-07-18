use std::sync::Arc;

use bytes::{Buf, Bytes, BytesMut};
use sha2::{Digest, Sha256};
use thiserror::Error;
use valentine::bedrock::{
    codec::{BedrockCodec, BedrockSized, Nbt, VarInt},
    version::v1_26_30::{
        ContainerClosePacket, ContainerOpenPacket, ContainerSetDataPacket, ContainerSlotType,
        FullContainerName, InventoryContentPacket, InventorySlotPacket,
        ItemExtraDataWithBlockingTick, ItemExtraDataWithoutBlockingTick,
        ItemExtraDataWithoutBlockingTickNbt, ItemNew, ItemNewExtra, ItemNewStackId,
        ItemStackResponsePacket, ItemStackResponsesItemStatus, ItemV4, ItemV4NetIdVariantType,
        PlayerHotbarPacket, WindowId, WindowIdVarint, WindowType,
    },
};

use crate::item::NetworkItemStack;

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
    #[error("verified item extra data cannot be decoded for protocol 1001")]
    InvalidItemExtra,
    #[error("item extra string has {bytes} bytes, exceeding {max}")]
    ItemExtraStringTooLarge { bytes: usize, max: usize },
    #[error("failed to encode validated inventory packet data")]
    EncodingFailed,
    #[error("item retained-byte digest does not match")]
    DigestMismatch,
    #[error("empty item has contradictory retained fields")]
    ContradictoryEmptyItem,
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
    ) -> Result<ItemNew, InventoryPacketError> {
        if self.inner.is_empty() {
            return Ok(ItemNew::default());
        }
        let network_id = i16::try_from(self.inner.network_id)
            .map_err(|_| InventoryPacketError::InvalidItemNetworkId(self.inner.network_id))?;
        let stack_id = (self.inner.stack_network_id != -1).then_some(ItemNewStackId {
            empty: 0,
            id: self.inner.stack_network_id,
        });
        let mut extra_bytes = Bytes::copy_from_slice(&self.inner.extra_data);
        let extra = if self.inner.network_id == shield_item_id {
            let extra = ItemExtraDataWithBlockingTick::decode(&mut extra_bytes, ())
                .map_err(|_| InventoryPacketError::InvalidItemExtra)?;
            ItemNewExtra::ShieldItemId(extra)
        } else {
            let extra = ItemExtraDataWithoutBlockingTick::decode(&mut extra_bytes, ())
                .map_err(|_| InventoryPacketError::InvalidItemExtra)?;
            ItemNewExtra::Default(extra)
        };
        if extra_bytes.has_remaining() {
            return Err(InventoryPacketError::InvalidItemExtra);
        }
        Ok(ItemNew {
            network_id,
            count: self.inner.count,
            metadata: i32::from_ne_bytes(self.inner.metadata.to_ne_bytes()),
            stack_id,
            block_runtime_id: self.inner.block_runtime_id,
            extra,
        })
    }
}

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
    validate_slot_count(packet.input.len())?;
    let container = container_identity_varint(packet.window_id, Some(packet.container))?;
    let slots = packet
        .input
        .into_iter()
        .map(normalize_item_v4)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: Arc::from(slots),
        storage_item: normalize_item_v4(packet.storage_item)?,
    }))
}

pub fn normalize_slot(packet: InventorySlotPacket) -> Result<InventoryEvent, InventoryPacketError> {
    let slot = checked_slot(packet.slot)?;
    let container = container_identity_varint(packet.window_id, packet.container)?;
    Ok(InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity { container, slot },
        stack: normalize_item_new(packet.item)?,
        storage_item: packet.storage_item.map(normalize_item_new).transpose()?,
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
        container: ContainerIdentity::window(raw_window_id(packet.window_id)?),
        slot,
        select_slot: packet.select_slot,
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
        let (status, containers) = match (response.status, response.content) {
            (ItemStackResponsesItemStatus::Ok, Some(content)) => {
                if content.containers.len() > MAX_RESPONSE_CONTAINERS {
                    return Err(InventoryPacketError::TooManyResponseContainers {
                        count: content.containers.len(),
                        max: MAX_RESPONSE_CONTAINERS,
                    });
                }
                let mut containers = Vec::with_capacity(content.containers.len());
                for container in content.containers {
                    validate_slot_count(container.slots.len()).map_err(|error| match error {
                        InventoryPacketError::TooManySlots { count, max } => {
                            InventoryPacketError::TooManyResponseSlots { count, max }
                        }
                        other => other,
                    })?;
                    let identity = full_container_identity(container.slot_type)?;
                    let mut slots = Vec::with_capacity(container.slots.len());
                    for slot in container.slots {
                        validate_response_name(&slot.custom_name)?;
                        validate_response_name(&slot.filtered_custom_name)?;
                        if slot.item_stack_id < 0 {
                            return Err(InventoryPacketError::InvalidStackNetworkId(
                                slot.item_stack_id,
                            ));
                        }
                        slots.push(StackResponseSlot {
                            slot: slot.slot,
                            hotbar_slot: slot.hotbar_slot,
                            count: slot.count,
                            item_stack_id: slot.item_stack_id,
                            custom_name: Arc::from(slot.custom_name),
                            filtered_custom_name: Arc::from(slot.filtered_custom_name),
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
            (ItemStackResponsesItemStatus::Ok, None) => {
                return Err(InventoryPacketError::MissingResponseContent);
            }
            (ItemStackResponsesItemStatus::Error, None) => {
                (StackResponseStatus::Rejected, Vec::new())
            }
            (ItemStackResponsesItemStatus::Unknown(value), None) => {
                (StackResponseStatus::Unknown(value), Vec::new())
            }
            (_, Some(_)) => return Err(InventoryPacketError::UnexpectedResponseContent),
        };
        responses.push(StackResponse {
            status,
            request_id: response.request_id,
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
        container: ContainerIdentity::window(raw_window_id(packet.window_id)?),
        window_type: raw_window_type(packet.window_type)?,
        position: [
            packet.coordinates.x,
            packet.coordinates.y,
            packet.coordinates.z,
        ],
        runtime_entity_id: packet.runtime_entity_id,
    }))
}

pub fn normalize_container_close(
    packet: ContainerClosePacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    Ok(InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(raw_window_id(packet.window_id)?),
        window_type: raw_window_type(packet.window_type)?,
        server_initiated: packet.server,
    }))
}

pub fn normalize_container_data(
    packet: ContainerSetDataPacket,
) -> Result<InventoryEvent, InventoryPacketError> {
    Ok(InventoryEvent::Data(ContainerDataEvent {
        container: ContainerIdentity::window(raw_window_id(packet.window_id)?),
        property: packet.property,
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

fn normalize_item_v4(item: ItemV4) -> Result<NetworkItemStack, InventoryPacketError> {
    if item.network_id == 0 {
        if item.count == 0
            && item.metadata == 0
            && item.net_id_variant.is_none()
            && item.block_runtime_id == 0
            && item.extra_data.is_empty()
        {
            return Ok(NetworkItemStack::empty());
        }
        return Err(InventoryPacketError::ContradictoryEmptyItem);
    }
    let stack_network_id = match item.net_id_variant {
        None => -1,
        Some(variant)
            if variant.type_ == ItemV4NetIdVariantType::ItemStackNetId && variant.id > 0 =>
        {
            variant.id
        }
        Some(_) => return Err(InventoryPacketError::ContradictoryStackId),
    };
    make_stack(
        i32::from(item.network_id),
        item.metadata,
        stack_network_id,
        item.count,
        item.block_runtime_id,
        item.extra_data,
    )
}

fn normalize_item_new(item: ItemNew) -> Result<NetworkItemStack, InventoryPacketError> {
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
        return Err(InventoryPacketError::ContradictoryEmptyItem);
    }
    let stack_network_id = match item.stack_id {
        None => -1,
        Some(stack_id) if stack_id.empty == 0 && stack_id.id > 0 => stack_id.id,
        Some(_) => return Err(InventoryPacketError::ContradictoryStackId),
    };
    let extra = match &item.extra {
        ItemNewExtra::Default(extra) => {
            validate_extra_without_blocking(extra)?;
            encode_extra(extra)?
        }
        ItemNewExtra::ShieldItemId(extra) => {
            validate_extra_with_blocking(extra)?;
            encode_extra(extra)?
        }
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

fn validate_extra_without_blocking(
    extra: &ItemExtraDataWithoutBlockingTick,
) -> Result<(), InventoryPacketError> {
    validate_extra_fields(extra.nbt.as_ref(), &extra.can_place_on, &extra.can_destroy)
}

fn validate_extra_with_blocking(
    extra: &ItemExtraDataWithBlockingTick,
) -> Result<(), InventoryPacketError> {
    validate_extra_fields(extra.nbt.as_ref(), &extra.can_place_on, &extra.can_destroy)
}

fn validate_extra_fields(
    nbt: Option<&ItemExtraDataWithoutBlockingTickNbt>,
    can_place_on: &[String],
    can_destroy: &[String],
) -> Result<(), InventoryPacketError> {
    if let Some(nbt) = nbt {
        if nbt.version != 1 {
            return Err(InventoryPacketError::UnsupportedItemNbtVersion(nbt.version));
        }
        validate_nbt(&nbt.nbt)?;
    }
    for value in can_place_on.iter().chain(can_destroy) {
        if value.len() > i16::MAX as usize {
            return Err(InventoryPacketError::ItemExtraStringTooLarge {
                bytes: value.len(),
                max: i16::MAX as usize,
            });
        }
    }
    Ok(())
}

fn validate_nbt(nbt: &Nbt) -> Result<(), InventoryPacketError> {
    validate_item_nbt_size(nbt.0.len())?;
    let mut bytes = nbt.0.clone();
    Nbt::decode_little_endian(&mut bytes).map_err(|_| InventoryPacketError::InvalidItemNbt)?;
    if bytes.has_remaining() {
        return Err(InventoryPacketError::InvalidItemNbt);
    }
    Ok(())
}

fn encode_extra<T>(value: &T) -> Result<Vec<u8>, InventoryPacketError>
where
    T: BedrockCodec<Args = ()> + BedrockSized,
{
    let size = value.encoded_size();
    if size > MAX_ITEM_EXTRA_BYTES {
        return Err(InventoryPacketError::ItemExtraTooLarge {
            bytes: size,
            max: MAX_ITEM_EXTRA_BYTES,
        });
    }
    let mut bytes = BytesMut::with_capacity(size);
    value
        .encode(&mut bytes)
        .map_err(|_| InventoryPacketError::EncodingFailed)?;
    Ok(bytes.to_vec())
}

fn container_identity_varint(
    window_id: WindowIdVarint,
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
        slot_type: Some(raw_container_slot(full.container_id)?),
        dynamic_id: full.dynamic_container_id,
    })
}

fn raw_window_id(value: WindowId) -> Result<i32, InventoryPacketError> {
    let mut bytes = BytesMut::with_capacity(1);
    value
        .encode(&mut bytes)
        .map_err(|_| InventoryPacketError::EncodingFailed)?;
    Ok(i32::from(i8::from_ne_bytes([bytes[0]])))
}

fn raw_window_id_varint(value: WindowIdVarint) -> Result<i32, InventoryPacketError> {
    let mut bytes = BytesMut::with_capacity(value.encoded_size());
    value
        .encode(&mut bytes)
        .map_err(|_| InventoryPacketError::EncodingFailed)?;
    VarInt::decode(&mut bytes.freeze(), ())
        .map(|raw| raw.0)
        .map_err(|_| InventoryPacketError::EncodingFailed)
}

fn raw_container_slot(value: ContainerSlotType) -> Result<u8, InventoryPacketError> {
    let mut bytes = BytesMut::with_capacity(1);
    value
        .encode(&mut bytes)
        .map_err(|_| InventoryPacketError::EncodingFailed)?;
    Ok(bytes[0])
}

fn raw_window_type(value: WindowType) -> Result<i8, InventoryPacketError> {
    let mut bytes = BytesMut::with_capacity(1);
    value
        .encode(&mut bytes)
        .map_err(|_| InventoryPacketError::EncodingFailed)?;
    Ok(i8::from_ne_bytes([bytes[0]]))
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
