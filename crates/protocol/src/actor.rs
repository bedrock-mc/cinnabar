use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::{
    ActorLinkType as VendorActorLinkType, MobEffectEvent as MobEffectPacketEventId,
};
use bedrock_protocol_1_26_51::packets::{
    AddActor as AddActorPacket, AddPlayer as AddPlayerPacket, MobEffect as MobEffectPacket,
    MoveActorAbsolute as MoveActorAbsolutePacket, MoveActorDelta as MoveActorDeltaPacket,
    PlayerList as PlayerListPacket, RemoveActor as RemoveActorPacket,
    SetActorData as SetActorDataPacket, SetActorLink as SetActorLinkPacket,
    UpdateAttributes as UpdateAttributesPacket,
};
use bedrock_protocol_1_26_51::types::{
    AttributeData, DataItemEntryValue, EntityLink as VendorActorLink, PlayerListData,
    PropertySyncData, SyncedAttribute, SynchedActorDataCopyableDataList,
};
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;

use crate::{NetworkItemStack, item::normalize_item};

mod error;
pub(crate) mod identifiers;
pub use error::ActorPacketError;
mod painting;
pub(crate) use painting::normalize_add_painting;
pub use painting::{PaintingData, PaintingDirection};
mod skin;
pub use identifiers::{ActorIdentifier, ActorIdentifierRegistry, MAX_ACTOR_IDENTIFIERS};
mod skin_update;
pub(crate) use skin_update::normalize_skin_update;
mod status;
use skin::normalize_player_skin;
pub use skin::{
    PlayerSkin, PlayerSkinUnavailable, normalize_classic_skin_rgba8, normalize_custom_skin_rgba8,
};
pub use status::{ActorStatusEvent, ActorStatusKind, ActorTakeItemEvent};
pub(crate) use status::{
    normalize_actor_event, normalize_add_item_actor, normalize_take_item_actor,
};

pub const MAX_ACTOR_IDENTIFIER_BYTES: usize = 256;
pub const MAX_ACTOR_NAME_BYTES: usize = 256;
pub const MAX_ACTOR_METADATA_ENTRIES: usize = 256;
pub const MAX_ACTOR_ATTRIBUTES: usize = 128;
pub const MAX_ACTOR_PROPERTIES: usize = 256;
/// Local normalization ceiling for the links retained from one spawn packet.
pub const MAX_ACTOR_LINKS_PER_SPAWN: usize = 256;
pub const MAX_ACTOR_ATTRIBUTE_MODIFIERS: usize = 64;
/// Longest metadata string kept; longer ones are skipped. Server name tags carry several
/// formatted lines, so this stays well above any one line of text.
pub const MAX_ACTOR_METADATA_STRING_BYTES: usize = 64 * 1024;
pub const MAX_ACTOR_METADATA_NBT_BYTES: usize = 1_048_576;
pub const MAX_PLAYER_LIST_RECORDS: usize = 4_096;
pub const MAX_PLAYER_LIST_SKIN_BYTES: usize = 64 * 1024 * 1024;

/// Native ItemActor origin above collision-box feet. Current ctor sets
/// collision height to .25 and this offset to half that height. AddItemActor and
/// both absolute/delta movement carry the native origin, not collision feet.
pub const ITEM_ACTOR_NETWORK_OFFSET: f32 = 0.125;

const ACTOR_DATA_ID_FLAGS: u32 = protocol_ids::ActorDataKey::Flags.wire_value();
const ACTOR_DATA_ID_FLAGS_EXTENDED: u32 = protocol_ids::ActorDataKey::FlagsTwo.wire_value();

