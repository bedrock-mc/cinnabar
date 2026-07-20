use std::sync::Arc;

use bytes::{Buf, Bytes, BytesMut};
use sha2::{Digest, Sha256};
use thiserror::Error;
use valentine::{
    bedrock::codec::{BedrockCodec, VarInt},
    bedrock::version::v1_26_30::{
        AddEntityPacket, AddPlayerPacket, DeltaMoveFlags, EntityAttributes, EntityProperties,
        GameMode, MetadataDictionary, MetadataDictionaryItemKey, MetadataDictionaryItemValue,
        MetadataDictionaryItemValueDefault, MoveEntityDeltaPacket, MoveEntityPacket,
        PlayerAttributes, PlayerListPacket, PlayerRecordsRecordsItem, PlayerRecordsType,
        RemoveEntityPacket, SetDefaultGameTypePacket, SetEntityDataPacket, UpdateAttributesPacket,
        UpdatePlayerGameTypePacket,
    },
    protocol::wire,
};

use crate::{ItemPacketError, NetworkItemStack, item::normalize_item};

pub const MAX_ACTOR_IDENTIFIER_BYTES: usize = 256;
pub const MAX_ACTOR_NAME_BYTES: usize = 256;
pub const MAX_ACTOR_METADATA_ENTRIES: usize = 256;
pub const MAX_ACTOR_ATTRIBUTES: usize = 128;
pub const MAX_ACTOR_PROPERTIES: usize = 256;
pub const MAX_ACTOR_ATTRIBUTE_MODIFIERS: usize = 64;
pub const MAX_ACTOR_METADATA_STRING_BYTES: usize = 4_096;
pub const MAX_ACTOR_METADATA_NBT_BYTES: usize = 1_048_576;
pub const MAX_PLAYER_LIST_RECORDS: usize = 4_096;
pub const MAX_STANDARD_SKIN_SIDE: u32 = 256;
pub const MAX_PLAYER_LIST_SKIN_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PLAYER_SKIN_GEOMETRY_BYTES: usize = 1_048_576;
pub const MAX_PLAYER_SKIN_GEOMETRY_DEPTH: usize = 32;
pub const MAX_PLAYER_SKIN_GEOMETRY_NODES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorKind {
    Player { uuid: [u8; 16], username: Arc<str> },
    Entity { identifier: Arc<str> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorGameMode {
    Survival,
    Creative,
    Adventure,
    SurvivalSpectator,
    CreativeSpectator,
    Fallback,
    Spectator,
    Unknown(i32),
}

impl ActorGameMode {
    #[must_use]
    pub const fn is_spectator(self) -> bool {
        matches!(
            self,
            Self::SurvivalSpectator | Self::CreativeSpectator | Self::Spectator
        )
    }

    #[must_use]
    pub const fn resolve_fallback(self, default: Self) -> Self {
        match (self, default) {
            (Self::Fallback, Self::Fallback | Self::Unknown(_)) => Self::Fallback,
            (Self::Fallback, resolved) => resolved,
            (resolved, _) => resolved,
        }
    }
}

impl From<GameMode> for ActorGameMode {
    fn from(value: GameMode) -> Self {
        match value {
            GameMode::Survival => Self::Survival,
            GameMode::Creative => Self::Creative,
            GameMode::Adventure => Self::Adventure,
            GameMode::SurvivalSpectator => Self::SurvivalSpectator,
            GameMode::CreativeSpectator => Self::CreativeSpectator,
            GameMode::Fallback => Self::Fallback,
            GameMode::Spectator => Self::Spectator,
            GameMode::Unknown(value) => Self::Unknown(value),
        }
    }
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
    Int { index: i32, value: i32 },
    Float { index: i32, value: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorMetadata {
    pub key: i32,
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
    pub game_mode: Option<ActorGameMode>,
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
    /// The packet carries Bedrock's teleport authority.
    pub teleported: bool,
    /// The retained pose must update without interpolation.
    pub snap: bool,
    pub player_mode: Option<crate::MovePlayerMode>,
    pub source_tick: Option<i64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorGameModeUpdateEvent {
    pub unique_id: i64,
    pub game_mode: ActorGameMode,
    pub tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefaultActorGameModeEvent {
    pub game_mode: ActorGameMode,
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
    pub geometry: PlayerSkinGeometry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerSkinGeometry {
    Wide,
    Slim,
    Custom {
        identifier: Arc<str>,
        data_sha256: [u8; 32],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSkinUnavailable {
    UnsupportedPersona,
    UnsupportedAppearance,
    InvalidDimensions,
    InvalidByteLength,
    InvalidArmSize,
    InvalidGeometry,
    GeometryTooLarge,
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
    GameMode(ActorGameModeUpdateEvent),
    DefaultGameMode(DefaultActorGameModeEvent),
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
    #[error("actor move rotation {field} has {count} bytes; expected exactly one")]
    InvalidRotationBytes { field: &'static str, count: usize },
    #[error("absolute actor move has an invalid runtime ID varuint")]
    InvalidAbsoluteMoveRuntimeId,
    #[error(
        "absolute actor move has {actual} body bytes after its runtime ID; expected {expected}"
    )]
    InvalidAbsoluteMoveLength { actual: usize, expected: usize },
    #[error("actor update has negative tick {0}")]
    NegativeTick(i64),
    #[error("PlayerList record count {declared} does not match {actual} records")]
    InvalidPlayerListCount { declared: i32, actual: usize },
    #[error("PlayerList action does not match its records")]
    InvalidPlayerListRecords,
    #[error("PlayerList Add verified count does not match its records")]
    InvalidPlayerListVerifiedCount,
    #[error("PlayerList has unsupported action {0}")]
    UnsupportedPlayerListAction(u8),
    #[error("failed to normalize actor metadata key")]
    InvalidMetadataKey,
}

pub(crate) fn normalize_add_entity(
    packet: AddEntityPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    if packet.entity_type.len() > MAX_ACTOR_IDENTIFIER_BYTES {
        return Err(ActorPacketError::IdentifierTooLong {
            bytes: packet.entity_type.len(),
            max: MAX_ACTOR_IDENTIFIER_BYTES,
        });
    }
    for (field, value) in [
        ("position.x", packet.position.x),
        ("position.y", packet.position.y),
        ("position.z", packet.position.z),
        ("velocity.x", packet.velocity.x),
        ("velocity.y", packet.velocity.y),
        ("velocity.z", packet.velocity.z),
        ("pitch", packet.pitch),
        ("yaw", packet.yaw),
        ("head_yaw", packet.head_yaw),
        ("body_yaw", packet.body_yaw),
    ] {
        if !value.is_finite() {
            return Err(ActorPacketError::NonFiniteSpawnField { field });
        }
    }

    let metadata = normalize_metadata(packet.metadata)?;
    let attributes = normalize_entity_attributes(packet.attributes)?;
    let properties = normalize_properties(packet.properties)?;
    Ok(ActorEvent::Spawn(ActorSpawnEvent {
        dimension,
        unique_id: packet.unique_id,
        runtime_id: packet.runtime_id as u64,
        kind: ActorKind::Entity {
            identifier: Arc::from(packet.entity_type),
        },
        game_mode: None,
        position: [packet.position.x, packet.position.y, packet.position.z],
        velocity: [packet.velocity.x, packet.velocity.y, packet.velocity.z],
        pitch: packet.pitch,
        yaw: packet.yaw,
        head_yaw: packet.head_yaw,
        body_yaw: packet.body_yaw,
        held_item: NetworkItemStack::empty(),
        metadata,
        attributes,
        properties,
    }))
}

pub(crate) fn normalize_add_player(
    packet: AddPlayerPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    validate_text("username", &packet.username, MAX_ACTOR_NAME_BYTES)?;
    for (field, value) in [
        ("position.x", packet.position.x),
        ("position.y", packet.position.y),
        ("position.z", packet.position.z),
        ("velocity.x", packet.velocity.x),
        ("velocity.y", packet.velocity.y),
        ("velocity.z", packet.velocity.z),
        ("pitch", packet.pitch),
        ("yaw", packet.yaw),
        ("head_yaw", packet.head_yaw),
    ] {
        validate_finite(field, value)?;
    }
    let metadata = normalize_metadata(packet.metadata)?;
    let properties = normalize_properties(packet.properties)?;
    let held_item = normalize_item(packet.held_item)?;
    Ok(ActorEvent::Spawn(ActorSpawnEvent {
        dimension,
        unique_id: packet.unique_id,
        runtime_id: packet.runtime_id as u64,
        kind: ActorKind::Player {
            uuid: *packet.uuid.as_bytes(),
            username: Arc::from(packet.username),
        },
        game_mode: Some(packet.gamemode.into()),
        position: [packet.position.x, packet.position.y, packet.position.z],
        velocity: [packet.velocity.x, packet.velocity.y, packet.velocity.z],
        pitch: packet.pitch,
        yaw: packet.yaw,
        head_yaw: packet.head_yaw,
        body_yaw: packet.yaw,
        held_item,
        metadata,
        attributes: Arc::from([]),
        properties,
    }))
}

pub(crate) const fn normalize_remove_entity(
    packet: RemoveEntityPacket,
    dimension: i32,
) -> ActorEvent {
    ActorEvent::Remove(ActorRemoveEvent {
        dimension,
        unique_id: packet.entity_id_self,
    })
}

pub(crate) fn normalize_move_entity(
    packet: MoveEntityPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    for (field, value) in [
        ("position.x", packet.position.x),
        ("position.y", packet.position.y),
        ("position.z", packet.position.z),
    ] {
        validate_finite(field, value)?;
    }
    Ok(ActorEvent::Move(ActorMoveEvent {
        dimension,
        runtime_id: packet.runtime_entity_id as u64,
        position: [
            Some(packet.position.x),
            Some(packet.position.y),
            Some(packet.position.z),
        ],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: Some(rotation_degrees("pitch", &packet.rotation.pitch)?),
        yaw: Some(rotation_degrees("yaw", &packet.rotation.yaw)?),
        head_yaw: Some(rotation_degrees("head_yaw", &packet.rotation.head_yaw)?),
        on_ground: Some(packet.flags & 1 != 0),
        teleported: packet.flags & 2 != 0,
        snap: packet.flags & 2 != 0,
        player_mode: None,
        source_tick: None,
    }))
}

/// Decodes the actual Bedrock MoveActorAbsolute wire shape.
///
/// Valentine currently models each byte rotation as a length-prefixed byte vector and models the
/// runtime ID as a signed VarLong. The packet wire format instead carries a VarUInt64 followed by
/// exactly three raw byte rotations, so the raw play path must not materialize the generated type.
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
        snap: flags & 2 != 0,
        player_mode: None,
        source_tick: None,
    }))
}

pub(crate) fn normalize_move_entity_delta(
    packet: MoveEntityDeltaPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    for (field, value) in [
        ("position.x", packet.x),
        ("position.y", packet.y),
        ("position.z", packet.z),
    ] {
        if let Some(value) = value {
            validate_finite(field, value)?;
        }
    }
    Ok(ActorEvent::Move(ActorMoveEvent {
        dimension,
        runtime_id: packet.runtime_entity_id as u64,
        position: [packet.x, packet.y, packet.z],
        position_origin: ActorPositionOrigin::Feet,
        pitch: packet.rot_x.map(byte_rotation_degrees),
        yaw: packet.rot_y.map(byte_rotation_degrees),
        head_yaw: packet.rot_z.map(byte_rotation_degrees),
        on_ground: Some(packet.flags.contains(DeltaMoveFlags::ON_GROUND)),
        teleported: packet.flags.contains(DeltaMoveFlags::TELEPORT),
        snap: packet
            .flags
            .intersects(DeltaMoveFlags::TELEPORT | DeltaMoveFlags::FORCE_MOVE),
        player_mode: None,
        source_tick: None,
    }))
}

pub(crate) fn normalize_update_player_game_type(
    packet: UpdatePlayerGameTypePacket,
) -> Result<ActorEvent, ActorPacketError> {
    let tick =
        u64::try_from(packet.tick).map_err(|_| ActorPacketError::NegativeTick(packet.tick))?;
    Ok(ActorEvent::GameMode(ActorGameModeUpdateEvent {
        unique_id: packet.player_unique_id,
        game_mode: packet.gamemode.into(),
        tick,
    }))
}

pub(crate) fn normalize_set_default_game_type(packet: SetDefaultGameTypePacket) -> ActorEvent {
    ActorEvent::DefaultGameMode(DefaultActorGameModeEvent {
        game_mode: packet.gamemode.into(),
    })
}

pub(crate) fn normalize_set_entity_data(
    packet: SetEntityDataPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let tick =
        u64::try_from(packet.tick).map_err(|_| ActorPacketError::NegativeTick(packet.tick))?;
    Ok(ActorEvent::Metadata(ActorMetadataUpdateEvent {
        dimension,
        runtime_id: packet.runtime_entity_id as u64,
        metadata: normalize_metadata(packet.metadata)?,
        properties: normalize_properties(packet.properties)?,
        tick,
    }))
}

pub(crate) fn normalize_update_attributes(
    packet: UpdateAttributesPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let tick =
        u64::try_from(packet.tick).map_err(|_| ActorPacketError::NegativeTick(packet.tick))?;
    Ok(ActorEvent::Attributes(ActorAttributesUpdateEvent {
        dimension,
        runtime_id: packet.runtime_entity_id as u64,
        attributes: normalize_player_attributes(packet.attributes)?,
        tick,
    }))
}

pub(crate) fn normalize_player_list(
    packet: PlayerListPacket,
) -> Result<ActorEvent, ActorPacketError> {
    let declared = packet.records.records_count;
    let actual = packet.records.records.len();
    if usize::try_from(declared).ok() != Some(actual) {
        return Err(ActorPacketError::InvalidPlayerListCount { declared, actual });
    }
    check_count("player_list", actual, MAX_PLAYER_LIST_RECORDS)?;
    let mut entries = Vec::with_capacity(actual);
    match packet.records.type_ {
        PlayerRecordsType::Add => {
            let verified = packet
                .records
                .verified
                .ok_or(ActorPacketError::InvalidPlayerListVerifiedCount)?;
            if verified.len() != actual {
                return Err(ActorPacketError::InvalidPlayerListVerifiedCount);
            }
            let mut retained_skin_bytes = 0usize;
            for (record, verified) in packet.records.records.into_iter().zip(verified) {
                let Some(PlayerRecordsRecordsItem::Add(record)) = record else {
                    return Err(ActorPacketError::InvalidPlayerListRecords);
                };
                validate_text(
                    "player_list.username",
                    &record.username,
                    MAX_ACTOR_NAME_BYTES,
                )?;
                let skin = normalize_player_skin(record.skin_data, &mut retained_skin_bytes);
                entries.push(PlayerListEntry::Add {
                    uuid: *record.uuid.as_bytes(),
                    unique_id: record.entity_unique_id,
                    username: Arc::from(record.username),
                    verified,
                    skin,
                });
            }
        }
        PlayerRecordsType::Remove => {
            if packet.records.verified.is_some() {
                return Err(ActorPacketError::InvalidPlayerListVerifiedCount);
            }
            for record in packet.records.records {
                let Some(PlayerRecordsRecordsItem::Remove(record)) = record else {
                    return Err(ActorPacketError::InvalidPlayerListRecords);
                };
                entries.push(PlayerListEntry::Remove {
                    uuid: *record.uuid.as_bytes(),
                });
            }
        }
        PlayerRecordsType::Unknown(action) => {
            return Err(ActorPacketError::UnsupportedPlayerListAction(action));
        }
    }
    Ok(ActorEvent::PlayerList(PlayerListUpdateEvent {
        entries: Arc::from(entries),
    }))
}

fn normalize_player_skin(
    skin: valentine::bedrock::version::v1_26_30::Skin,
    retained_bytes: &mut usize,
) -> PlayerSkin {
    if skin.persona {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedPersona);
    }
    if !skin.animations.is_empty()
        || !skin.personal_pieces.is_empty()
        || !skin.piece_tint_colors.is_empty()
        || !skin.animation_data.is_empty()
    {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedAppearance);
    }
    let geometry = match normalize_player_skin_geometry(
        &skin.arm_size,
        &skin.skin_resource_pack,
        &skin.geometry_data,
    ) {
        Ok(geometry) => geometry,
        Err(unavailable) => return PlayerSkin::Unavailable(unavailable),
    };
    let Ok(width) = u32::try_from(skin.skin_data.width) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
    let Ok(height) = u32::try_from(skin.skin_data.height) else {
        return PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions);
    };
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
    if skin.skin_data.data.len() != expected_bytes {
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
        rgba8: Arc::from(skin.skin_data.data),
        geometry,
    })
}

