//! Equipment capabilities admitted independently of a mount's screen or artwork.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{EntityAssetKind, EntityAssetSource, EntityAssetSymbol, invalid};
use crate::AssetError;

pub const MOUNT_EQUIPMENT_SOURCE_PATH: &str = "registry/mount-equipment.json";
pub const MOUNT_EQUIPMENT_SOURCE_BYTES: &[u8] = crate::facts::entity::SOURCE_BYTES;

/// The admitted kind of equipment worn in the body slot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MountBody {
    None,
    HorseArmor,
    Carpet,
    NautilusArmor,
}

/// Equipment cells supported by one admitted capability.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MountSlots {
    pub saddle: bool,
    pub body: MountBody,
}

impl MountSlots {
    /// Empty cells used only when drawing an unresolved or admitted empty layout.
    pub const EMPTY: Self = Self {
        saddle: false,
        body: MountBody::None,
    };
}

/// Unknown capabilities remain distinct from an admitted empty equipment layout.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum MountEquipment {
    #[default]
    Unresolved,
    NotEquippable,
    Equippable(MountSlots),
    /// Preserves an existing layout while component activation remains incomplete.
    Provisional(MountSlots),
}

impl MountEquipment {
    /// Returns admitted cells, retaining `None` for unresolved capabilities.
    pub const fn slots(self) -> Option<MountSlots> {
        match self {
            Self::Unresolved => None,
            Self::NotEquippable => Some(MountSlots::EMPTY),
            Self::Equippable(slots) | Self::Provisional(slots) => Some(slots),
        }
    }

    /// Hides unavailable equipment cells while preserving the server's storage cells.
    pub const fn permits_inventory_slot(self, slot: u8) -> bool {
        let slots = match self.slots() {
            Some(slots) => slots,
            None => MountSlots::EMPTY,
        };
        match slot {
            0 => slots.saddle,
            1 => !matches!(slots.body, MountBody::None),
            _ => true,
        }
    }
}

/// Resolves compiled equipment cells during asset or actor admission.
pub fn builtin_mount_equipment(identifier: &str) -> MountEquipment {
    crate::facts::entity::EntityFacts::pinned()
        .get(identifier)
        .equipment
}

/// Rejects unsupported assignments and facts without their exact source identity.
pub(super) fn validate_mount_equipment(
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
) -> Result<(), AssetError> {
    let source = sources
        .iter()
        .find(|source| source.path.as_ref() == MOUNT_EQUIPMENT_SOURCE_PATH);
    if source.is_some_and(|source| {
        source.source_bytes as usize != MOUNT_EQUIPMENT_SOURCE_BYTES.len()
            || source.source_sha256
                != <[u8; 32]>::from(Sha256::digest(MOUNT_EQUIPMENT_SOURCE_BYTES))
    }) {
        return Err(invalid(
            "mount equipment source identity differs; rebuild with make assets",
        ));
    }
    for symbol in symbols {
        if symbol.mount_equipment == MountEquipment::Unresolved {
            continue;
        }
        if symbol.kind != EntityAssetKind::Entity
            || source.is_none()
            || symbol.mount_equipment != builtin_mount_equipment(&symbol.identifier)
        {
            return Err(invalid(
                "mount equipment is outside its compiled source inventory",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a source-bound entity disposition without requiring local carriers.
    fn fixture() -> (Vec<EntityAssetSource>, Vec<EntityAssetSymbol>) {
        (
            vec![EntityAssetSource {
                path: MOUNT_EQUIPMENT_SOURCE_PATH.into(),
                source_bytes: MOUNT_EQUIPMENT_SOURCE_BYTES.len() as u32,
                source_sha256: Sha256::digest(MOUNT_EQUIPMENT_SOURCE_BYTES).into(),
            }],
            vec![EntityAssetSymbol {
                kind: EntityAssetKind::Entity,
                identifier: "minecraft:llama".into(),
                source_index: 0,
                dependencies: Box::default(),
                mount_equipment: builtin_mount_equipment("minecraft:llama"),
            }],
        )
    }

    #[test]
    fn equipment_requires_its_hash_checked_catalog_and_entity_inventory() {
        let (sources, symbols) = fixture();
        assert!(validate_mount_equipment(&sources, &symbols).is_ok());
        assert!(validate_mount_equipment(&[], &symbols).is_err());
        let mut forged = sources.clone();
        forged[0].source_sha256[0] ^= 1;
        assert!(validate_mount_equipment(&forged, &symbols).is_err());
        let mut forged = symbols.clone();
        forged[0].identifier = "fixture:custom_mount".into();
        assert!(validate_mount_equipment(&sources, &forged).is_err());
        forged[0].identifier = "minecraft:llama".into();
        forged[0].kind = EntityAssetKind::Geometry;
        assert!(validate_mount_equipment(&sources, &forged).is_err());
    }
}
