//! Hash-checked entity facts and native class dispositions.

use crate::{
    AssetError,
    entity::{
        interactions::EntityInteractionTraits, mount::MountEquipment,
        placement::EntityPlacementFrame,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

macro_rules! source {
    ($file:literal) => {
        pub const FILENAME: &str = $file;
        pub const SOURCE_PATH: &str = concat!("crates/assets/data/", $file);
        pub const SOURCE_BYTES: &[u8] = include_bytes!(concat!("../../data/", $file));
    };
}
source!("entity-facts-v2193.bin");
pub const EXPANSION_PATH: &str = "crates/assets/data/entity-facts-expansion-v2193.tsv";
pub const MAGIC: &[u8; 8] = b"EFACT001";
pub const MAX_BYTES: usize = 1024 * 1024;
/// Interaction-box inflation when an actor has no constructor override.
pub const DEFAULT_ACTOR_PICK_RADIUS: f32 = 0.1;

/// Native behavior families used by class-specific actor consumers.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub enum NativeEntityClass {
    #[default]
    Unresolved,
    Ordinary,
    Player,
    Arrow,
    Boat,
    Minecart,
    Horse,
    Creeper,
    EnderDragon,
    EnderCrystal,
    FallingBlock,
    Tnt,
    Painting,
    FishingHook,
    LightningBolt,
    Item,
    XpOrb,
    Wolf,
    SplashPotion,
    LingeringPotion,
    Tropicalfish,
    DragonFireball,
    FireworksRocket,
    AreaEffectCloud,
}

impl NativeEntityClass {
    /// Returns the canonical asset identity only when this class has one pinned constructor.
    pub fn identifier(self) -> Option<&'static str> {
        static NAMES: OnceLock<std::collections::HashMap<NativeEntityClass, Option<&'static str>>> =
            OnceLock::new();
        NAMES
            .get_or_init(|| {
                let mut names = std::collections::HashMap::new();
                for record in &EntityFacts::pinned().records {
                    names
                        .entry(record.fact.native_class)
                        .and_modify(|name| *name = None)
                        .or_insert(Some(record.identifier.as_ref()));
                }
                names
            })
            .get(&self)
            .copied()
            .flatten()
    }
}

/// Boat and chest-boat texture aliases in actor-variant order; the variant indexes this list.
pub const BOAT_WOOD_TEXTURES: [&str; 11] = [
    "oak", "spruce", "birch", "jungle", "acacia", "darkoak", "mangrove", "bamboo", "cherry",
    "paleoak", "poplar",
];
/// The boat variant drawn with the raft model rather than the hull.
pub const BOAT_RAFT_VARIANT: usize = 7;

/// A class's radius formula, with state inputs supplied by the actor.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub enum ShadowRadius {
    Width(f32),
    BabyWidth { adult: f32, baby: f32 },
    Variant(f32),
    Fixed(f32),
    RidingHidden(f32),
}
impl ShadowRadius {
    /// Evaluates the compiled formula without resolving an identifier.
    pub fn radius(self, width: f32, baby: bool, variant: i32, riding: bool) -> f32 {
        match self {
            Self::Width(scale) => width * scale,
            Self::BabyWidth { adult, baby: scale } => width * if baby { scale } else { adult },
            Self::Variant(scale) => variant.max(0) as f32 * scale,
            Self::Fixed(radius) => radius,
            Self::RidingHidden(scale) => {
                if riding {
                    0.0
                } else {
                    width * scale
                }
            }
        }
    }
    /// Checks every coefficient before a formula enters the runtime.
    fn valid(self) -> bool {
        let values = match self {
            Self::BabyWidth { adult, baby } => [adult, baby],
            Self::Width(v) | Self::Variant(v) | Self::Fixed(v) | Self::RidingHidden(v) => [v, v],
        };
        values.into_iter().all(|v| v.is_finite() && v >= 0.0)
    }
}

