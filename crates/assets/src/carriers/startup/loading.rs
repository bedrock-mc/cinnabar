//! Parallel runtime carrier loading and diagnostic fallback construction.
use super::{
    ATMOSPHERE_FILENAME, AssetSelection, AssetStartupError, COMPILE_COMMAND, CarrierMetrics,
    ENTITY_ASSETS_FILENAME, FETCH_COMMAND, FONT_ASSETS_FILENAME, LOCAL_FONT_ASSETS_FILENAME,
    LoadedAssetKind, LoadedAssets, LoadedAtmosphereAssets, LoadedEntityAssets, LoadedFontAssets,
    MAX_RUNTIME_BLOB_BYTES, VANILLA_SOURCE_JSON, format_sha256,
    optional_carriers::{
        load_atmosphere_assets, load_entity_assets, load_font_assets, load_material_keys,
        load_vanilla_entity_refs,
    },
    require_entity_facts, world_provenance,
};
use crate::RuntimeAssets;
use serde::Deserialize;
use std::path::PathBuf;
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
};

#[derive(Deserialize)]
struct VanillaSource {
    tag: String,
    sha256: String,
}

/// Selects the atmosphere carrier beside the world carrier.
#[must_use]
pub fn atmosphere_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ATMOSPHERE_FILENAME)
}

/// Selects the entity carrier beside the world carrier.
#[must_use]
pub fn entity_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ENTITY_ASSETS_FILENAME)
}

/// Selects the bundled font carrier beside the world carrier.
#[must_use]
pub fn font_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(FONT_ASSETS_FILENAME)
}

/// Selects the optional local font carrier beside the world carrier.
#[must_use]
pub fn local_font_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(LOCAL_FONT_ASSETS_FILENAME)
}

/// Loads and validates the runtime carriers without recording wall-clock timings.
pub fn load_runtime_assets(selection: AssetSelection) -> Result<LoadedAssets, AssetStartupError> {
    load_runtime_assets_observed(selection, &())
}

/// Loads the required facts and artwork carriers, preserving world error priority.
pub fn load_runtime_assets_observed(
    selection: AssetSelection,
    observer: &impl CarrierLoadObserver,
) -> Result<LoadedAssets, AssetStartupError> {
    let item_facts = observer.observe("item-facts", || super::require_item_facts(&selection.path));
    let source: VanillaSource = serde_json::from_str(VANILLA_SOURCE_JSON)?;
    let path = selection.path.as_path();
    let (world, atmosphere, entities, fonts) = std::thread::scope(|scope| {
        let atmosphere =
            scope.spawn(|| observer.observe("atmosphere", || load_atmosphere_assets(path)));
        let entities = scope.spawn(|| observer.observe("entity", || load_entity_assets(path)));
        let fonts = scope.spawn(|| observer.observe("font", || load_font_assets(path)));
        let world = observer.observe("world", || load_world_carrier(path));
        (world, join(atmosphere), join(entities), join(fonts))
    });
    let world = world?;
    // Preserve the existing required-carrier error order before checking the facts sibling.
    let atmosphere = atmosphere?;
    let entities = entities?;
    observer.observe("entity facts", || require_entity_facts(path))?;
    let Some(world) = world else {
        return Ok(diagnostic_assets(
            selection,
            source,
            atmosphere,
            entities,
            fonts?,
            item_facts?,
        ));
    };
    let metrics = runtime_metrics(&world.runtime, source, world.blob_sha256);
    Ok(LoadedAssets {
        runtime: world.runtime,
        item_facts: item_facts?,
        material_keys: world.material_keys,
        vanilla_entity_refs: world.vanilla_entity_refs,
        atmosphere,
        entities,
        fonts: fonts?,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::CompiledBlob,
        notice: None,
    })
}

/// Loads the menu's required atmosphere and font without touching world-only carriers.
pub fn load_menu_assets(
    path: &Path,
    observer: &impl CarrierLoadObserver,
) -> Result<super::MenuAssets, AssetStartupError> {
    let (atmosphere, fonts) = std::thread::scope(|scope| {
        let fonts = scope.spawn(|| observer.observe("font", || load_font_assets(path)));
        (
            observer.observe("atmosphere", || load_atmosphere_assets(path)),
            join(fonts),
        )
    });
    Ok(super::MenuAssets {
        atmosphere: atmosphere?,
        fonts: fonts?,
    })
}

