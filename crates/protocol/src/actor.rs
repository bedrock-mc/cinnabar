use std::sync::Arc;

use bytes::{Buf, Bytes};
use thiserror::Error;
use valentine::{
    bedrock::version::v1_26_40::{
        ActorLink as VendorActorLink, AddActorPacket, AddPlayerPacket, AttributeData,
        DataItemEntryPayload, EnumsActorLinkType as VendorActorLinkType,
        EnumsMobEffectPacketPayloadEvent as MobEffectPacketEventId, MobEffectPacket,
        MoveActorAbsolutePacket, MoveActorDeltaPacket, PlayerListPacket,
        PlayerListPacketEntriesItem, PropertySyncData, RemoveActorPacket, SerializedSkinRef,
        SetActorDataPacket, SetActorLinkPacket, SyncedAttribute, SynchedActorDataCopyableDataList,
        UpdateAttributesPacket,
    },
    protocol::wire,
};

use crate::{ItemPacketError, NetworkItemStack, item::normalize_item};

pub const MAX_ACTOR_IDENTIFIER_BYTES: usize = 256;
pub const MAX_ACTOR_NAME_BYTES: usize = 256;
pub const MAX_ACTOR_METADATA_ENTRIES: usize = 256;
pub const MAX_ACTOR_ATTRIBUTES: usize = 128;
pub const MAX_ACTOR_PROPERTIES: usize = 256;
/// Local normalization ceiling for the links retained from one spawn packet.
pub const MAX_ACTOR_LINKS_PER_SPAWN: usize = 256;
pub const MAX_ACTOR_ATTRIBUTE_MODIFIERS: usize = 64;
pub const MAX_ACTOR_METADATA_STRING_BYTES: usize = 4_096;
pub const MAX_ACTOR_METADATA_NBT_BYTES: usize = 1_048_576;
pub const MAX_PLAYER_LIST_RECORDS: usize = 4_096;
pub const MAX_STANDARD_SKIN_SIDE: u32 = 256;
pub const MAX_PLAYER_LIST_SKIN_BYTES: usize = 64 * 1024 * 1024;

/// Actor-data id of the primary 64-bit actor flag word.
///
/// The 1.26.40 generator emits raw actor-data ids instead of the named key enum
/// protocol 1001 carried, so the two flag words are recognised by id here and
/// re-typed into the `Flags`/`FlagsExtended` values downstream already reads.
/// gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
/// `minecraft/protocol/entity_metadata.go`: `EntityDataKeyFlags = iota`.
const ACTOR_DATA_ID_FLAGS: u32 = 0;

/// Actor-data id of the overflow 64-bit actor flag word.
///
/// gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
/// `minecraft/protocol/entity_metadata.go`: `EntityDataKeyFlagsTwo` (92).
const ACTOR_DATA_ID_FLAGS_EXTENDED: u32 = 92;

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
    pub player_mode: Option<crate::MovePlayerMode>,
    pub source_tick: Option<u64>,
}