/// Cached facts for one native constructor; custom actors copy their advertised base's record.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityFact {
    pub native_class: NativeEntityClass,
    /// Stable within this carrier's sorted constructor inventory; zero is unresolved.
    pub class_key: u16,
    pub shadow: ShadowRadius,
    pub shadow_drop: f32,
    pub network_y_offset: f32,
    pub projectile: bool,
    pub horse: bool,
    pub living_mount: bool,
    pub aquatic: bool,
    #[serde(default)]
    pub fish_animation: bool,
    /// The default definition enables solid actor collision; synced flags may override it.
    pub collidable: bool,
    pub interactions: EntityInteractionTraits,
    pub placement: EntityPlacementFrame,
    pub equipment: MountEquipment,
    pub collision: Option<[f32; 2]>,
    /// Absolute interaction-box inflation, independent of the actor's scale.
    pub pick_radius: f32,
    /// At least one native compatibility rule still has incomplete parity coverage.
    pub provisional: bool,
}
impl Default for EntityFact {
    /// Leaves unknown constructors unresolved while preserving the generic shadow formula.
    fn default() -> Self {
        Self {
            native_class: NativeEntityClass::Unresolved,
            class_key: 0,
            shadow: ShadowRadius::Width(1.0),
            shadow_drop: 0.0,
            network_y_offset: 0.0,
            projectile: false,
            horse: false,
            living_mount: false,
            aquatic: false,
            fish_animation: false,
            collidable: false,
            interactions: EntityInteractionTraits::Unresolved,
            placement: EntityPlacementFrame::Unresolved,
            equipment: MountEquipment::Unresolved,
            collision: None,
            pick_radius: DEFAULT_ACTOR_PICK_RADIUS,
            provisional: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityFactRecord {
    pub identifier: Box<str>,
    pub fact: EntityFact,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityFacts {
    pub protocol: u32,
    pub actors_sha256: [u8; 32],
    pub records: Box<[EntityFactRecord]>,
}
impl EntityFacts {
    /// Validates framing, checksum, target, ordering and numeric facts before retention.
    pub fn decode(bytes: &[u8], protocol: u32) -> Result<Self, AssetError> {
        let bad = || AssetError::InvalidCompiledAssets {
            detail: "invalid entity facts; rebuild with make assets".into(),
        };
        if bytes.len() > MAX_BYTES || bytes.len() < MAGIC.len() + 32 || !bytes.starts_with(MAGIC) {
            return Err(bad());
        }
        let (payload, hash) = bytes.split_at(bytes.len() - 32);
        if <[u8; 32]>::from(Sha256::digest(payload)).as_slice() != hash {
            return Err(bad());
        }
        let facts: Self = serde_json::from_slice(&payload[MAGIC.len()..]).map_err(|_| bad())?;
        if facts.protocol != protocol
            || facts.records.is_empty()
            || facts.records.len() > u16::MAX as usize
            || facts.actors_sha256
                != <[u8; 32]>::from(Sha256::digest(include_bytes!(
                    "../../../protocol/data/retail_actors_1_26_50.txt"
                )))
            || !facts
                .records
                .windows(2)
                .all(|pair| pair[0].identifier < pair[1].identifier)
            || !facts.records.iter().enumerate().all(|(i, r)| {
                r.identifier.starts_with("minecraft:")
                    && r.fact.class_key as usize == i + 1
                    && r.fact.shadow.valid()
                    && r.fact.shadow_drop.is_finite()
                    && r.fact.network_y_offset.is_finite()
                    && r.fact.network_y_offset >= 0.0
                    && r.fact.pick_radius.is_finite()
                    && r.fact.pick_radius >= 0.0
                    && r.fact
                        .collision
                        .is_none_or(|v| v.into_iter().all(|n| n.is_finite() && n > 0.0))
            })
        {
            return Err(bad());
        }
        if !include_str!("../../../protocol/data/retail_actors_1_26_50.txt")
            .lines()
            .all(|name| {
                facts
                    .records
                    .binary_search_by(|r| r.identifier.as_ref().cmp(name))
                    .is_ok()
            })
        {
            return Err(bad());
        }
        Ok(facts)
    }
    /// Rejects corrupt or stale bytes that differ from this build's pinned carrier.
    pub fn validate_pinned(bytes: &[u8]) -> Result<(), AssetError> {
        Self::decode(bytes, crate::active_content_registry_protocol())?;
        if bytes != SOURCE_BYTES {
            return Err(AssetError::InvalidCompiledAssets {
                detail: "entity facts differ from the pinned carrier; rebuild with make assets"
                    .into(),
            });
        }
        Ok(())
    }

    /// Encodes deterministic compiler output and validates it using the runtime reader.
    pub fn encode(&self) -> Result<Vec<u8>, AssetError> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(
            serde_json::to_vec(self).map_err(|e| AssetError::InvalidCompiledAssets {
                detail: e.to_string().into(),
            })?,
        );
        bytes.extend_from_slice(&Sha256::digest(&bytes));
        Self::decode(&bytes, self.protocol)?;
        Ok(bytes)
    }
    /// Returns the checked-in carrier once; startup validates the same bytes before admission.
    pub fn pinned() -> &'static Self {
        static FACTS: OnceLock<EntityFacts> = OnceLock::new();
        FACTS.get_or_init(|| {
            Self::decode(SOURCE_BYTES, crate::active_content_registry_protocol())
                .expect("validated entity facts")
        })
    }
    /// Returns the compiled player constructor for player packet admission.
    pub fn player(&self) -> EntityFact {
        self.get("minecraft:player")
    }

    /// Resolves an exact constructor identity only during asset or actor admission.
    pub fn get(&self, identifier: &str) -> EntityFact {
        self.records
            .binary_search_by(|r| r.identifier.as_ref().cmp(identifier))
            .ok()
            .map_or_else(EntityFact::default, |i| self.records[i].fact)
    }
}

#[cfg(test)]
mod tests;