fn normalize_player_skin_geometry(
    arm_size: &str,
    resource_patch: &str,
    geometry_data: &str,
) -> Result<PlayerSkinGeometry, PlayerSkinUnavailable> {
    let (standard, expected_identifier) = match arm_size {
        "wide" => (PlayerSkinGeometry::Wide, "geometry.humanoid.custom"),
        "slim" => (PlayerSkinGeometry::Slim, "geometry.humanoid.customSlim"),
        _ => return Err(PlayerSkinUnavailable::InvalidArmSize),
    };
    if resource_patch.is_empty() && geometry_data.is_empty() {
        return Ok(standard);
    }
    let patch_identifier = skin_resource_patch_identifier(resource_patch)?;
    if patch_identifier != expected_identifier {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    if geometry_data.is_empty() {
        return Ok(standard);
    }
    if geometry_data.len() > MAX_PLAYER_SKIN_GEOMETRY_BYTES {
        return Err(PlayerSkinUnavailable::GeometryTooLarge);
    }
    let value: serde_json::Value =
        serde_json::from_str(geometry_data).map_err(|_| PlayerSkinUnavailable::InvalidGeometry)?;
    validate_skin_geometry_tree(&value, 0, &mut 0)?;
    let geometry = select_skin_geometry(&value, &patch_identifier)?;
    Ok(PlayerSkinGeometry::Custom {
        identifier: Arc::from(patch_identifier),
        data_sha256: Sha256::digest(
            serde_json::to_vec(geometry).map_err(|_| PlayerSkinUnavailable::InvalidGeometry)?,
        )
        .into(),
    })
}

fn skin_resource_patch_identifier(patch: &str) -> Result<String, PlayerSkinUnavailable> {
    if patch.is_empty() || patch.len() > 4_096 {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    let value: serde_json::Value =
        serde_json::from_str(patch).map_err(|_| PlayerSkinUnavailable::InvalidGeometry)?;
    validate_skin_geometry_tree(&value, 0, &mut 0)?;
    let root = value
        .as_object()
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    if root.len() != 1 {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    let geometry = root
        .get("geometry")
        .and_then(serde_json::Value::as_object)
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    if geometry.len() != 1 {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    geometry
        .get("default")
        .and_then(serde_json::Value::as_str)
        .filter(|identifier| identifier.len() <= MAX_ACTOR_IDENTIFIER_BYTES)
        .map(str::to_owned)
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)
}

fn validate_skin_geometry_tree(
    value: &serde_json::Value,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), PlayerSkinUnavailable> {
    if depth > MAX_PLAYER_SKIN_GEOMETRY_DEPTH {
        return Err(PlayerSkinUnavailable::InvalidGeometry);
    }
    *nodes = nodes
        .checked_add(1)
        .filter(|nodes| *nodes <= MAX_PLAYER_SKIN_GEOMETRY_NODES)
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                validate_skin_geometry_tree(value, depth + 1, nodes)?;
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                validate_skin_geometry_tree(value, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn select_skin_geometry<'a>(
    value: &'a serde_json::Value,
    selected: &str,
) -> Result<&'a serde_json::Value, PlayerSkinUnavailable> {
    if let Some(geometries) = value.get("minecraft:geometry") {
        let geometries = geometries
            .as_array()
            .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
        let matches = geometries
            .iter()
            .filter(|geometry| {
                geometry
                    .get("description")
                    .and_then(|description| description.get("identifier"))
                    .and_then(serde_json::Value::as_str)
                    == Some(selected)
            })
            .collect::<Vec<_>>();
        return match matches.as_slice() {
            [geometry] => Ok(*geometry),
            _ => Err(PlayerSkinUnavailable::InvalidGeometry),
        };
    }
    let object = value
        .as_object()
        .ok_or(PlayerSkinUnavailable::InvalidGeometry)?;
    let matches = object
        .iter()
        .filter(|(identifier, _)| identifier.split(':').next() == Some(selected))
        .map(|(_, geometry)| geometry)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [geometry] => Ok(*geometry),
        _ => Err(PlayerSkinUnavailable::InvalidGeometry),
    }
}

fn normalize_entity_attributes(
    attributes: EntityAttributes,
) -> Result<Arc<[ActorAttribute]>, ActorPacketError> {
    check_count("attributes", attributes.len(), MAX_ACTOR_ATTRIBUTES)?;
    // Skip individual malformed attributes (over-long name, non-finite bound —
    // servers send INFINITY for "unbounded") rather than dropping the actor.
    let normalized = attributes
        .into_iter()
        .filter_map(|attribute| {
            if attribute.name.len() > MAX_ACTOR_NAME_BYTES
                || [attribute.min, attribute.max, attribute.value]
                    .iter()
                    .any(|value| !value.is_finite())
            {
                return None;
            }
            Some(ActorAttribute {
                name: Arc::from(attribute.name),
                min: attribute.min,
                max: attribute.max,
                current: attribute.value,
                default: None,
                modifiers: Arc::from([]),
            })
        })
        .collect::<Vec<_>>();
    Ok(Arc::from(normalized))
}

fn normalize_player_attributes(
    attributes: PlayerAttributes,
) -> Result<Arc<[ActorAttribute]>, ActorPacketError> {
    check_count("attributes", attributes.len(), MAX_ACTOR_ATTRIBUTES)?;
    let normalized = attributes
        .into_iter()
        .filter_map(|attribute| {
            if attribute.name.len() > MAX_ACTOR_NAME_BYTES
                || attribute.modifiers.len() > MAX_ACTOR_ATTRIBUTE_MODIFIERS
                || [
                    attribute.min,
                    attribute.max,
                    attribute.current,
                    attribute.default_min,
                    attribute.default_max,
                    attribute.default,
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
                        serializable: modifier.serializable,
                    })
                })
                .collect::<Vec<_>>();
            Some(ActorAttribute {
                name: Arc::from(attribute.name),
                min: attribute.min,
                max: attribute.max,
                current: attribute.current,
                default: Some(attribute.default),
                modifiers: Arc::from(modifiers),
            })
        })
        .collect::<Vec<_>>();
    Ok(Arc::from(normalized))
}