/// Coordinate space carried by an actor movement position.
///
/// Spawn positions and partial actor movement values use the actor store's
/// retained coordinate space. Absolute actor and player movement packets use a
/// network coordinate whose player offset can be removed once actor kind is known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActorPositionOrigin {
    /// The position is already in the actor store's retained coordinate space.
    #[default]
    Feet,
    /// The position came from an absolute Bedrock network movement packet.
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
        skin: PlayerSkin,
    },
    Remove {
        uuid: [u8; 16],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardSkin {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSkinUnavailable {
    UnsupportedPersona,
    InvalidDimensions,
    InvalidByteLength,
    RetainedBudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerSkin {
    Standard(StandardSkin),
    Unavailable(PlayerSkinUnavailable),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerListUpdateEvent {
    pub entries: Arc<[PlayerListEntry]>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActorEvent {
    Spawn(ActorSpawnEvent),
    Remove(ActorRemoveEvent),
    Move(ActorMoveEvent),
    Metadata(ActorMetadataUpdateEvent),
    Attributes(ActorAttributesUpdateEvent),
    PlayerList(PlayerListUpdateEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActorPacketError {
    #[error(transparent)]
    Item(#[from] ItemPacketError),

    #[error("actor identifier has {bytes} UTF-8 bytes, exceeding {max}")]
    IdentifierTooLong { bytes: usize, max: usize },
    #[error("actor spawn contains a non-finite {field}")]
    NonFiniteSpawnField { field: &'static str },
    #[error("actor collection {collection} has {count} entries, exceeding {max}")]
    TooManyEntries {
        collection: &'static str,
        count: usize,
        max: usize,
    },
    #[error("actor text field {field} has {bytes} UTF-8 bytes, exceeding {max}")]
    TextTooLong {
        field: &'static str,
        bytes: usize,
        max: usize,
    },
    #[error("actor field {field} is non-finite")]
    NonFiniteField { field: &'static str },
    #[error("absolute actor move has an invalid runtime ID varuint")]
    InvalidAbsoluteMoveRuntimeId,
    #[error(
        "absolute actor move has {actual} body bytes after its runtime ID; expected {expected}"
    )]
    InvalidAbsoluteMoveLength { actual: usize, expected: usize },
    #[error("actor update has negative tick {0}")]
    NegativeTick(i64),
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
        ("head_yaw", packet.y_head_rotation),
        ("body_yaw", packet.y_body_rotation),
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
        unique_id: packet.target_actor_id.actor_unique_id,
        runtime_id: packet.target_runtime_id.actor_runtime_id,
        kind: ActorKind::Entity {
            identifier: Arc::from(packet.actor_type),
        },
        position: [packet.position.x, packet.position.y, packet.position.z],
        velocity: [packet.velocity.x, packet.velocity.y, packet.velocity.z],
        pitch,
        yaw,
        head_yaw: packet.y_head_rotation,
        body_yaw: packet.y_body_rotation,
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
        ("head_yaw", packet.y_head_rotation),
    ] {
        validate_finite(field, value)?;
    }
    let metadata = normalize_metadata(packet.entity_data)?;
    let properties = normalize_properties(packet.synched_properties)?;
    let held_item = normalize_item(packet.carried_item)?;
    let links = normalize_actor_links(packet.actor_links, dimension)?;
    Ok(ActorEvent::Spawn(ActorSpawnEvent {
        dimension,
        // AddPlayer carries no standalone unique ID; the spawned player's unique
        // ID is the first field of the embedded ability data. Protocol 1001's
        // prismarine schema flattened that block, which is why the old code read
        // a top-level `unique_id`. gophertunnel
        // be6713da4dc051a4197f897d04835e89e9c54321
        // `minecraft/protocol/ability.go`: `AbilityData.EntityUniqueID`.
        unique_id: packet.abilities_data.target_player_raw_id,
        runtime_id: packet.target_runtime_id.actor_runtime_id,
        kind: ActorKind::Player {
            uuid: *packet.uuid.as_bytes(),
            username: Arc::from(packet.player_name),
        },
        position: [packet.position.x, packet.position.y, packet.position.z],
        velocity: [packet.velocity.x, packet.velocity.y, packet.velocity.z],
        pitch,
        yaw,
        head_yaw: packet.y_head_rotation,
        body_yaw: yaw,
        held_item,
        metadata,
        attributes: Arc::from([]),
        properties,
        links,
    }))
}

pub(crate) const fn normalize_remove_entity(
    packet: RemoveActorPacket,
    dimension: i32,
) -> ActorEvent {
    ActorEvent::Remove(ActorRemoveEvent {
        dimension,
        unique_id: packet.target_actor_id.actor_unique_id,
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
        runtime_id: move_data.actor_runtime_id.actor_runtime_id,
        position: [
            Some(move_data.position.x),
            Some(move_data.position.y),
            Some(move_data.position.z),
        ],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: Some(byte_rotation_degrees(move_data.rotation_x)),
        yaw: Some(byte_rotation_degrees(move_data.rotation_y)),
        head_yaw: Some(byte_rotation_degrees(move_data.rotation_y_head)),
        // `header` is the movement flag byte. gophertunnel
        // be6713da4dc051a4197f897d04835e89e9c54321
        // `minecraft/protocol/packet/move_actor_absolute.go`:
        // `MoveFlagOnGround = 1 << iota`, `MoveFlagTeleport`.
        on_ground: Some(move_data.header & 1 != 0),
        teleported: move_data.header & 2 != 0,
        player_mode: None,
        source_tick: None,
    }))
}

/// Decodes the Bedrock MoveActorAbsolute body straight off the wire.
///
/// Protocol 1001 needed this because Valentine modelled each byte rotation as a
/// length-prefixed byte vector and the runtime ID as a signed VarLong. The
/// 1.26.40 `MoveActorAbsoluteData` is wire-correct (VarUInt64 runtime ID, then
/// the flag byte, the position and three raw rotation bytes), so this path is no
/// longer required for correctness -- it is retained only as the allocation-free
/// fast path the raw play loop uses, and must stay byte-for-byte identical to
/// [`normalize_move_entity`].
pub(crate) fn normalize_move_entity_body(
    body: &Bytes,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    const FIXED_BODY_BYTES: usize = 1 + 3 * size_of::<f32>() + 3;

    let mut body = body.as_ref();
    let runtime_id = wire::read_var_u64(&mut body)
        .map_err(|_| ActorPacketError::InvalidAbsoluteMoveRuntimeId)?;
    if body.remaining() != FIXED_BODY_BYTES {
        return Err(ActorPacketError::InvalidAbsoluteMoveLength {
            actual: body.remaining(),
            expected: FIXED_BODY_BYTES,
        });
    }
    let flags = body.get_u8();
    let position = [body.get_f32_le(), body.get_f32_le(), body.get_f32_le()];
    for (field, value) in [
        ("position.x", position[0]),
        ("position.y", position[1]),
        ("position.z", position[2]),
    ] {
        validate_finite(field, value)?;
    }
    let pitch = byte_rotation_degrees(body.get_u8());
    let yaw = byte_rotation_degrees(body.get_u8());
    let head_yaw = byte_rotation_degrees(body.get_u8());

    Ok(ActorEvent::Move(ActorMoveEvent {
        dimension,
        runtime_id,
        position: position.map(Some),
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: Some(pitch),
        yaw: Some(yaw),
        head_yaw: Some(head_yaw),
        on_ground: Some(flags & 1 != 0),
        teleported: flags & 2 != 0,
        player_mode: None,
        source_tick: None,
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
            validate_finite(field, value)?;
        }
    }
    Ok(ActorEvent::Move(ActorMoveEvent {
        dimension,
        runtime_id: move_data.actor_runtime_id.actor_runtime_id,
        position: [
            move_data.new_position_x,
            move_data.new_position_y,
            move_data.new_position_z,
        ],
        position_origin: ActorPositionOrigin::Feet,
        pitch: move_data.rotation_x.map(signed_byte_rotation_degrees),
        yaw: move_data.rotation_y.map(signed_byte_rotation_degrees),
        head_yaw: move_data.rotation_y_head.map(signed_byte_rotation_degrees),
        on_ground: Some(move_data.is_on_ground),
        // 1.26.40 replaced the packed u16 flag word with explicit booleans and
        // dropped the dedicated teleport bit. `ForceMove` carries the same
        // meaning the teleport bit had for a consumer: snap, do not interpolate.
        // gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
        // `minecraft/protocol/packet/move_actor_delta.go`: "ForceMove specifies
        // whether the client should snap the entity to its new position without
        // interpolation."
        teleported: move_data.force_move,
        player_mode: None,
        source_tick: None,
    }))
}

pub(crate) fn normalize_set_entity_data(
    packet: SetActorDataPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let tick = normalize_tick(packet.tick.inputtick);
    Ok(ActorEvent::Metadata(ActorMetadataUpdateEvent {
        dimension,
        runtime_id: packet.target_runtime_id.actor_runtime_id,
        metadata: normalize_metadata(packet.actor_data)?,
        properties: normalize_properties(packet.synched_properties)?,
        tick,
    }))
}

pub(crate) fn normalize_update_attributes(
    packet: UpdateAttributesPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let tick = normalize_tick(packet.tick.inputtick);
    Ok(ActorEvent::Attributes(ActorAttributesUpdateEvent {
        dimension,
        runtime_id: packet.target_runtime_id.actor_runtime_id,
        attributes: normalize_attribute_data(packet.attribute_list)?,
        tick,
    }))
}

pub(crate) fn normalize_mob_effect(
    packet: MobEffectPacket,
    dimension: i32,
) -> Result<ActorEffectEvent, ActorPacketError> {
    let tick = normalize_tick(packet.tick.inputtick);
    Ok(ActorEffectEvent {
        dimension,
        actor_runtime_id: packet.target_runtime_id.actor_runtime_id,
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
        effect_id: packet.effect_id,
        amplifier: packet.effect_amplifier,
        particles: packet.show_particles,
        ambient: packet.ambient,
        duration_ticks: packet.effect_duration_ticks,
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
        ridden_unique_id: link.target_a.actor_unique_id,
        rider_unique_id: link.target_b.actor_unique_id,
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
            PlayerListPacketEntriesItem::AddEntry(record) => {
                validate_text(
                    "player_list.username",
                    &record.player_name,
                    MAX_ACTOR_NAME_BYTES,
                )?;
                // The per-entry "verified" bit used to ride in a trailing bool
                // array; it is now the skin's own trusted flag, serialised as a
                // string. gophertunnel be6713da4dc051a4197f897d04835e89e9c54321
                // `minecraft/protocol/skin.go`: the flag is written as "true" /
                // "false" and read back with `strings.EqualFold(trusted, "true")`.
                let verified = record
                    .serialized_skin
                    .trusted_skin_flag
                    .eq_ignore_ascii_case("true");
                let skin = normalize_player_skin(record.serialized_skin, &mut retained_skin_bytes);
                entries.push(PlayerListEntry::Add {
                    uuid: *record.uuid.as_bytes(),
                    unique_id: record.actor_unique_id.actor_unique_id,
                    username: Arc::from(record.player_name),
                    verified,
                    skin,
                });
            }
            PlayerListPacketEntriesItem::RemoveEntry(record) => {
                entries.push(PlayerListEntry::Remove {
                    uuid: *record.uuid.as_bytes(),
                });
            }
        }
    }
    Ok(ActorEvent::PlayerList(PlayerListUpdateEvent {
        entries: Arc::from(entries),
    }))
}

fn normalize_player_skin(skin: SerializedSkinRef, retained_bytes: &mut usize) -> PlayerSkin {
    if skin.is_persona {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedPersona);
    }
    let (width, height) = (skin.image_data.width, skin.image_data.height);
    if width != height || !matches!(width, 64 | 128 | MAX_STANDARD_SKIN_SIDE) {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    }
    let Some(expected_bytes) = usize::try_from(width)
        .ok()
        .and_then(|width| usize::try_from(height).ok().map(|height| (width, height)))
        .and_then(|(width, height)| width.checked_mul(height))
        .and_then(|pixels| pixels.checked_mul(4))
    else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
    if skin.image_data.image_bytes.len() != expected_bytes {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidByteLength);
    }
    let Some(next_bytes) = retained_bytes.checked_add(expected_bytes) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    };
    if next_bytes > MAX_PLAYER_LIST_SKIN_BYTES {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded);
    }
    *retained_bytes = next_bytes;
    PlayerSkin::Standard(StandardSkin {
        width,
        height,
        rgba8: Arc::from(skin.image_data.image_bytes),
    })
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
    let normalized = attributes
        .into_iter()
        .filter_map(|attribute| {
            if attribute.attribute_name.len() > MAX_ACTOR_NAME_BYTES
                || [
                    attribute.min_value,
                    attribute.max_value,
                    attribute.current_value,
                ]
                .iter()
                .any(|value| !value.is_finite())
            {
                return None;
            }
            Some(ActorAttribute {
                name: Arc::from(attribute.attribute_name),
                min: attribute.min_value,
                max: attribute.max_value,
                current: attribute.current_value,
                default: None,
                modifiers: Arc::from([]),
            })
        })
        .collect::<Vec<_>>();
    Ok(Arc::from(normalized))
}

fn normalize_attribute_data(
    attributes: Vec<AttributeData>,
) -> Result<Arc<[ActorAttribute]>, ActorPacketError> {
    check_count("attributes", attributes.len(), MAX_ACTOR_ATTRIBUTES)?;
    let normalized = attributes
        .into_iter()
        .filter_map(|attribute| {
            if attribute.name.len() > MAX_ACTOR_NAME_BYTES
                || attribute.modifiers.len() > MAX_ACTOR_ATTRIBUTE_MODIFIERS
                || [
                    attribute.min_value,
                    attribute.max_value,
                    attribute.current_value,
                    attribute.default_min_value,
                    attribute.default_max_value,
                    attribute.default_value,
                ]
                .iter()
                .any(|value| !value.is_finite())
            {
                return None;
            }
            let modifiers = attribute
                .modifiers
                .into_iter()
                .filter_map(|modifier| {
                    if modifier.id.len() > MAX_ACTOR_NAME_BYTES
                        || modifier.name.len() > MAX_ACTOR_NAME_BYTES
                        || !modifier.amount.is_finite()
                    {
                        return None;
                    }
                    Some(ActorAttributeModifier {
                        id: Arc::from(modifier.id),
                        name: Arc::from(modifier.name),
                        amount: modifier.amount,
                        operation: modifier.operation,
                        operand: modifier.operand,
                        serializable: modifier.is_serializable,
                    })
                })
                .collect::<Vec<_>>();
            Some(ActorAttribute {
                name: Arc::from(attribute.name),
                min: attribute.min_value,
                max: attribute.max_value,
                current: attribute.current_value,
                default: Some(attribute.default_value),
                modifiers: Arc::from(modifiers),
            })
        })
        .collect::<Vec<_>>();
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
                index: property.property_index,
                value: property.data,
            }),
    );
    for property in properties.float_entries_list {
        // Skip a non-finite custom property value rather than dropping the actor.
        if !property.data.is_finite() {
            continue;
        }
        normalized.push(ActorProperty::Float {
            index: property.property_index,
            value: property.data,
        });
    }
    Ok(Arc::from(normalized))
}

