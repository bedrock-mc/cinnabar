//! HUD screens a pack worker resolves against a server catalog before its first frame, so the
//! frame that installs the catalog binds them instead of resolving them.

use std::{collections::BTreeSet, sync::Arc};

use json_ui::{CROSSHAIR_SCREEN, Catalog, Context, HUD_SCREEN, ResolvedControl};

/// Resolves the HUD and crosshair screens under each of `placements`, the HUD contexts of every
/// chat placement, on the calling thread, and returns the textures their controls name.
pub fn prepare(
    catalog: &Arc<Catalog>,
    placements: impl IntoIterator<Item = Context>,
) -> BTreeSet<String> {
    let mut resolved = Vec::new();
    let mut textures = BTreeSet::new();
    for context in placements {
        for reference in [HUD_SCREEN, CROSSHAIR_SCREEN] {
            let tree = json_ui::resolve(catalog, reference, &context)
                .control
                .map(Arc::new);
            if let Some(tree) = &tree {
                named_textures(tree, &mut textures);
            }
            resolved.push((reference, context.clone(), tree));
        }
    }
    ui_engine::prepared::keep(catalog, resolved);
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