fn normalize_properties(
    properties: EntityProperties,
) -> Result<Arc<[ActorProperty]>, ActorPacketError> {
    let count = properties
        .ints
        .len()
        .saturating_add(properties.floats.len());
    check_count("properties", count, MAX_ACTOR_PROPERTIES)?;
    let mut normalized = Vec::with_capacity(count);
    normalized.extend(
        properties
            .ints
            .into_iter()
            .map(|property| ActorProperty::Int {
                index: property.index,
                value: property.value,
            }),
    );
    for property in properties.floats {
        // Skip a non-finite custom property value rather than dropping the actor.
        if !property.value.is_finite() {
            continue;
        }
        normalized.push(ActorProperty::Float {
            index: property.index,
            value: property.value,
        });
    }
    Ok(Arc::from(normalized))
}

fn normalize_metadata(
    metadata: MetadataDictionary,
) -> Result<Arc<[ActorMetadata]>, ActorPacketError> {
    check_count("metadata", metadata.len(), MAX_ACTOR_METADATA_ENTRIES)?;
    // Skip individual entries the client cannot model (unknown/newer value
    // types, non-finite floats, oversized payloads) rather than dropping the
    // whole actor. The client renders the entity from the entries it does know.
    let entries = metadata
        .into_iter()
        .filter_map(|entry| {
            let key = metadata_key_id(&entry.key).ok()?;
            let value = match entry.value {
                MetadataDictionaryItemValue::Flags(value) => {
                    ActorMetadataValue::Flags(value.bits())
                }
                MetadataDictionaryItemValue::FlagsExtended(value) => {
                    ActorMetadataValue::FlagsExtended(value.bits())
                }
                MetadataDictionaryItemValue::SeatCameraRelaxDistanceSmoothing(value)
                | MetadataDictionaryItemValue::SeatThirdPersonCameraRadius(value) => value
                    .is_finite()
                    .then_some(ActorMetadataValue::Float(value))?,
                MetadataDictionaryItemValue::Default(value) => match *value {
                    Some(MetadataDictionaryItemValueDefault::Byte(value)) => {
                        ActorMetadataValue::Byte(value)
                    }
                    Some(MetadataDictionaryItemValueDefault::Short(value)) => {
                        ActorMetadataValue::Short(value)
                    }
                    Some(MetadataDictionaryItemValueDefault::Int(value)) => {
                        ActorMetadataValue::Int(value)
                    }
                    Some(MetadataDictionaryItemValueDefault::Float(value)) => value
                        .is_finite()
                        .then_some(ActorMetadataValue::Float(value))?,
                    Some(MetadataDictionaryItemValueDefault::String(value)) => {
                        if value.len() > MAX_ACTOR_METADATA_STRING_BYTES {
                            return None;
                        }
                        ActorMetadataValue::String(Arc::from(value))
                    }
                    Some(MetadataDictionaryItemValueDefault::Compound(value)) => {
                        if value.0.len() > MAX_ACTOR_METADATA_NBT_BYTES {
                            return None;
                        }
                        ActorMetadataValue::Compound(Arc::from(value.0.to_vec()))
                    }
                    Some(MetadataDictionaryItemValueDefault::Vec3I(value)) => {
                        ActorMetadataValue::BlockPosition([value.x, value.y, value.z])
                    }
                    Some(MetadataDictionaryItemValueDefault::Long(value)) => {
                        ActorMetadataValue::Long(value)
                    }
                    Some(MetadataDictionaryItemValueDefault::Vec3F(value)) => {
                        if [value.x, value.y, value.z].iter().any(|c| !c.is_finite()) {
                            return None;
                        }
                        ActorMetadataValue::Vector([value.x, value.y, value.z])
                    }
                    None => return None,
                },
            };
            Some(ActorMetadata { key, value })
        })
        .collect::<Vec<_>>();
    Ok(Arc::from(entries))
}

