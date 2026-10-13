use super::*;
use bedrock_protocol_1_26_51::packets::AddPainting;

/// Motive and wall direction retained from the dedicated painting spawn packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintingData {
    pub motive: Arc<str>,
    pub direction: PaintingDirection,
}

/// The four horizontal hanging-actor directions carried by AddPainting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintingDirection {
    South,
    West,
    North,
    East,
}

impl PaintingDirection {
    /// Retains a valid wire direction without folding unknown values into a wall.
    pub const fn from_wire(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::South),
            1 => Some(Self::West),
            2 => Some(Self::North),
            3 => Some(Self::East),
            _ => None,
        }
    }

    /// Hanging actors swap the north/south yaw indices before converting to degrees.
    pub const fn yaw_degrees(self) -> f32 {
        match self {
            Self::South => 180.0,
            Self::West => 90.0,
            Self::North => 0.0,
            Self::East => 270.0,
        }
    }
}

/// Admits the dedicated spawn packet, preserving the centre and both actor ID domains.
pub(crate) fn normalize_add_painting(
    packet: AddPainting,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    let direction = PaintingDirection::from_wire(packet.direction.0).ok_or(
        ActorPacketError::InvalidPaintingDirection(packet.direction.0),
    )?;
    validate_text("painting.motive", &packet.motif, MAX_ACTOR_IDENTIFIER_BYTES)?;
    for (field, value) in [
        ("position.x", packet.position.x),
        ("position.y", packet.position.y),
        ("position.z", packet.position.z),
    ] {
        validate_finite(field, value)?;
    }
    let yaw = direction.yaw_degrees();
    Ok(ActorEvent::PaintingSpawn {
        painting: PaintingData {
            motive: Arc::from(packet.motif.to_string_lossy()),
            direction,
        },
        spawn: ActorSpawnEvent {
            dimension,
            unique_id: packet.target_actor_id.0,
            runtime_id: packet.target_runtime_id.0,
            kind: ActorKind::Entity {
                identifier: Arc::from("minecraft:painting"),
            },
            position: packet.position.to_array(),
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw,
            head_yaw: yaw,
            body_yaw: yaw,
            held_item: NetworkItemStack::empty(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        },
    })
}