/// Loads world-only carriers after admission, reusing the menu's already validated artwork.
pub fn load_world_assets_observed(
    selection: AssetSelection,
    menu: super::MenuAssets,
    observer: &impl CarrierLoadObserver,
) -> Result<LoadedAssets, AssetStartupError> {
    let path = selection.path.as_path();
    let (world, entities, item_facts) = std::thread::scope(|scope| {
        let entities = scope.spawn(|| observer.observe("entity", || load_entity_assets(path)));
        let item_facts =
            scope.spawn(|| observer.observe("item-facts", || super::require_item_facts(path)));
        (
            observer.observe("world", || load_world_carrier(path)),
            join(entities),
            join(item_facts),
        )
    });
    let world = world?;
    let entities = entities?;
    observer.observe("entity facts", || require_entity_facts(path))?;
    let source = serde_json::from_str(VANILLA_SOURCE_JSON)?;
    let Some(world) = world else {
        return Ok(diagnostic_assets(
            selection,
            source,
            menu.atmosphere,
            entities,
            menu.fonts,
            item_facts?,
        ));
    };
    let metrics = runtime_metrics(&world.runtime, source, world.blob_sha256);
    Ok(LoadedAssets {
        runtime: world.runtime,
        item_facts: item_facts?,
        material_keys: world.material_keys,
        vanilla_entity_refs: world.vanilla_entity_refs,
        atmosphere: menu.atmosphere,
        entities,
        fonts: menu.fonts,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::CompiledBlob,
        notice: None,
    })
}

/// The decoded world carrier, identity and optional pack sidecars.
struct LoadedWorldAssets {
    runtime: Arc<RuntimeAssets>,
    blob_sha256: String,
    material_keys: Option<crate::material_keys::MaterialKeys>,
    vanilla_entity_refs: Option<crate::VanillaEntityRefs>,
}