/// Actor-data ID for flags starting at bit 128.
pub const ACTOR_DATA_ID_FLAGS_THIRD: u32 = protocol_ids::ActorDataKey::FlagsThird.wire_value();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorKind {
    Player { uuid: [u8; 16], username: Arc<str> },
    Entity { identifier: Arc<str> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorAttribute {
    pub name: Arc<str>,
    pub min: f32,
    pub max: f32,
    pub current: f32,
    pub default: Option<f32>,
    pub modifiers: Arc<[ActorAttributeModifier]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorAttributeModifier {
    pub id: Arc<str>,
    pub name: Arc<str>,
    pub amount: f32,
    pub operation: i32,
    pub operand: i32,
    pub serializable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActorProperty {
    Int { index: u32, value: i32 },
    Float { index: u32, value: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorMetadata {
    pub key: u32,
    pub value: ActorMetadataValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActorMetadataValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Float(f32),
    String(Arc<str>),
    Compound(Arc<[u8]>),
    BlockPosition([i32; 3]),
    Long(i64),
    Vector([f32; 3]),
    Flags(u64),
    FlagsExtended(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorSpawnEvent {
    pub dimension: i32,
    pub unique_id: i64,
    pub runtime_id: u64,
    pub kind: ActorKind,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub body_yaw: f32,
    pub held_item: NetworkItemStack,
    pub metadata: Arc<[ActorMetadata]>,
    pub attributes: Arc<[ActorAttribute]>,
    pub properties: Arc<[ActorProperty]>,
    pub links: Arc<[ActorLinkEvent]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorRemoveEvent {
    pub dimension: i32,
    pub unique_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorMoveEvent {
    pub dimension: i32,
    pub runtime_id: u64,
    pub position: [Option<f32>; 3],
    pub position_origin: ActorPositionOrigin,
    pub pitch: Option<f32>,
    pub yaw: Option<f32>,
    pub head_yaw: Option<f32>,
    pub on_ground: Option<bool>,
    pub teleported: bool,
    /// Accept this move even when the actor is controlled by the local client.
    pub force_local_move: bool,
    pub player_mode: Option<crate::MovePlayerMode>,
    pub source_tick: Option<u64>,
    pub interpolation: ActorInterpolation,
}

/// Server-requested actor movement duration and completion ordering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActorInterpolation {
    pub ticks: u64,
    pub force_completion: bool,
}

const MOVE_ON_GROUND: u8 = 1;
const MOVE_TELEPORT: u8 = 1 << 1;
const MOVE_FORCE_LOCAL: u8 = 1 << 2;
const MOVE_FORCE_COMPLETION: u8 = 1 << 3;

impl ActorMoveEvent {
    /// Whether server actor movement may replace a locally controlled position.
    pub const fn controls_local_actor(self) -> bool {
        self.teleported || self.force_local_move
    }
}

/// Coordinate space carried by an actor movement position.
///
/// Normalized spawn positions use the actor store's retained coordinate space.
/// Absolute and partial wire movement share the native actor origin; its offset
/// can be removed once actor kind is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActorPositionOrigin {
    /// The position is already in the actor store's retained coordinate space.
    #[default]
    Feet,
    /// The position came from an absolute or delta Bedrock movement packet.
    NetworkOffset,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorMetadataUpdateEvent {
    pub dimension: i32,
    pub runtime_id: u64,
    pub metadata: Arc<[ActorMetadata]>,
    pub properties: Arc<[ActorProperty]>,
    pub tick: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorAttributesUpdateEvent {
    pub dimension: i32,
    pub runtime_id: u64,
    pub attributes: Arc<[ActorAttribute]>,
    pub tick: u64,
}

/// MobEffect lifecycle verb, retained verbatim so an unknown verb can be
/// skipped downstream instead of guessed at.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActorEffectAction {
    Add,
    Update,
    Remove,
    Unknown(u8),
}

/// MobEffect id of Haste, which shortens arm swings.
pub const HASTE_EFFECT_ID: i32 = protocol_ids::Effect::Haste.wire_value();
/// MobEffect id of Mining Fatigue, which lengthens arm swings.
pub const MINING_FATIGUE_EFFECT_ID: i32 = protocol_ids::Effect::MiningFatigue.wire_value();

/// One server-authoritative MobEffect change. Effect and amplifier values are
/// retained raw; presentation decides which ids it can draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorEffectEvent {
    pub dimension: i32,
    pub actor_runtime_id: u64,
    pub action: ActorEffectAction,
    pub effect_id: i32,
    pub amplifier: i32,
    pub particles: bool,
    pub ambient: bool,
    /// Remaining duration in ticks. Bedrock uses negative values for
    /// effectively infinite effects, so the sign is preserved.
    pub duration_ticks: i32,
    pub tick: u64,
}

/// SetActorLink verb, retained verbatim for the same skip-not-guess reason.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActorLinkType {
    Remove,
    Rider,
    Passenger,
    Unknown(u8),
}

/// One server-authoritative rider/mount link change between two unique actor ids.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorLinkEvent {
    pub dimension: i32,
    pub ridden_unique_id: i64,
    pub rider_unique_id: i64,
    pub link_type: ActorLinkType,
    pub immediate: bool,
    pub rider_initiated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerListEntry {
    Add {
        uuid: [u8; 16],
        unique_id: i64,
        username: Arc<str>,
        verified: bool,
        /// Decimal Xbox user ID, empty when the server sent none or a malformed one.
        xuid: Arc<str>,
        skin: PlayerSkin,
    },
    Remove {
        uuid: [u8; 16],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerListUpdateEvent {
    pub entries: Arc<[PlayerListEntry]>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActorEvent {
    Identifiers(ActorIdentifierRegistry),
    Spawn(ActorSpawnEvent),
    PaintingSpawn {
        spawn: ActorSpawnEvent,
        painting: PaintingData,
    },
    PlayerSpawn {
        spawn: ActorSpawnEvent,
        game_mode: crate::GameModeUpdate,
    },
    Remove(ActorRemoveEvent),
    Move(ActorMoveEvent),
    Metadata(ActorMetadataUpdateEvent),
    Attributes(ActorAttributesUpdateEvent),
    PlayerList(PlayerListUpdateEvent),
    Skin {
        uuid: [u8; 16],
        skin: PlayerSkin,
    },
    Status(ActorStatusEvent),
    TakeItem(ActorTakeItemEvent),
}

pub(crate) fn normalize_add_entity(
    packet: AddActorPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    if packet.actor_type.len() > MAX_ACTOR_IDENTIFIER_BYTES {
        return Err(ActorPacketError::IdentifierTooLong {
            bytes: packet.actor_type.len(),
            max: MAX_ACTOR_IDENTIFIER_BYTES,
        });
    }
    // 1.26.40 packs pitch and yaw into a single `Vec2` written in that order.
    // gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
    // `minecraft/protocol/packet/add_actor.go`: `Float32(&pk.Pitch)` then
    // `Float32(&pk.Yaw)`, then HeadYaw and BodyYaw.
    let (pitch, yaw) = (packet.rotation.x, packet.rotation.y);
    for (field, value) in [
        ("position.x", packet.position.x),
        ("position.y", packet.position.y),
        ("position.z", packet.position.z),
        ("velocity.x", packet.velocity.x),
        ("velocity.y", packet.velocity.y),
        ("velocity.z", packet.velocity.z),
        ("pitch", pitch),
        ("yaw", yaw),
        ("head_yaw", packet.y_head_rotation.0),
        ("body_yaw", packet.y_body_rotation.0),
    ] {
        if !value.is_finite() {
            return Err(ActorPacketError::NonFiniteSpawnField { field });
        }
    }

    let metadata = normalize_metadata(packet.actor_data)?;
    let attributes = normalize_synced_attributes(packet.attributes_list)?;
    let properties = normalize_properties(packet.synched_properties)?;
    let links = normalize_actor_links(packet.actor_links, dimension)?;
    Ok(ActorEvent::Spawn(ActorSpawnEvent {
        dimension,
        unique_id: packet.target_actor_id.0,
        runtime_id: packet.target_runtime_id.0,
        kind: ActorKind::Entity {
            identifier: Arc::from(packet.actor_type.to_string_lossy()),
        },
        position: [packet.position.x, packet.position.y, packet.position.z],
        velocity: [packet.velocity.x, packet.velocity.y, packet.velocity.z],
        pitch,
        yaw,
        head_yaw: packet.y_head_rotation.0,
        body_yaw: packet.y_body_rotation.0,
        held_item: NetworkItemStack::empty(),
        metadata,
        attributes,
        properties,
        links,
    }))
}

pub(crate) fn normalize_add_player(
    packet: AddPlayerPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    validate_text("username", &packet.player_name, MAX_ACTOR_NAME_BYTES)?;
    // As on AddActor, pitch/yaw arrive as a single `Vec2` in that order.
    // gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
    // `minecraft/protocol/packet/add_player.go`.
    let (pitch, yaw) = (packet.rotation.x, packet.rotation.y);
    for (field, value) in [
        ("position.x", packet.position.x),
        ("position.y", packet.position.y),
        ("position.z", packet.position.z),
        ("velocity.x", packet.velocity.x),
        ("velocity.y", packet.velocity.y),
        ("velocity.z", packet.velocity.z),
        ("pitch", pitch),
        ("yaw", yaw),
        ("head_yaw", packet.y_head_rotation.0),
    ] {
        validate_finite(field, value)?;
    }
    let metadata = normalize_metadata(packet.entity_data)?;
    let properties = normalize_properties(packet.synched_properties)?;
    let held_item = normalize_item(packet.carried_item)?;
    let links = normalize_actor_links(packet.actor_links, dimension)?;
    let game_mode = crate::PlayerGameMode::update_from_game_mode(packet.player_game_type);
    Ok(ActorEvent::PlayerSpawn {
        game_mode,
        spawn: ActorSpawnEvent {
            dimension,
            // AddPlayer carries no standalone unique ID; the spawned player's unique
            // ID is the first field of the embedded ability data. Protocol 1001's
            // prismarine schema flattened that block, which is why the old code read
            // a top-level `unique_id`. gophertunnel
            // be6713da4dc051a4197f897d04835e89e9c54321
            // `minecraft/protocol/ability.go`: `AbilityData.EntityUniqueID`.
            unique_id: packet.abilities_data.target_player_raw_id.0,
            runtime_id: packet.target_runtime_id.0,
            kind: ActorKind::Player {
                uuid: *packet.uuid.as_bytes(),
                username: Arc::from(packet.player_name.to_string_lossy()),
            },
            position: [packet.position.x, packet.position.y, packet.position.z],
            velocity: [packet.velocity.x, packet.velocity.y, packet.velocity.z],
            pitch,
            yaw,
            head_yaw: packet.y_head_rotation.0,
            body_yaw: yaw,
            held_item,
            metadata,
            attributes: Arc::from([]),
            properties,
            links,
        },
    })
}

pub(crate) const fn normalize_remove_entity(
    packet: RemoveActorPacket,
    dimension: i32,
) -> ActorEvent {
    ActorEvent::Remove(ActorRemoveEvent {
        dimension,
        unique_id: packet.target_actor_id.0,
    })
}

pub(crate) fn normalize_move_entity(
    packet: MoveActorAbsolutePacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let move_data = packet.move_data;
    for (field, value) in [
        ("position.x", move_data.position.x),
        ("position.y", move_data.position.y),
        ("position.z", move_data.position.z),
    ] {
        validate_finite(field, value)?;
    }
    Ok(ActorEvent::Move(ActorMoveEvent {
        dimension,
        runtime_id: move_data.actor_runtime_id.0,
        position: [
            Some(move_data.position.x),
            Some(move_data.position.y),
            Some(move_data.position.z),
        ],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: Some(byte_rotation_degrees(move_data.rotation_x.0)),
        yaw: Some(byte_rotation_degrees(move_data.rotation_y.0)),
        head_yaw: Some(byte_rotation_degrees(move_data.rotation_y_head.0)),
        // `header` is the movement flag byte. gophertunnel
        // be6713da4dc051a4197f897d04835e89e9c54321
        // `minecraft/protocol/packet/move_actor_absolute.go`:
        // `MoveFlagOnGround = 1 << iota`, `MoveFlagTeleport`.
        on_ground: Some(move_data.header.0 & MOVE_ON_GROUND != 0),
        teleported: move_data.header.0 & MOVE_TELEPORT != 0,
        force_local_move: move_data.header.0 & MOVE_FORCE_LOCAL != 0,
        player_mode: None,
        source_tick: None,
        interpolation: ActorInterpolation {
            force_completion: move_data.header.0 & MOVE_FORCE_COMPLETION != 0,
            ..Default::default()
        },
    }))
}

/// Decodes the Bedrock MoveActorAbsolute body straight off the wire.
///
/// The generated `MoveActorAbsoluteData` is wire-correct (VarUInt64 runtime ID, then the flag
/// byte, the position and three raw rotation bytes); this is only the allocation-free fast path
/// the raw play loop uses, and must stay byte-for-byte identical to [`normalize_move_entity`].
pub(crate) fn normalize_move_entity_body(
    body: &Bytes,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    const FIXED_BODY_BYTES: usize = 1 + 3 * size_of::<f32>() + 3;

    let mut reader = wire::Reader::new(body);
    let runtime_id = reader
        .read_var_u64()
        .map_err(|_| ActorPacketError::InvalidAbsoluteMoveRuntimeId)?;
    let Ok(fixed) = reader.read_bytes::<FIXED_BODY_BYTES>() else {
        return Err(ActorPacketError::InvalidAbsoluteMoveLength {
            actual: reader.remaining(),
            expected: FIXED_BODY_BYTES,
        });
    };
    if !reader.is_empty() {
        return Err(ActorPacketError::InvalidAbsoluteMoveLength {
            actual: FIXED_BODY_BYTES + reader.remaining(),
            expected: FIXED_BODY_BYTES,
        });
    }
    let flags = fixed[0];
    let float =
        |at: usize| f32::from_le_bytes([fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]]);
    let position = [float(1), float(5), float(9)];
    for (field, value) in [
        ("position.x", position[0]),
        ("position.y", position[1]),
        ("position.z", position[2]),
    ] {
        validate_finite(field, value)?;
    }
    let pitch = byte_rotation_degrees(fixed[13]);
    let yaw = byte_rotation_degrees(fixed[14]);
    let head_yaw = byte_rotation_degrees(fixed[15]);

    Ok(ActorEvent::Move(ActorMoveEvent {
        dimension,
        runtime_id,
        position: position.map(Some),
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: Some(pitch),
        yaw: Some(yaw),
        head_yaw: Some(head_yaw),
        on_ground: Some(flags & MOVE_ON_GROUND != 0),
        teleported: flags & MOVE_TELEPORT != 0,
        force_local_move: flags & MOVE_FORCE_LOCAL != 0,
        player_mode: None,
        source_tick: None,
        interpolation: ActorInterpolation {
            force_completion: flags & MOVE_FORCE_COMPLETION != 0,
            ..Default::default()
        },
    }))
}

pub(crate) fn normalize_move_entity_delta(
    packet: MoveActorDeltaPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let move_data = packet.move_data;
    for (field, value) in [
        ("position.x", move_data.new_position_x),
        ("position.y", move_data.new_position_y),
        ("position.z", move_data.new_position_z),
    ] {
        if let Some(value) = value {
            validate_finite(field, value.0)?;
        }
    }
    Ok(ActorEvent::Move(ActorMoveEvent {
        dimension,
        runtime_id: move_data.actor_runtime_id.0,
        position: [
            move_data.new_position_x.map(|value| value.0),
            move_data.new_position_y.map(|value| value.0),
            move_data.new_position_z.map(|value| value.0),
        ],
        // Partial movement retains the same coordinate origin as absolute movement.
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: move_data
            .rotation_x
            .map(|value| signed_byte_rotation_degrees(value.0)),
        yaw: move_data
            .rotation_y
            .map(|value| signed_byte_rotation_degrees(value.0)),
        head_yaw: move_data
            .rotation_y_head
            .map(|value| signed_byte_rotation_degrees(value.0)),
        on_ground: Some(move_data.is_on_ground),
        // 1.26.40 replaced the packed u16 flag word with explicit booleans and
        // dropped the dedicated teleport bit. `ForceMove` carries the same
        // meaning the teleport bit had for a consumer: snap, do not interpolate.
        // gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
        // `minecraft/protocol/packet/move_actor_delta.go`: "ForceMove specifies
        // whether the client should snap the entity to its new position without
        // interpolation."
        teleported: move_data.force_move,
        force_local_move: move_data.force_move_local_entity,
        player_mode: None,
        source_tick: None,
        interpolation: ActorInterpolation {
            ticks: move_data.ticks.0,
            force_completion: move_data.force_completion,
        },
    }))
}

pub(crate) fn normalize_set_entity_data(
    packet: SetActorDataPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let tick = normalize_tick(packet.tick.0);
    Ok(ActorEvent::Metadata(ActorMetadataUpdateEvent {
        dimension,
        runtime_id: packet.target_runtime_id.0,
        metadata: normalize_metadata(packet.actor_data)?,
        properties: normalize_properties(packet.synched_properties)?,
        tick,
    }))
}

pub(crate) fn normalize_update_attributes(
    packet: UpdateAttributesPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let tick = normalize_tick(packet.tick.0);
    Ok(ActorEvent::Attributes(ActorAttributesUpdateEvent {
        dimension,
        runtime_id: packet.target_runtime_id.0,
        attributes: normalize_attribute_data(packet.attribute_list)?,
        tick,
    }))
}

pub(crate) fn normalize_mob_effect(
    packet: MobEffectPacket,
    dimension: i32,
) -> Result<ActorEffectEvent, ActorPacketError> {
    let tick = normalize_tick(packet.tick.0);
    Ok(ActorEffectEvent {
        dimension,
        actor_runtime_id: packet.target_runtime_id.0,
        action: match packet.event_id {
            MobEffectPacketEventId::Add => ActorEffectAction::Add,
            MobEffectPacketEventId::Update => ActorEffectAction::Update,
            MobEffectPacketEventId::Remove => ActorEffectAction::Remove,
            // `Invalid` is the generator's name for operation 0, which is not a
            // lifecycle verb the client can act on. It is retained verbatim as an
            // unknown verb rather than guessed at, exactly like any other id.
            MobEffectPacketEventId::Invalid => ActorEffectAction::Unknown(0),
            MobEffectPacketEventId::Unknown(value) => ActorEffectAction::Unknown(value),
        },
        effect_id: packet.effect_id.0,
        amplifier: packet.effect_amplifier.0,
        particles: packet.show_particles,
        ambient: packet.ambient,
        duration_ticks: packet.effect_duration_ticks.0,
        tick,
    })
}

pub(crate) fn normalize_set_entity_link(
    packet: SetActorLinkPacket,
    dimension: i32,
) -> ActorLinkEvent {
    normalize_actor_link(packet.link, dimension)
}

fn normalize_actor_links(
    links: Vec<VendorActorLink>,
    dimension: i32,
) -> Result<Arc<[ActorLinkEvent]>, ActorPacketError> {
    check_count("actor_links", links.len(), MAX_ACTOR_LINKS_PER_SPAWN)?;
    Ok(links
        .into_iter()
        .map(|link| normalize_actor_link(link, dimension))
        .collect())
}

fn normalize_actor_link(link: VendorActorLink, dimension: i32) -> ActorLinkEvent {
    ActorLinkEvent {
        dimension,
        // `target_a` is the ridden actor and `target_b` the rider. gophertunnel
        // be6713da4dc051a4197f897d04835e89e9c54321
        // `minecraft/protocol/entity_link.go`: `ActorUniqueID(&x.RiddenEntityUniqueID)`
        // then `ActorUniqueID(&x.RiderEntityUniqueID)`.
        ridden_unique_id: link.target_a.0,
        rider_unique_id: link.target_b.0,
        link_type: match link.type_ {
            VendorActorLinkType::None => ActorLinkType::Remove,
            VendorActorLinkType::Riding => ActorLinkType::Rider,
            VendorActorLinkType::Passenger => ActorLinkType::Passenger,
            VendorActorLinkType::Unknown(value) => ActorLinkType::Unknown(value),
        },
        immediate: link.immediate,
        rider_initiated: link.passenger_initiated,
    }
}

pub(crate) fn normalize_player_list(
    packet: PlayerListPacket,
) -> Result<ActorEvent, ActorPacketError> {
    // 1.26.40 sends one self-describing record per entry: no shared record
    // count, no packet-level action, and no trailing parallel array. Protocol
    // 1001's hand-patched count/action/verified cross-checks are therefore gone
    // -- the shapes they guarded cannot be constructed any more. gophertunnel
    // be6713da4dc051a4197f897d04835e89e9c54321
    // `minecraft/protocol/packet/player_list.go`: `Slice(io, &pk.Entries)`.
    let count = packet.entries.len();
    check_count("player_list", count, MAX_PLAYER_LIST_RECORDS)?;
    let mut entries = Vec::with_capacity(count);
    let mut retained_skin_bytes = 0usize;
    for entry in packet.entries {
        match entry {
            PlayerListData::AddEntry {
                uuid,
                actor_unique_id,
                player_name,
                xbl_xuid,
                serialized_skin,
                ..
            } => {
                validate_text("player_list.username", &player_name, MAX_ACTOR_NAME_BYTES)?;
                // The per-entry "verified" bit used to ride in a trailing bool
                // array; it is now the skin's own trusted flag, serialised as a
                // string. gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
                // `minecraft/protocol/skin.go`: the flag is written as "true" /
                // "false" and read back with `strings.EqualFold(trusted, "true")`.
                let verified = serialized_skin
                    .trusted_skin_flag
                    .as_bytes()
                    .eq_ignore_ascii_case(b"true");
                let skin = normalize_player_skin(*serialized_skin, &mut retained_skin_bytes);
                entries.push(PlayerListEntry::Add {
                    uuid: *uuid.as_bytes(),
                    unique_id: actor_unique_id.0,
                    username: Arc::from(player_name.to_string_lossy()),
                    verified,
                    xuid: player_list_xuid(&xbl_xuid),
                    skin,
                });
            }
            PlayerListData::RemoveEntry { uuid, .. } => {
                entries.push(PlayerListEntry::Remove {
                    uuid: *uuid.as_bytes(),
                });
            }
        }
    }
    tracing::debug!(
        entries = entries.len(),
        identities = entries
            .iter()
            .filter(|entry| matches!(entry, PlayerListEntry::Add { xuid, .. } if !xuid.is_empty()))
            .count(),
        "Cinnabar player-list identities decoded"
    );
    Ok(ActorEvent::PlayerList(PlayerListUpdateEvent {
        entries: Arc::from(entries),
    }))
}

/// Most decimal digits an Xbox user ID carries.
const MAX_XUID_DIGITS: usize = 20;

/// Keeps a well-formed decimal XUID and drops anything else without rejecting the entry.
fn player_list_xuid(xuid: &wire::Str) -> Arc<str> {
    let bytes = xuid.as_bytes();
    if !bytes.is_empty() && bytes.len() <= MAX_XUID_DIGITS && bytes.iter().all(u8::is_ascii_digit) {
        Arc::from(xuid.to_string_lossy())
    } else {
        Arc::from("")
    }
}

/// Normalizes the four-field spawn attribute list AddActor carries.
///
/// This shape has no defaults and no modifiers; only UpdateAttributes carries
/// the full [`AttributeData`].
fn normalize_synced_attributes(
    attributes: Vec<SyncedAttribute>,
) -> Result<Arc<[ActorAttribute]>, ActorPacketError> {
    check_count("attributes", attributes.len(), MAX_ACTOR_ATTRIBUTES)?;
    // Skip individual malformed attributes (over-long name, non-finite bound —
    // servers send INFINITY for "unbounded") rather than dropping the actor.
    let mut normalized = Vec::with_capacity(attributes.len());
    normalized.extend(attributes.into_iter().filter_map(|attribute| {
        if attribute.attribute_name.len() > MAX_ACTOR_NAME_BYTES
            || [
                attribute.min_value.0,
                attribute.max_value.0,
                attribute.current_value.0,
            ]
            .iter()
            .any(|value| !value.is_finite())
        {
            return None;
        }
        Some(ActorAttribute {
            name: Arc::from(attribute.attribute_name.to_string_lossy()),
            min: attribute.min_value.0,
            max: attribute.max_value.0,
            current: attribute.current_value.0,
            default: None,
            modifiers: Arc::from([]),
        })
    }));
    Ok(Arc::from(normalized))
}

fn normalize_attribute_data(
    attributes: Vec<AttributeData>,
) -> Result<Arc<[ActorAttribute]>, ActorPacketError> {
    check_count("attributes", attributes.len(), MAX_ACTOR_ATTRIBUTES)?;
    let mut normalized = Vec::with_capacity(attributes.len());
    normalized.extend(attributes.into_iter().filter_map(|attribute| {
        if attribute.name.len() > MAX_ACTOR_NAME_BYTES
            || attribute.modifiers.len() > MAX_ACTOR_ATTRIBUTE_MODIFIERS
            || [
                attribute.min_value.0,
                attribute.max_value.0,
                attribute.current_value.0,
                attribute.default_min_value.0,
                attribute.default_max_value.0,
                attribute.default_value.0,
            ]
            .iter()
            .any(|value| !value.is_finite())
        {
            return None;
        }
        let mut modifiers = Vec::with_capacity(attribute.modifiers.len());
        modifiers.extend(attribute.modifiers.into_iter().filter_map(|modifier| {
            if modifier.id.len() > MAX_ACTOR_NAME_BYTES
                || modifier.name.len() > MAX_ACTOR_NAME_BYTES
                || !modifier.amount.0.is_finite()
            {
                return None;
            }
            Some(ActorAttributeModifier {
                id: Arc::from(modifier.id.to_string_lossy()),
                name: Arc::from(modifier.name.to_string_lossy()),
                amount: modifier.amount.0,
                operation: modifier.operation.0,
                operand: modifier.operand.0,
                serializable: modifier.is_serializable,
            })
        }));
        Some(ActorAttribute {
            name: Arc::from(attribute.name.to_string_lossy()),
            min: attribute.min_value.0,
            max: attribute.max_value.0,
            current: attribute.current_value.0,
            default: Some(attribute.default_value.0),
            modifiers: Arc::from(modifiers),
        })
    }));
    Ok(Arc::from(normalized))
}

fn normalize_properties(
    properties: PropertySyncData,
) -> Result<Arc<[ActorProperty]>, ActorPacketError> {
    let count = properties
        .int_entries_list
        .len()
        .saturating_add(properties.float_entries_list.len());
    check_count("properties", count, MAX_ACTOR_PROPERTIES)?;
    let mut normalized = Vec::with_capacity(count);
    normalized.extend(
        properties
            .int_entries_list
            .into_iter()
            .map(|property| ActorProperty::Int {
                index: property.property_index.0,
                value: property.data.0,
            }),
    );
    for property in properties.float_entries_list {
        // Skip a non-finite custom property value rather than dropping the actor.
        if !property.data.0.is_finite() {
            continue;
        }
        normalized.push(ActorProperty::Float {
            index: property.property_index.0,
            value: property.data.0,
        });
    }
    Ok(Arc::from(normalized))
}

fn normalize_metadata(
    metadata: SynchedActorDataCopyableDataList,
) -> Result<Arc<[ActorMetadata]>, ActorPacketError> {
    #[cfg(feature = "tracy")]
    let _zone = tracing::info_span!("actor.metadata.normalize").entered();
    check_count("metadata", metadata.data.len(), MAX_ACTOR_METADATA_ENTRIES)?;
    // Skip individual entries the client cannot model (non-finite floats,
    // oversized payloads) rather than dropping the whole actor. The client
    // renders the entity from the entries it does know.
    //
    // 1.26.40 keys every entry with a raw actor-data id and tags the payload
    // with its own value type, so there is no named key enum to normalize and no
    // key-specific value variants: the payload type alone decides the mapping.
    let mut entries = Vec::with_capacity(metadata.data.len());
    entries.extend(metadata.data.into_iter().filter_map(|entry| {
        let key = entry.id.0;
        let value = match entry.payload {
            DataItemEntryValue::DataItemByte { value, .. } => ActorMetadataValue::Byte(value.0),
            DataItemEntryValue::DataItemShort { value, .. } => ActorMetadataValue::Short(value.0),
            DataItemEntryValue::DataItemInt { value, .. } => ActorMetadataValue::Int(value.0),
            DataItemEntryValue::DataItemFloat { value, .. } => value
                .0
                .is_finite()
                .then_some(ActorMetadataValue::Float(value.0))?,
            DataItemEntryValue::DataItemString { value, .. } => {
                if value.len() > MAX_ACTOR_METADATA_STRING_BYTES {
                    return None;
                }
                ActorMetadataValue::String(Arc::from(value.to_string_lossy()))
            }
            DataItemEntryValue::DataItemCompoundTag { value, .. } => {
                if value.0.len() > MAX_ACTOR_METADATA_NBT_BYTES {
                    return None;
                }
                ActorMetadataValue::Compound(Arc::from(&value.0[..]))
            }
            DataItemEntryValue::DataItemPos { value, .. } => {
                ActorMetadataValue::BlockPosition([value.x.0, value.y.0, value.z.0])
            }
            // The actor flag words are ordinary Int64 payloads on the wire;
            // only their id distinguishes them from a plain long.
            DataItemEntryValue::DataItemInt64 { value, .. } => match key {
                ACTOR_DATA_ID_FLAGS => ActorMetadataValue::Flags(value.0 as u64),
                ACTOR_DATA_ID_FLAGS_EXTENDED | ACTOR_DATA_ID_FLAGS_THIRD => {
                    ActorMetadataValue::FlagsExtended(value.0 as u64)
                }
                _ => ActorMetadataValue::Long(value.0),
            },
            DataItemEntryValue::DataItemVec3 { value, .. } => {
                let value = value.to_array();
                if value.iter().any(|c| !c.is_finite()) {
                    return None;
                }
                ActorMetadataValue::Vector(value)
            }
        };
        Some(ActorMetadata { key, value })
    }));
    Ok(Arc::from(entries))
}

fn normalize_tick(tick: u64) -> u64 {
    tick
}

fn byte_rotation_degrees(value: u8) -> f32 {
    signed_byte_rotation_degrees(value as i8)
}

/// Converts a signed rotation byte to degrees in the half-turn range.
fn signed_byte_rotation_degrees(value: i8) -> f32 {
    f32::from(value) * (360.0 / 256.0)
}

fn check_count(collection: &'static str, count: usize, max: usize) -> Result<(), ActorPacketError> {
    if count > max {
        return Err(ActorPacketError::TooManyEntries {
            collection,
            count,
            max,
        });
    }
    Ok(())
}

fn validate_text(
    field: &'static str,
    value: &wire::Str,
    max: usize,
) -> Result<(), ActorPacketError> {
    if value.len() > max {
        return Err(ActorPacketError::TextTooLong {
            field,
            bytes: value.len(),
            max,
        });
    }
    Ok(())
}

fn validate_finite(field: &'static str, value: f32) -> Result<(), ActorPacketError> {
    if !value.is_finite() {
        return Err(ActorPacketError::NonFiniteField { field });
    }
    Ok(())
}
