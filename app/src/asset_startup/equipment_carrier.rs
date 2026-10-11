//! Optional attachable equipment carrier. Absence or a stale pin degrades to sprite-only held
//! items and no worn armor; it never blocks startup.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{MAX_EQUIPMENT_CARRIER_BYTES, RuntimeEquipmentCatalog};

use super::{LoadedEntityAssets, shell_quote_path};

const EQUIPMENT_ASSETS_FILENAME: &str = assets::carriers::EQUIPMENT.output;

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
        diagnostics::log_stderr!(
            "equipment carrier unavailable at {} ({error}); worn armor is not drawn and held items use sprites. Build it with `{rebuild}`",
            path.display()
        );
        return None;
    }
    let catalog = match RuntimeEquipmentCatalog::decode(&bytes) {
        Ok(catalog) => catalog,
        Err(error) => {
            diagnostics::log_stderr!(
                "equipment carrier at {} rejected ({error}); rebuild with `{rebuild}`",
                path.display()
            );
            return None;
        }
    };
    if catalog.entity_blob_sha256() != entities.identity {
        diagnostics::log_stderr!(
            "equipment carrier at {} is pinned to a different entity carrier; rebuild with `{rebuild}`",
            path.display()
        );
        return None;
    }
    diagnostics::log_stderr!(
        "loaded equipment carrier from {} ({} bindings, {} textures)",
        path.display(),
        catalog.bindings().len(),
        catalog.textures().len()
    );
    Some(Arc::new(catalog))
}