/// The decoded world carrier and its SHA-256, or `None` when it is absent.
fn load_world_carrier(path: &Path) -> Result<Option<LoadedWorldAssets>, AssetStartupError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AssetStartupError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    if file
        .metadata()
        .map_err(|source| AssetStartupError::Read {
            path: path.to_owned(),
            source,
        })?
        .len()
        > MAX_RUNTIME_BLOB_BYTES
    {
        return Err(AssetStartupError::TooLarge {
            path: path.to_owned(),
            max_bytes: MAX_RUNTIME_BLOB_BYTES,
        });
    }

    let mut bytes = Vec::new();
    file.take(MAX_RUNTIME_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::Read {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > MAX_RUNTIME_BLOB_BYTES {
        return Err(AssetStartupError::TooLarge {
            path: path.to_owned(),
            max_bytes: MAX_RUNTIME_BLOB_BYTES,
        });
    }

    let (runtime, identity) =
        RuntimeAssets::decode_sealed(&bytes).map_err(|source| AssetStartupError::Decode {
            path: path.to_owned(),
            source: Box::new(source),
            rebuild_command: COMPILE_COMMAND,
        })?;
    let runtime = Arc::new(runtime);
    let material_keys = load_material_keys(path, runtime.material_count());
    let vanilla_entity_refs = load_vanilla_entity_refs(path);
    world_provenance::verify_world_carrier(path, &runtime)?;
    Ok(Some(LoadedWorldAssets {
        runtime,
        blob_sha256: format_sha256(identity),
        material_keys,
        vanilla_entity_refs,
    }))
}

/// Observes a carrier load without making the asset loader depend on a clock.
pub trait CarrierLoadObserver: Sync {
    /// Runs the load and returns its result, allowing the caller to record its duration.
    fn observe<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T;
}

impl CarrierLoadObserver for () {
    /// Runs the carrier load without instrumentation.
    fn observe<T>(&self, _carrier: &'static str, load: impl FnOnce() -> T) -> T {
        load()
    }
}

/// Joins a scoped load, re-raising its panic on this thread.
fn join<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Supplies diagnostic terrain while preserving the validated sibling carriers.
fn diagnostic_assets(
    selection: AssetSelection,
    source: VanillaSource,
    atmosphere: LoadedAtmosphereAssets,
    entities: LoadedEntityAssets,
    fonts: LoadedFontAssets,
    item_facts: Arc<crate::facts::ItemFacts>,
) -> LoadedAssets {
    let runtime = Arc::new(RuntimeAssets::diagnostic());
    let metrics = runtime_metrics(&runtime, source, "diagnostic".to_owned());
    let notice = format!(
        "compiled vanilla assets were not found at {}; using the programmatic diagnostic texture\n\
         Fetch and compile the local vanilla pack explicitly (the app never downloads it):\n  {FETCH_COMMAND}\n  {}",
        selection.path.display(),
        COMPILE_COMMAND
    );
    LoadedAssets {
        runtime,
        item_facts,
        material_keys: None,
        vanilla_entity_refs: None,
        atmosphere,
        entities,
        fonts,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::Diagnostic,
        notice: Some(notice),
    }
}

/// Counts the decoded terrain data for startup and runtime diagnostics.
fn runtime_metrics(
    runtime: &RuntimeAssets,
    source: VanillaSource,
    blob_sha256: String,
) -> CarrierMetrics {
    let pages = runtime.texture_pages();
    CarrierMetrics {
        source_tag: source.tag,
        source_sha256: source.sha256,
        blob_sha256,
        texture_layers: pages.iter().map(|page| page.texture.layers).sum(),
        texture_pages: u32::try_from(pages.len()).unwrap_or(u32::MAX),
        texture_bytes_including_mips: pages
            .iter()
            .flat_map(|page| page.texture.mips.iter())
            .map(|mip| mip.rgba8.len() as u64)
            .sum(),
        material_count: u32::try_from(runtime.materials().len()).unwrap_or(u32::MAX),
        model_template_count: u32::try_from(runtime.model_templates().len()).unwrap_or(u32::MAX),
        model_quad_count: u32::try_from(runtime.model_quads().len()).unwrap_or(u32::MAX),
        animation_count: u32::try_from(runtime.animations().len()).unwrap_or(u32::MAX),
        animation_frame_count: u32::try_from(runtime.animation_frames().len()).unwrap_or(u32::MAX),
        missing_mapping_count: runtime.missing_count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carriers::startup::test_carriers::{
        synthetic_atmosphere_blob, synthetic_entity_blob, write_pinned_facts,
    };

    #[derive(Default)]
    struct Loads(std::sync::Mutex<Vec<&'static str>>);

    impl CarrierLoadObserver for Loads {
        /// Records each carrier load without reading the clock.
        fn observe<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T {
            self.0.lock().unwrap().push(carrier);
            load()
        }
    }

    #[test]
    fn menu_loading_never_opens_world_entity_or_physics_carriers() {
        let directory = tempfile::tempdir().unwrap();
        let world = directory.path().join(crate::carriers::WORLD.output);
        std::fs::write(atmosphere_asset_path(&world), synthetic_atmosphere_blob()).unwrap();
        // Corrupt world-only siblings must remain unread until world admission.
        std::fs::write(&world, b"not a world carrier").unwrap();
        std::fs::write(entity_asset_path(&world), b"not an entity carrier").unwrap();
        let observer = Loads::default();
        let loaded = load_menu_assets(&world, &observer).unwrap();
        assert!(loaded.fonts.is_diagnostic());
        let mut loads = observer.0.into_inner().unwrap();
        loads.sort_unstable();
        assert_eq!(loads, ["atmosphere", "font"]);
        assert!(
            load_world_assets_observed(
                super::super::select_asset_path(Some(&world), None),
                loaded,
                &()
            )
            .is_err()
        );
    }

    #[test]
    fn diagnostic_startup_loads_every_required_sibling_once() {
        let directory = tempfile::tempdir().unwrap();
        let world = directory.path().join(crate::carriers::WORLD.output);
        write_pinned_facts(&world);
        std::fs::write(atmosphere_asset_path(&world), synthetic_atmosphere_blob()).unwrap();
        std::fs::write(entity_asset_path(&world), synthetic_entity_blob(0)).unwrap();
        let observer = Loads::default();
        let loaded = load_runtime_assets_observed(
            super::super::select_asset_path(Some(&world), None),
            &observer,
        )
        .unwrap();
        assert_eq!(loaded.kind, LoadedAssetKind::Diagnostic);
        assert!(Arc::ptr_eq(
            &loaded.item_facts,
            crate::facts::ItemFacts::builtin().unwrap(),
        ));
        let mut loads = observer.0.into_inner().unwrap();
        loads.sort_unstable();
        assert_eq!(
            loads,
            [
                "atmosphere",
                "entity",
                "entity facts",
                "font",
                "item-facts",
                "world"
            ]
        );
    }
}
