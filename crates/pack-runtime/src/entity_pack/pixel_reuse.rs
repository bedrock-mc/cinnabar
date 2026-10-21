//! Recompiled catalogs reuse unchanged actor pixels from the preceding live catalog.
use assets::SessionEntityPack;
use std::{collections::BTreeMap, sync::Arc};

/// Preserves the new catalog's routes and dimensions while sharing equal raster content.
pub(super) fn reuse(
    previous: &SessionEntityPack,
    next: Arc<SessionEntityPack>,
) -> Arc<SessionEntityPack> {
    let by_digest: BTreeMap<_, _> = previous
        .textures
        .iter()
        .map(|texture| (texture.pixel_sha256, &texture.rgba8))
        .collect();
    let mut changed = false;
    let textures: Vec<_> = next
        .textures
        .iter()
        .map(|texture| {
            let mut texture = texture.clone();
            if let Some(pixels) = by_digest.get(&texture.pixel_sha256)
                && !Arc::ptr_eq(pixels, &texture.rgba8)
                && pixels.as_ref() == texture.rgba8.as_ref()
            {
                texture.rgba8 = Arc::clone(pixels);
                changed = true;
            }
            texture
        })
        .collect();
    if !changed {
        return next;
    }
    Arc::new(SessionEntityPack {
        assets: Arc::clone(&next.assets),
        textures: textures.into(),
        bindings: Arc::clone(&next.bindings),
        equipment: next.equipment.clone(),
    })
}
