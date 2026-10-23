//! Observations of outbound packets other than transactions and recipes, and world-packet
//! fixtures normalized through the real decoder.

use bedrock_protocol_1_26_51::enums::{DataItemType, PlayStatusType};
use bedrock_protocol_1_26_51::packets::{
    self, AddActor, ChunkRadiusUpdated, ItemRegistry, ModalFormRequest, MoveActorAbsolute,
    MoveActorDelta, PlayStatus,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, ActorUniqueID, DataItemEntry, DataItemEntryValue, MoveActorAbsoluteData,
    MoveActorDeltaData, SynchedActorDataCopyableDataList,
};
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};

use crate::{Packet, WorldEvent, WorldPacketError, into_world_event};

/// Builds both terrain packet shapes with the requested blob-cache mode.
pub fn terrain_cache_packets(cache_enabled: bool) -> [Packet; 2] {
    [
        Packet::new(packets::LevelChunk {
            cache_enabled,
            ..Default::default()
        }),
        Packet::new(packets::SubChunk {
            cache_enabled,
            ..Default::default()
        }),
    ]
}

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
    let packets::Packet::Emote(emote) = &*packet.data else {
        return None;
    };
    Some(EmoteObservation {
        actor_runtime_id: emote.actor_runtime_id.0,
        emote_id: emote.emote_id.to_string_lossy().into_owned(),
        length_ticks: emote.emote_length_ticks.0,
        flags: emote.flags.0,
    })
}

/// The geometry engine version a PlayerSkin `packet` declares, or `None` for any other packet.
#[must_use]
pub fn skin_engine_version(packet: &Packet) -> Option<&str> {
    let packets::Packet::PlayerSkin(skin) = &*packet.data else {
        return None;
    };
    skin.serialized_skin
        .geometry_data_min_engine_version
        .to_str()
        .ok()
}

/// The nine movement attributes a ClientMovementPredictionSync `packet` reports.
#[must_use]
pub fn prediction_sync_attributes(packet: &Packet) -> Option<[f32; 9]> {
    let packets::Packet::ClientMovementPredictionSync(sync) = &*packet.data else {
        return None;
    };
    Some(sync.movement_attributes.map(|value| value.0))
}

/// The held-item update a MobEquipment `packet` sends, or `None` for any other packet.
#[must_use]
pub fn equipment(packet: &Packet) -> Option<EquipmentObservation> {
    let packets::Packet::MobEquipment(equipment) = &*packet.data else {
        return None;
    };
    Some(EquipmentObservation {
        actor_runtime_id: equipment.target_runtime_id.0,
        network_id: i32::from(equipment.item.id.0),
        count: equipment.item.stack_size.0,
        stack_network_id: equipment.item.net_id_variant.map(|id| id.0),
        slot: equipment.slot.0,
        selected_slot: equipment.selected_slot.0,
        container_id: equipment.container_id.0,
    })
}

/// The window a ContainerClose `packet` closes, or `None` for any other packet.
#[must_use]
pub fn container_close(packet: &Packet) -> Option<ContainerCloseObservation> {
    let packets::Packet::ContainerClose(close) = &*packet.data else {
        return None;
    };
    Some(ContainerCloseObservation {
        container_id: close.container_id.0,
        server_initiated: close.server_initiated_close,
    })
}

/// The crafter slot a toggle request `packet` changes, or `None` for any other packet.
#[must_use]
pub fn crafter_slot_toggle(packet: &Packet) -> Option<CrafterSlotToggleObservation> {
    let packets::Packet::PlayerToggleCrafterSlotRequest(toggle) = &*packet.data else {
        return None;
    };
    Some(CrafterSlotToggleObservation {
        position: [toggle.pos_x.0, toggle.pos_y.0, toggle.pos_z.0],
        slot: toggle.slot_index.0,
        disabled: toggle.is_disabled,
    })
}

/// A server ModalFormRequest carrying `json` as form `form_id`.
#[must_use]
pub fn modal_form_request_packet(form_id: u32, json: &str) -> Packet {
    crate::Packet::new(ModalFormRequest {
        form_id: wire::VarUInt(form_id),
        form_ui_json: json.into(),
    })
}

/// An empty server ItemRegistry.
#[must_use]
pub fn item_registry_packet() -> Packet {
    crate::Packet::new(ItemRegistry::default())
}

/// A server ChunkRadiusUpdated granting `chunk_radius`.
#[must_use]
pub fn chunk_radius_updated_packet(chunk_radius: i32) -> Packet {
    crate::Packet::new(ChunkRadiusUpdated {
        chunk_radius: wire::ZigZag32(chunk_radius),
    })
}

