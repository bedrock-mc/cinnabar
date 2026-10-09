//! HUD screens a pack worker resolves against a server catalog before its first frame, so the
//! frame that installs the catalog binds them instead of resolving them.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, Weak},
};

use json_ui::{CROSSHAIR_SCREEN, Catalog, Context, HUD_SCREEN, ResolvedControl, hud_context};

use super::super::server_pack::PrereadTextures;

/// One screen resolved for a catalog and context.
struct Prepared {
    catalog: Weak<Catalog>,
    reference: &'static str,
    context: Context,
    tree: Option<Arc<ResolvedControl>>,
}

/// Resolutions kept for live catalogs; one session prepares four.
const MAX_PREPARED: usize = 64;

static PREPARED: Mutex<Vec<Prepared>> = Mutex::new(Vec::new());

/// Resolves the HUD and crosshair screens under both chat placements on the calling thread, and
/// returns the textures their controls name.
pub(in crate::ui_runtime::presentation::forms) fn prepare(
    catalog: &Arc<Catalog>,
) -> BTreeSet<String> {
    let base = hud_context(&super::super::menu_screens::retail_context());
    let mut resolved = Vec::new();
    let mut textures = BTreeSet::new();
    for top in [false, true] {
        let context = super::super::chat_position::placed(base.clone(), top);
        for reference in [HUD_SCREEN, CROSSHAIR_SCREEN] {
            let tree = json_ui::resolve(catalog, reference, &context)
                .control
                .map(Arc::new);
            if let Some(tree) = &tree {
                named_textures(tree, &mut textures);
            }
            resolved.push(Prepared {
                catalog: Arc::downgrade(catalog),
                reference,
                tree,
                context: context.clone(),
            });
        }
    }
    let mut prepared = PREPARED.lock().unwrap_or_else(|poison| poison.into_inner());
    prepared.retain(|entry| entry.catalog.strong_count() > 0);
    prepared.extend(resolved);
    let excess = prepared.len().saturating_sub(MAX_PREPARED);
    prepared.drain(..excess);
    textures
}

/// Adds the literal texture paths `control` and its descendants name; bound ones resolve later.
fn named_textures(control: &ResolvedControl, textures: &mut BTreeSet<String>) {
    if let Some(path) = control
        .properties
        .get("texture")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty() && !path.starts_with(['#', '$']))
    {
        textures.insert(path.to_owned());
    }
    for child in &control.children {
        named_textures(child, textures);
    }
}

/// Pack textures a worker read for a catalog's prepared HUD.
struct Textures {
    catalog: Weak<Catalog>,
    textures: Arc<PrereadTextures>,
}

static TEXTURES: Mutex<Vec<Textures>> = Mutex::new(Vec::new());

/// Keeps `textures`, read for `catalog`'s prepared HUD, until the frame installing it takes them.
pub(in crate::ui_runtime::presentation::forms) fn keep_textures(
    catalog: &Arc<Catalog>,
    textures: PrereadTextures,
) {
    let mut kept = TEXTURES.lock().unwrap_or_else(|poison| poison.into_inner());
    kept.retain(|entry| entry.catalog.strong_count() > 0);
    kept.push(Textures {
        catalog: Arc::downgrade(catalog),
        textures: Arc::new(textures),
    });
    let excess = kept.len().saturating_sub(MAX_PREPARED);
    kept.drain(..excess);
}

/// Takes the pack textures a worker read for `catalog`'s HUD.
pub(in crate::ui_runtime::presentation::forms) fn take_textures(
    catalog: &Arc<Catalog>,
) -> Option<Arc<PrereadTextures>> {
    let mut kept = TEXTURES.lock().unwrap_or_else(|poison| poison.into_inner());
    let index = kept.iter().position(|entry| {
        entry
            .catalog
            .upgrade()
            .is_some_and(|kept| Arc::ptr_eq(&kept, catalog))
    })?;
    Some(kept.swap_remove(index).textures)
}

/// Takes `reference` resolved for `catalog` under `context`, when a worker prepared it, so the
/// frame owns the tree alone.
pub(super) fn find(
    catalog: &Arc<Catalog>,
    reference: &str,
    context: &Context,
) -> Option<Option<Arc<ResolvedControl>>> {
    let mut prepared = PREPARED.lock().unwrap_or_else(|poison| poison.into_inner());
    let index = prepared.iter().position(|entry| {
        entry.reference == reference
            && entry
                .catalog
                .upgrade()
                .is_some_and(|prepared| Arc::ptr_eq(&prepared, catalog))
            && entry.context == *context
    })?;
    Some(prepared.swap_remove(index).tree)
}