fn metadata_key_id(key: &MetadataDictionaryItemKey) -> Result<i32, ActorPacketError> {
    let mut bytes = BytesMut::with_capacity(5);
    key.encode(&mut bytes)
        .map_err(|_| ActorPacketError::InvalidMetadataKey)?;
    let mut bytes: Bytes = bytes.freeze();
    VarInt::decode(&mut bytes, ())
        .map(|value| value.0)
        .map_err(|_| ActorPacketError::InvalidMetadataKey)
}

fn rotation_degrees(field: &'static str, bytes: &[u8]) -> Result<f32, ActorPacketError> {
    let [value] = bytes else {
        return Err(ActorPacketError::InvalidRotationBytes {
            field,
            count: bytes.len(),
        });
    };
    Ok(byte_rotation_degrees(*value))
}

fn byte_rotation_degrees(value: u8) -> f32 {
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

#[cfg(test)]
mod skin_geometry_tests {
    use super::*;

    #[test]
    fn resource_patch_selects_one_geometry_from_a_multi_geometry_payload() {
        let slim = serde_json::json!({
            "description": {"identifier": "geometry.humanoid.customSlim"},
            "bones": [{"name": "root"}]
        });
        let payload = serde_json::json!({
            "format_version": "1.12.0",
            "minecraft:geometry": [
                {"description": {"identifier": "geometry.humanoid.custom"}},
                slim.clone()
            ]
        });
        let geometry = normalize_player_skin_geometry(
            "slim",
            r#"{"geometry":{"default":"geometry.humanoid.customSlim"}}"#,
            &serde_json::to_string(&payload).unwrap(),
        )
        .unwrap();
        assert_eq!(
            geometry,
            PlayerSkinGeometry::Custom {
                identifier: "geometry.humanoid.customSlim".into(),
                data_sha256: Sha256::digest(serde_json::to_vec(&slim).unwrap()).into(),
            }
        );
    }

    #[test]
    fn resource_patch_arm_mismatch_and_ambiguous_geometry_fail_closed() {
        let payload = r#"{"minecraft:geometry":[{"description":{"identifier":"geometry.humanoid.custom"}},{"description":{"identifier":"geometry.humanoid.custom"}}]}"#;
        assert_eq!(
            normalize_player_skin_geometry(
                "slim",
                r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#,
                payload,
            ),
            Err(PlayerSkinUnavailable::InvalidGeometry)
        );
        assert_eq!(
            normalize_player_skin_geometry(
                "wide",
                r#"{"geometry":{"default":"geometry.humanoid.custom"}}"#,
                payload,
            ),
            Err(PlayerSkinUnavailable::InvalidGeometry)
        );
    }
}