fn normalize_metadata(
    metadata: SynchedActorDataCopyableDataList,
) -> Result<Arc<[ActorMetadata]>, ActorPacketError> {
    check_count("metadata", metadata.data.len(), MAX_ACTOR_METADATA_ENTRIES)?;
    // Skip individual entries the client cannot model (non-finite floats,
    // oversized payloads) rather than dropping the whole actor. The client
    // renders the entity from the entries it does know.
    //
    // 1.26.40 keys every entry with a raw actor-data id and tags the payload
    // with its own value type, so there is no named key enum to normalize and no
    // key-specific value variants: the payload type alone decides the mapping.
    let entries = metadata
        .data
        .into_iter()
        .filter_map(|entry| {
            let key = entry.id;
            let value = match entry.payload {
                DataItemEntryPayload::DataItemBytePayload(payload) => {
                    ActorMetadataValue::Byte(payload.value)
                }
                DataItemEntryPayload::DataItemShortPayload(payload) => {
                    ActorMetadataValue::Short(payload.value)
                }
                DataItemEntryPayload::DataItemIntPayload(payload) => {
                    ActorMetadataValue::Int(payload.value)
                }
                DataItemEntryPayload::DataItemFloatPayload(payload) => payload
                    .value
                    .is_finite()
                    .then_some(ActorMetadataValue::Float(payload.value))?,
                DataItemEntryPayload::DataItemStringPayload(payload) => {
                    if payload.value.len() > MAX_ACTOR_METADATA_STRING_BYTES {
                        return None;
                    }
                    ActorMetadataValue::String(Arc::from(payload.value))
                }
                DataItemEntryPayload::DataItemCompoundTagPayload(payload) => {
                    if payload.value.0.len() > MAX_ACTOR_METADATA_NBT_BYTES {
                        return None;
                    }
                    ActorMetadataValue::Compound(Arc::from(payload.value.0.to_vec()))
                }
                DataItemEntryPayload::DataItemPosPayload(payload) => {
                    ActorMetadataValue::BlockPosition([
                        payload.value.x,
                        payload.value.y,
                        payload.value.z,
                    ])
                }
                // The two actor flag words are ordinary Int64 payloads on the
                // wire; only their id distinguishes them from a plain long.
                DataItemEntryPayload::DataItemInt64Payload(payload) => match key {
                    ACTOR_DATA_ID_FLAGS => ActorMetadataValue::Flags(payload.value as u64),
                    ACTOR_DATA_ID_FLAGS_EXTENDED => {
                        ActorMetadataValue::FlagsExtended(payload.value as u64)
                    }
                    _ => ActorMetadataValue::Long(payload.value),
                },
                DataItemEntryPayload::DataItemVec3Payload(payload) => {
                    let value = [payload.value.x, payload.value.y, payload.value.z];
                    if value.iter().any(|c| !c.is_finite()) {
                        return None;
                    }
                    ActorMetadataValue::Vector(value)
                }
            };
            Some(ActorMetadata { key, value })
        })
        .collect::<Vec<_>>();
    Ok(Arc::from(entries))
}

fn normalize_tick(tick: u64) -> u64 {
    tick
}

fn byte_rotation_degrees(value: u8) -> f32 {
    f32::from(value) * (360.0 / 256.0)
}

/// Converts a rotation byte the generator types as `i8`.
///
/// Bedrock reads every rotation byte unsigned. gophertunnel
/// be6713da4dc051a4197f897d04835e89e9c54321 `minecraft/protocol/reader.go`:
/// `ByteFloat` reads a `uint8` and scales it by `360.0 / 256.0`. The generated
/// `MoveActorDeltaData` stores the same byte as `i8`, so the bit pattern is
/// reinterpreted rather than sign-extended.
fn signed_byte_rotation_degrees(value: i8) -> f32 {
    byte_rotation_degrees(value as u8)
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

fn validate_text(field: &'static str, value: &str, max: usize) -> Result<(), ActorPacketError> {
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
