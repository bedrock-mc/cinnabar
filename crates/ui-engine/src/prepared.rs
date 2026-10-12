//! Screens and textures a pack worker resolves against a server catalog before its first frame,
//! so the frame that installs the catalog binds them instead of resolving them.

use std::sync::{Arc, Mutex, Weak};

use json_ui::{Catalog, Context, ResolvedControl};

use crate::atlas::preread::PrereadTextures;

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

/// A screen a worker resolved: its reference, the context it resolved under, and its tree.
pub type Resolved = (&'static str, Context, Option<Arc<ResolvedControl>>);

/// Keeps each screen resolved for `catalog` until a frame takes it.
pub fn keep(catalog: &Arc<Catalog>, resolved: impl IntoIterator<Item = Resolved>) {
    let resolved: Vec<_> = resolved
        .into_iter()
        .map(|(reference, context, tree)| Prepared {
            catalog: Arc::downgrade(catalog),
            reference,
            tree,
            context,
        })
        .collect();
    let mut prepared = PREPARED.lock().unwrap_or_else(|poison| poison.into_inner());
    prepared.retain(|entry| entry.catalog.strong_count() > 0);
    prepared.extend(resolved);
    let excess = prepared.len().saturating_sub(MAX_PREPARED);
    prepared.drain(..excess);
}

/// Pack textures a worker read for a catalog's prepared HUD.
struct Textures {
    catalog: Weak<Catalog>,
    textures: Arc<PrereadTextures>,
}

static TEXTURES: Mutex<Vec<Textures>> = Mutex::new(Vec::new());

/// Keeps `textures`, read for `catalog`'s prepared HUD, until the frame installing it takes them.
pub fn keep_textures(catalog: &Arc<Catalog>, textures: PrereadTextures) {
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
pub fn take_textures(catalog: &Arc<Catalog>) -> Option<Arc<PrereadTextures>> {
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
pub(crate) fn find(
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
