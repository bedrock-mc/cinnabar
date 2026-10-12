//! Observations of outbound packets other than transactions and recipes, and world-packet
//! fixtures normalized through the real decoder.

use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, ActorUniqueId, AddActorPacket, ChunkRadiusUpdatedPacket, DataItemEntry,
    DataItemEntryPayload, DataItemShortPayload, EnumsDataItemType, EnumsPlayStatus,
    ItemRegistryPacket, McpePacketData, ModalFormRequestPacket, MoveActorAbsoluteData,
    MoveActorAbsolutePacket, MoveActorDeltaData, MoveActorDeltaPacket, PlayStatusPacket,
    SynchedActorDataCopyableDataList, Vec3,
};

use crate::{Packet, WorldEvent, WorldPacketError, into_world_event};

/// An outbound Emote packet's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmoteObservation {
    pub actor_runtime_id: u64,
    pub emote_id: String,
    pub length_ticks: u32,
    pub flags: u8,
}

/// An outbound MobEquipment packet's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EquipmentObservation {
    pub actor_runtime_id: u64,
    pub network_id: i32,
    pub count: u16,
    /// The stack network ID the descriptor carries, if any.
    pub stack_network_id: Option<i32>,
    pub slot: u8,
    pub selected_slot: u8,
    pub container_id: u8,
}

/// An outbound ContainerClose packet's fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerCloseObservation {
    pub container_id: u8,
    pub server_initiated: bool,
}

/// An outbound crafter slot toggle request's fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrafterSlotToggleObservation {
    pub position: [i32; 3],
    pub slot: u8,
    pub disabled: bool,
}

/// The emote `packet` starts, or `None` for any other packet.
#[must_use]
pub fn emote(packet: &Packet) -> Option<EmoteObservation> {
    let McpePacketData::EmotePacket(emote) = &packet.data else {
        return None;
    };
    Some(EmoteObservation {
        actor_runtime_id: emote.actor_runtime_id.actor_runtime_id,
        emote_id: emote.emote_id.clone(),
        length_ticks: emote.emote_length_ticks,
        flags: emote.flags,
    })
}

/// The geometry engine version a PlayerSkin `packet` declares, or `None` for any other packet.
#[must_use]
pub fn skin_engine_version(packet: &Packet) -> Option<&str> {
    let McpePacketData::PlayerSkinPacket(skin) = &packet.data else {
        return None;
    };
    Some(
        skin.serialized_skin
            .geometry_data_min_engine_version
            .as_str(),
    )
}

/// The nine movement attributes a ClientMovementPredictionSync `packet` reports.
#[must_use]
pub fn prediction_sync_attributes(packet: &Packet) -> Option<[f32; 9]> {
    let McpePacketData::ClientMovementPredictionSyncPacket(sync) = &packet.data else {
        return None;
    };
    Some(sync.movement_attributes)
}

/// The held-item update a MobEquipment `packet` sends, or `None` for any other packet.
#[must_use]
pub fn equipment(packet: &Packet) -> Option<EquipmentObservation> {
    let McpePacketData::MobEquipmentPacket(equipment) = &packet.data else {
        return None;
    };
    Some(EquipmentObservation {
        actor_runtime_id: equipment.target_runtime_id.actor_runtime_id,
        network_id: i32::from(equipment.item.id),
        count: equipment.item.stacksize,
        stack_network_id: equipment.item.net_id_variant,
        slot: equipment.slot,
        selected_slot: equipment.selected_slot,
        container_id: equipment.container_id,
    })
}

/// The window a ContainerClose `packet` closes, or `None` for any other packet.
#[must_use]
pub fn container_close(packet: &Packet) -> Option<ContainerCloseObservation> {
    let McpePacketData::ContainerClosePacket(close) = &packet.data else {
        return None;
    };
    Some(ContainerCloseObservation {
        container_id: close.container_id,
        server_initiated: close.server_initiated_close,
    })
}

/// The crafter slot a toggle request `packet` changes, or `None` for any other packet.
#[must_use]
pub fn crafter_slot_toggle(packet: &Packet) -> Option<CrafterSlotToggleObservation> {
    let McpePacketData::PlayerToggleCrafterSlotRequestPacket(toggle) = &packet.data else {
        return None;
    };
    Some(CrafterSlotToggleObservation {
        position: [toggle.pos_x, toggle.pos_y, toggle.pos_z],
        slot: toggle.slot_index,
        disabled: toggle.is_disabled,
    })
}