/// The PlayStatus that completes a join at player spawn.
#[must_use]
pub fn player_spawn_packet() -> Packet {
    crate::Packet::new(PlayStatus {
        status: PlayStatusType::Playerspawn,
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
    let packet = round_trip(MoveActorDelta {
        move_data: MoveActorDeltaData {
            actor_runtime_id: ActorRuntimeID(delta.runtime_id),
            new_position_x: delta.position[0].map(wire::F32LE),
            new_position_y: delta.position[1].map(wire::F32LE),
            new_position_z: delta.position[2].map(wire::F32LE),
            rotation_x: delta.rotation[0].map(wire::I8),
            rotation_y: delta.rotation[1].map(wire::I8),
            rotation_y_head: delta.rotation[2].map(wire::I8),
            ticks: wire::VarULong(delta.ticks),
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
    let packet = round_trip(MoveActorAbsolute {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeID(absolute.runtime_id),
            header: wire::U8(absolute.flags),
            position: absolute.position.into(),
            rotation_x: wire::U8(absolute.rotation[0]),
            rotation_y: wire::U8(absolute.rotation[1]),
            rotation_y_head: wire::U8(absolute.rotation[2]),
        },
    });
    into_world_event(crate::Packet::new(packet), 0)
}

/// Normalizes a dedicated AddPainting packet without exposing vendor types to fixture consumers.
pub fn add_painting_event(
    unique_id: i64,
    runtime_id: u64,
    motive: &str,
    direction: i32,
    position: [f32; 3],
) -> Result<Option<WorldEvent>, WorldPacketError> {
    into_world_event(
        crate::Packet::new(bedrock_protocol_1_26_51::packets::AddPainting {
            target_actor_id: ActorUniqueID(unique_id),
            target_runtime_id: ActorRuntimeID(runtime_id),
            position: position.into(),
            direction: wire::ZigZag32(direction),
            motif: motive.into(),
        }),
        0,
    )
}

/// Normalizes an AddActor spawning `identifier` with short-typed `metadata`, in dimension 0.
pub fn add_actor_event(
    unique_id: i64,
    runtime_id: u64,
    identifier: &str,
    metadata: &[(u32, i16)],
) -> Result<Option<WorldEvent>, WorldPacketError> {
    let packet = AddActor {
        target_actor_id: ActorUniqueID(unique_id),
        target_runtime_id: ActorRuntimeID(runtime_id),
        actor_type: identifier.into(),
        actor_data: SynchedActorDataCopyableDataList {
            data: metadata
                .iter()
                .map(|&(id, value)| DataItemEntry {
                    id: wire::VarUInt(id),
                    payload: DataItemEntryValue::DataItemShort {
                        type_: DataItemType::Short,
                        value: wire::I16LE(value),
                    },
                })
                .collect(),
        },
        ..Default::default()
    };
    into_world_event(crate::Packet::new(packet), 0)
}

/// Encodes and decodes one packet body so fixtures carry exactly what the wire can.
fn round_trip<T: Encode + Decode>(packet: T) -> T {
    let bytes = packet.encode_to_vec().expect("a fixture packet encodes");
    T::decode_exact(&bytes).expect("an encoded fixture decodes")
}

/// A successful acknowledgement with no corrections, in either optional-list encoding.
pub fn empty_stack_response(request_id: i32, list_present: bool) -> Packet {
    Packet::new(round_trip(packets::ItemStackResponse {
        responses: vec![bedrock_protocol_1_26_51::types::ItemStackResponseInfo {
            result: bedrock_protocol_1_26_51::enums::ItemStackNetResult::Success,
            client_request_id: bedrock_protocol_1_26_51::types::ItemStackRequestID(request_id),
            containers: list_present.then(Vec::new),
        }],
    }))
}

/// Reads server-authoritative predicted destroys in the order carried by one input packet.
pub fn predicted_block_breaks(packet: &Packet) -> Vec<[i32; 3]> {
    input_block_actions(packet)
        .into_iter()
        .filter_map(|(kind, position, _)| {
            (kind == crate::BlockActionKind::PredictDestroy.wire_value()).then_some(position)
        })
        .collect()
}

/// Reads tick-ordered block actions without interpreting their wire kind or abort percent.
pub fn input_block_actions(packet: &Packet) -> Vec<(i32, [i32; 3], i32)> {
    let packets::Packet::PlayerAuthInput(input) = &*packet.data else {
        return Vec::new();
    };
    input
        .player_block_actions
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|action| {
            (
                action.player_action_type.to_raw(),
                [
                    action.position.x.0,
                    action.position.y.0,
                    action.position.z.0,
                ],
                action.facing.0,
            )
        })
        .collect()
}

/// Reads the runtime ID and optional world-space point of an entity-hover report.
pub fn entity_hover(packet: &Packet) -> Option<(u64, Option<[f32; 3]>)> {
    let packets::Packet::Interact(interact) = &*packet.data else {
        return None;
    };
    (interact.action == bedrock_protocol_1_26_51::enums::InteractAction::Interactupdate).then(
        || {
            (
                interact.target_runtime_id.0,
                interact.position.map(|point| point.to_array()),
            )
        },
    )
}

/// Reads the actor runtime ID and swing source of an arm-swing animation.
pub fn swing(packet: &Packet) -> Option<(u64, Option<String>)> {
    let packets::Packet::Animate(animate) = &*packet.data else {
        return None;
    };
    (animate.action == bedrock_protocol_1_26_51::enums::AnimateAction::Swing).then(|| {
        (
            animate.target_actor_runtime_id.0,
            animate
                .swing_source
                .as_ref()
                .map(|source| source.to_string_lossy().into_owned()),
        )
    })
}
