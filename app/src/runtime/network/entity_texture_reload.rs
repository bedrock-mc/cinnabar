//! Base entity rasters can be overridden without restating vanilla entity definitions.

use assets::RuntimeEntityAssets;
use render::ActorArtworkPages;
use render_model::equipment::EquipmentRaster;
use resource_pack::LayeredPackView;
use std::sync::{Arc, OnceLock};

static BASE: OnceLock<(ActorArtworkPages, Arc<RuntimeEntityAssets>)> = OnceLock::new();

/// Retains immutable carrier artwork as the fallback for optional texture-only packs.
pub(crate) fn set_base_actor_artwork(pages: ActorArtworkPages, entities: Arc<RuntimeEntityAssets>) {
    let _ = BASE.set((pages, entities));
}

/// Decodes winning entity source paths on the worker and restores the base after removal.
pub(super) fn prepare(view: &LayeredPackView) -> Option<Arc<ActorArtworkPages>> {
    let (pages, entities) = BASE.get()?;
    let mut bytes = 0usize;
    let overrides = entities
        .sources()
        .iter()
        .enumerate()
        .filter_map(|(index, source)| {
            if !source.path.starts_with("textures/") {
                return None;
            }
            let image = super::resource_packs::decode_pack_texture(view, &source.path)?;
            let width = u16::try_from(image.width).ok()?;
            let height = u16::try_from(image.height).ok()?;
            if !retain_override_bytes(&mut bytes, image.rgba8.len(), assets::MAX_ACTOR_PIXEL_BYTES)
            {
                return None;
            }
            Some((
                index as u32,
                EquipmentRaster {
                    width,
                    height,
                    rgba8: image.rgba8.into(),
                },
            ))
        })
        .collect::<Vec<_>>();
    Some(Arc::new(
        pages.clone().with_source_texture_overrides(&overrides),
    ))
}

/// Accounts for source overrides against the retained artwork budget.
fn retain_override_bytes(retained: &mut usize, bytes: usize, budget: usize) -> bool {
    let Some(total) = retained.checked_add(bytes).filter(|total| *total <= budget) else {
        return false;
    };
    *retained = total;
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn review_rejected_texture_bytes_do_not_suppress_later_overrides() {
        let mut retained = 0;
        assert!(super::retain_override_bytes(&mut retained, 24, 32));
        assert!(!super::retain_override_bytes(&mut retained, 12, 32));
        assert!(super::retain_override_bytes(&mut retained, 4, 32));
        assert_eq!(retained, 28);
    }
}
