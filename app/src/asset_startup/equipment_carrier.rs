//! Optional attachable equipment carrier. Absence or a stale pin degrades to sprite-only held
//! items and no worn armor; it never blocks startup.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{MAX_EQUIPMENT_CARRIER_BYTES, RuntimeBlockEntityAssets, RuntimeEquipmentCatalog};

use super::{LoadedEntityAssets, shell_quote_path};

const EQUIPMENT_ASSETS_FILENAME: &str = "vanilla-v1.mcbeeqp";

pub(crate) fn equipment_asset_path(world: &Path) -> PathBuf {
    world.with_file_name(EQUIPMENT_ASSETS_FILENAME)
}

/// Loads the equipment carrier beside the world blob when it exists and pins the loaded entity
/// carrier; otherwise logs the exact rebuild command and returns `None`.
pub(crate) fn load_optional_equipment_assets(
    world: &Path,
    entities: &LoadedEntityAssets,
) -> Option<Arc<RuntimeEquipmentCatalog>> {
    let path = equipment_asset_path(world);
    let rebuild = format!(
        "make equipment-assets EQUIPMENT_ASSET_BLOB={}",
        shell_quote_path(&path)
    );
    let mut bytes = Vec::new();
    let read = File::open(&path).and_then(|file| {
        file.take(MAX_EQUIPMENT_CARRIER_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
    });
    if let Err(error) = read {
        eprintln!(
            "equipment carrier unavailable at {} ({error}); worn armor is not drawn and held items use sprites. Build it with `{rebuild}`",
            path.display()
        );
        return None;
    }
    let catalog = match RuntimeEquipmentCatalog::decode(&bytes) {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!(
                "equipment carrier at {} rejected ({error}); rebuild with `{rebuild}`",
                path.display()
            );
            return None;
        }
    };
    if catalog.entity_blob_sha256() != entities.identity {
        eprintln!(
            "equipment carrier at {} is pinned to a different entity carrier; rebuild with `{rebuild}`",
            path.display()
        );
        return None;
    }
    eprintln!(
        "loaded equipment carrier from {} ({} bindings, {} textures)",
        path.display(),
        catalog.bindings().len(),
        catalog.textures().len()
    );
    Some(Arc::new(catalog))
}

/// The optional block-entity carrier, read here only for the skull textures worn heads reuse;
/// absence just leaves worn heads undrawn (the block-entity scene reports it).
pub(crate) fn load_optional_block_entity_assets(
    world: &Path,
) -> Option<Arc<RuntimeBlockEntityAssets>> {
    let path = world.with_file_name(crate::block_entities::BLOCK_ENTITY_ASSETS_FILENAME);
    let bytes = match diagnostics::bounded_file::read(
        &path,
        assets::MAX_BLOCK_ENTITY_CARRIER_BYTES as u64,
    ) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "worn-head carrier {} unavailable ({error}); rebuild with: make block-entity-assets",
                path.display()
            );
            return None;
        }
    };
    match RuntimeBlockEntityAssets::decode(&bytes) {
        Ok(assets) => Some(Arc::new(assets)),
        Err(error) => {
            eprintln!(
                "worn-head carrier {} rejected ({error}); rebuild with: make block-entity-assets",
                path.display()
            );
            None
        }
    }
}
