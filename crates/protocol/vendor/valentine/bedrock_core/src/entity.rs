//! Entity data types for Minecraft Bedrock.
//!
//! Provides static entity definitions from minecraft-data.

/// Entity behavior type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityType {
    Animal,
    Hostile,
    Passive,
    Ambient,
    Mob,
    Player,
    Living,
    Projectile,
    Other,
    /// Empty string or unknown type
    Unknown,
}

impl std::str::FromStr for EntityType {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "animal" => Self::Animal,
            "hostile" => Self::Hostile,
            "passive" => Self::Passive,
            "ambient" => Self::Ambient,
            "mob" => Self::Mob,
            "player" => Self::Player,
            "living" => Self::Living,
            "projectile" => Self::Projectile,
            "other" => Self::Other,
            _ => Self::Unknown,
        })
    }
}

/// Static entity definition data.
///
/// Contains the protocol-level information about an entity type.
/// For behavior and AI, see jolyne's entity implementations.
#[derive(Debug, Clone, Copy)]
pub struct EntityData {
    /// Unique entity ID in this version.
    pub id: u32,
    /// Internal entity ID (may have duplicates across entities).
    pub internal_id: u32,
    /// String identifier with namespace (e.g., "minecraft:zombie").
    pub string_id: &'static str,
    /// Display name (e.g., "Zombie").
    pub name: &'static str,
    /// Entity hitbox height.
    pub height: f32,
    /// Entity hitbox width (None if not provided).
    pub width: Option<f32>,
    /// Entity hitbox length (None if not provided).
    pub length: Option<f32>,
    /// Entity offset (None if not provided).
    pub offset: Option<f32>,
    /// Entity behavior type.
    pub entity_type: EntityType,
    /// UI category (e.g., "Hostile mobs").
    pub category: &'static str,
}

impl EntityData {
    /// Get width or default to height (square hitbox).
    #[inline]
    pub fn width_or_height(&self) -> f32 {
        self.width.unwrap_or(self.height)
    }

    /// Get length or default to width.
    #[inline]
    pub fn length_or_width(&self) -> f32 {
        self.length.unwrap_or_else(|| self.width_or_height())
    }
}
