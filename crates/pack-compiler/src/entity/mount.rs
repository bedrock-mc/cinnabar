//! Compiles one equipment disposition for each independently collected entity symbol.

use assets::{EntityAssetKind, EntityAssetSource, EntityAssetSymbol, entity::mount::*};
use sha2::{Digest, Sha256};

/// Binds the compatibility catalog only for built-in compilation; packs remain unresolved.
pub(super) fn admit(sources: &mut Vec<EntityAssetSource>, builtin: bool) {
    if builtin {
        sources.push(EntityAssetSource {
            path: MOUNT_EQUIPMENT_SOURCE_PATH.into(),
            source_bytes: MOUNT_EQUIPMENT_SOURCE_BYTES.len() as u32,
            source_sha256: Sha256::digest(MOUNT_EQUIPMENT_SOURCE_BYTES).into(),
        });
    }
}

/// Resolves each entity from the collected symbol inventory without inferring custom capabilities.
pub(super) fn compile(symbols: &mut [EntityAssetSymbol], builtin: bool) {
    for symbol in symbols {
        symbol.mount_equipment = if builtin && symbol.kind == EntityAssetKind::Entity {
            builtin_mount_equipment(&symbol.identifier)
        } else {
            MountEquipment::Unresolved
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Makes an independent symbol inventory, including an unknown entity and non-entity alias.
    fn inventory() -> Vec<EntityAssetSymbol> {
        [
            (EntityAssetKind::Entity, "minecraft:llama"),
            (EntityAssetKind::Entity, "fixture:custom_mount"),
            (EntityAssetKind::Geometry, "minecraft:horse"),
        ]
        .into_iter()
        .map(|(kind, identifier)| EntityAssetSymbol {
            kind,
            identifier: identifier.into(),
            source_index: 0,
            dependencies: Box::default(),
            mount_equipment: MountEquipment::Unresolved,
        })
        .collect()
    }

    #[test]
    fn every_collected_symbol_keeps_an_explicit_equipment_disposition() {
        let mut symbols = inventory();
        compile(&mut symbols, true);
        assert_eq!(
            symbols[0].mount_equipment,
            MountEquipment::Provisional(MountSlots {
                saddle: false,
                body: MountBody::Carpet,
            })
        );
        assert_eq!(symbols[1].mount_equipment, MountEquipment::Unresolved);
        assert_eq!(symbols[2].mount_equipment, MountEquipment::Unresolved);
        assert_eq!(MountEquipment::Unresolved.slots(), None);
        assert_eq!(
            MountEquipment::NotEquippable.slots(),
            Some(MountSlots {
                saddle: false,
                body: MountBody::None,
            })
        );
    }

    #[test]
    fn server_pack_aliases_do_not_claim_builtin_equipment() {
        let mut symbols = inventory();
        compile(&mut symbols, true);
        compile(&mut symbols, false);
        assert!(
            symbols
                .iter()
                .all(|symbol| symbol.mount_equipment == MountEquipment::Unresolved)
        );
    }
}