/// A server ModalFormRequest carrying `json` as form `form_id`.
#[must_use]
pub fn modal_form_request_packet(form_id: u32, json: &str) -> Packet {
    crate::Packet::new(ModalFormRequestPacket {
        form_id,
        form_uijson: json.to_owned(),
    })
}

/// An empty server ItemRegistry.
#[must_use]
pub fn item_registry_packet() -> Packet {
    crate::Packet::new(ItemRegistryPacket::default())
}

/// A server ChunkRadiusUpdated granting `chunk_radius`.
#[must_use]
pub fn chunk_radius_updated_packet(chunk_radius: i32) -> Packet {
    crate::Packet::new(ChunkRadiusUpdatedPacket { chunk_radius })
}

/// The PlayStatus that completes a join at player spawn.
#[must_use]
pub fn player_spawn_packet() -> Packet {
    crate::Packet::new(PlayStatusPacket {
        status: EnumsPlayStatus::Playerspawn,
    })
}

/// A MoveActorDelta's fields; `None` leaves a component unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActorDeltaFixture {
    pub runtime_id: u64,
    pub position: [Option<f32>; 3],
    /// Pitch, yaw and head yaw in wire angle steps.
    pub rotation: [Option<i8>; 3],
    pub ticks: u64,
    pub force_completion: bool,
}

/// A MoveActorAbsolute's fields.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActorAbsoluteFixture {
    pub runtime_id: u64,
    /// The wire flag byte (on-ground, teleport, force-move bits).
    pub flags: u8,
    pub position: [f32; 3],
    /// Pitch, yaw and head yaw in wire angle steps.
    pub rotation: [u8; 3],
}

/// Normalizes `delta` after a wire round trip, as a dimension-0 world event.
pub fn move_actor_delta_event(
    delta: ActorDeltaFixture,
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let packet = round_trip(MoveActorDeltaPacket {
        move_data: MoveActorDeltaData {
            actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: delta.runtime_id,
            },
            new_position_x: delta.position[0],
            new_position_y: delta.position[1],
            new_position_z: delta.position[2],
            rotation_x: delta.rotation[0],
            rotation_y: delta.rotation[1],
            rotation_y_head: delta.rotation[2],
            ticks: delta.ticks,
            force_completion: delta.force_completion,
            ..Default::default()
        },
    });
    into_world_event(crate::Packet::new(packet), 0)
}

/// Normalizes `absolute` after a wire round trip, as a dimension-0 world event.
pub fn move_actor_absolute_event(
    absolute: ActorAbsoluteFixture,
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let [x, y, z] = absolute.position;
    let packet = round_trip(MoveActorAbsolutePacket {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: absolute.runtime_id,
            },
            header: absolute.flags,
            position: Vec3 { x, y, z },
            rotation_x: absolute.rotation[0],
            rotation_y: absolute.rotation[1],
            rotation_y_head: absolute.rotation[2],
        },
    });
    into_world_event(crate::Packet::new(packet), 0)
}

/// Normalizes an AddActor spawning `identifier` with short-typed `metadata`, in dimension 0.
pub fn add_actor_event(
    unique_id: i64,
    runtime_id: u64,
    identifier: &str,
    metadata: &[(u32, i16)],
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let packet = AddActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: unique_id,
        },
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        actor_type: identifier.to_owned(),
        actor_data: SynchedActorDataCopyableDataList {
            data: metadata
                .iter()
                .map(|&(id, value)| DataItemEntry {
                    id,
                    payload: DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                        type_: EnumsDataItemType::Short,
                        value,
                    }),
                })
                .collect(),
        },
        ..Default::default()
    };
    into_world_event(crate::Packet::new(packet), 0)
}

/// Encodes and decodes one packet body so fixtures carry exactly what the wire can.
fn round_trip<T: BedrockCodec<Args = ()>>(packet: T) -> T {
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).expect("a fixture packet encodes");
    T::decode(&mut bytes.as_slice(), ()).expect("an encoded fixture decodes")
}
