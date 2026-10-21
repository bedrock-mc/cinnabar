//! Server-pack entities for one session: compiled at admission into their own index space
//! and layered over the vanilla catalog. A bad file skips its entity, never the session.

use std::{path::PathBuf, sync::Arc};

use assets::{RuntimeEntityAssets, RuntimeEquipmentCatalog};
use resource_pack::LayeredPackView;

pub mod collect;
mod equipment_artwork;

static BASE_EQUIPMENT: std::sync::OnceLock<Arc<RuntimeEquipmentCatalog>> =
    std::sync::OnceLock::new();

/// Installs the validated startup artwork paths before any stack is compiled.
pub fn set_base_equipment_catalog(catalog: Arc<RuntimeEquipmentCatalog>) {
    let _ = BASE_EQUIPMENT.set(catalog);
}
use collect::collect_files;

use crate::pack_textures::parse_pack_json;
use resource_pack::StackFingerprint;

use assets::SessionEntityPack;

/// Vanilla definitions server-pack entities may reference; replaced whenever a carrier loads.
static VANILLA_REFS: std::sync::RwLock<Option<Arc<assets::VanillaEntityRefs>>> =
    std::sync::RwLock::new(None);

/// Replaces the fallback definitions used by subsequent entity compiles.
pub fn set_vanilla_refs(refs: assets::VanillaEntityRefs) {
    *VANILLA_REFS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(refs));
}

/// The vanilla definitions entity compiles currently resolve against.
pub fn vanilla_refs() -> Option<Arc<assets::VanillaEntityRefs>> {
    VANILLA_REFS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

static VANILLA_PACK_DIR: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// The installed vanilla layer supplies rasters omitted by server packs.
pub fn set_vanilla_pack_dir(path: PathBuf) {
    *VANILLA_PACK_DIR
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
}

/// The installed vanilla layer entity compiles currently read rasters from.
pub fn vanilla_pack_dir() -> Option<PathBuf> {
    VANILLA_PACK_DIR
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

mod cache;
use cache::{Compiled, ENTITY_CACHE, EntityInputs};

/// Compiles the stack's entity files: the inner `None` when it defines no usable entity, the
/// outer `None` once `cancelled` stopped the compile between its steps.
pub fn compile_session_entities(
    fingerprint: &StackFingerprint,
    view: &LayeredPackView,
    cancelled: &dyn Fn() -> bool,
) -> Option<Option<Arc<SessionEntityPack>>> {
    let inputs = EntityInputs {
        stack: fingerprint.clone(),
        vanilla: vanilla_refs(),
        vanilla_pack_dir: vanilla_pack_dir(),
        equipment: BASE_EQUIPMENT.get().cloned(),
    };
    ENTITY_CACHE.get_or_compile(
        inputs,
        view,
        crate::compile_cache::current(),
        cancelled,
        compile_encoded,
    )
}

/// Compiles a fixture against explicit vanilla definitions without using the session cache.
#[cfg(feature = "test-support")]
pub fn compile(
    view: &LayeredPackView,
    vanilla: Option<&assets::VanillaEntityRefs>,
    vanilla_pack_dir: Option<&std::path::Path>,
    equipment: Option<&RuntimeEquipmentCatalog>,
) -> Option<Arc<SessionEntityPack>> {
    compile_encoded(view, vanilla, vanilla_pack_dir, equipment, false, &|| false)
        .expect("an uncancellable compile completes")
        .0
}

/// `encode` also returns the entity blob, which only a disk-cached compile needs. Checks
/// `cancelled` between its steps; `None` once it holds.
fn compile_encoded(
    view: &LayeredPackView,
    vanilla: Option<&assets::VanillaEntityRefs>,
    vanilla_pack_dir: Option<&std::path::Path>,
    base_equipment: Option<&RuntimeEquipmentCatalog>,
    encode: bool,
    cancelled: &dyn Fn() -> bool,
) -> Option<Compiled> {
    let mut files = collect_files(view, vanilla, vanilla_pack_dir);
    let artwork_sources = equipment_artwork::collect(view, base_equipment, &mut files);
    if cancelled() {
        return None;
    }
    let mut compiled = match pack_entity_compiler::compile_actor_pack_unless(files, cancelled)? {
        Ok(Some(compiled)) => compiled,
        Ok(None) => return Some((None, None)),
        Err(error) => {
            tracing::warn!(%error, "server pack entities were not applied");
            return Some((None, None));
        }
    };
    if cancelled() {
        return None;
    }
    let skipped = compiled.skipped;
    if skipped != pack_entity_compiler::EntityPackSkips::default() || !compiled.fallbacks.is_empty()
    {
        tracing::warn!(
            oversized = skipped.oversized,
            unparsable = skipped.unparsable,
            over_budget = skipped.over_budget,
            isolated = skipped.isolated,
            rigs_without_artwork = compiled.fallbacks.len(),
            "server pack entities are incomplete"
        );
    }
    equipment_artwork::merge(view, &mut compiled.equipment_textures, artwork_sources);
    if compiled.equipment_textures.is_empty()
        && !compiled.entities.symbols.iter().any(|symbol| {
            matches!(
                symbol.kind,
                assets::EntityAssetKind::Entity | assets::EntityAssetKind::Attachable
            )
        })
    {
        // Invalid artwork-only sources do not publish an empty layer.
        return Some((None, None));
    }
    let equipment =
        if compiled.equipment_bindings.is_empty() && compiled.equipment_textures.is_empty() {
            None
        } else {
            match RuntimeEquipmentCatalog::from_parts(
                compiled.identity,
                compiled.equipment_bindings,
                compiled.equipment_textures,
            ) {
                Ok(catalog) => Some(Arc::new(catalog)),
                Err(error) => {
                    tracing::warn!(%error, "server pack attachables were rejected");
                    None
                }
            }
        };
    let built = if encode {
        RuntimeEntityAssets::from_compiled_encoded(compiled.entities)
    } else {
        RuntimeEntityAssets::from_compiled(compiled.entities).map(|assets| (assets, None))
    };
    let (assets, blob) = match built {
        Ok((assets, blob)) => (Arc::new(assets), blob),
        Err(error) => {
            tracing::warn!(%error, "server pack entity catalog was rejected");
            return Some((None, None));
        }
    };
    let pack = Arc::new(SessionEntityPack {
        assets,
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment,
    });
    Some((Some(pack), blob))
}

/// Reads an entity identifier from a normalized client-entity definition.
pub fn entity_identifier(bytes: &[u8]) -> Option<String> {
    parse_pack_json(bytes)?["minecraft:client_entity"]["description"]["identifier"]
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
