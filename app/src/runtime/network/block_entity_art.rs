//! Worker-built block-entity artwork over the immutable startup carrier.

use assets::RuntimeBlockEntityAssets;
use render_model::{BlockEntityArtworkPreparer, BlockEntityScene, PreparedBlockEntityArtwork};
use resource_pack::LayeredPackView;
use std::sync::{Arc, OnceLock};

struct BaseArtwork {
    assets: Arc<RuntimeBlockEntityAssets>,
    preparer: BlockEntityArtworkPreparer,
    prepared: Arc<PreparedBlockEntityArtwork>,
}

static BASE: OnceLock<BaseArtwork> = OnceLock::new();

/// Retains the startup carrier and appended artwork so every stack uses the same base.
pub(crate) fn install_base(base: Option<Arc<RuntimeBlockEntityAssets>>, scene: &BlockEntityScene) {
    if let Some((assets, prepared)) = base.zip(scene.prepared_artwork()) {
        let _ = BASE.set(BaseArtwork {
            assets,
            preparer: scene.artwork_preparer(),
            prepared: Arc::new(prepared),
        });
    }
}

/// Returns the already prepared startup fallback used by join and reload workers.
pub(super) fn base() -> Option<&'static Arc<PreparedBlockEntityArtwork>> {
    BASE.get().map(|base| &base.prepared)
}

/// Resolves carrier tiles, appends mobs and hashes final static pixels on the worker.
pub(super) fn prepare(view: &LayeredPackView) -> Option<Arc<PreparedBlockEntityArtwork>> {
    let base = BASE.get()?;
    Some(
        prepare_over(view, &base.assets, &base.preparer)
            .map_or_else(|| base.prepared.clone(), Arc::new),
    )
}

/// Rebuilds one complete static atlas; overrides that exceed the final budget retain their tile.
pub(super) fn prepare_over(
    view: &LayeredPackView,
    base: &RuntimeBlockEntityAssets,
    preparer: &BlockEntityArtworkPreparer,
) -> Option<PreparedBlockEntityArtwork> {
    let assets = pack_entity_compiler::overlay_block_entity_assets(base, |size| preparer.accepts_size(size), |name| {
        pack_runtime::pack_textures::decode_pack_texture(view, name).map(|texture| {
            pack_entity_compiler::BlockEntityTexture {
                width: texture.width, height: texture.height, rgba8: texture.rgba8,
            }
        })
    }).map_err(|error| bevy::log::warn!(%error, "block-entity artwork retained after invalid atlas overlay")).ok()?;
    preparer.prepare(&assets)
}

/// Pairs the active catalog's board models with worker-built pixels, restoring startup on removal.
pub(super) fn with_entities(
    artwork: Option<Arc<PreparedBlockEntityArtwork>>,
    entities: Option<&assets::SessionEntityPack>,
    changes: &pack_runtime::reload_diff::Changes,
) -> Option<Arc<PreparedBlockEntityArtwork>> {
    let Some(base) = BASE.get() else {
        return artwork;
    };
    base.with_entities(artwork, entities, changes)
}

impl BaseArtwork {
    /// Reuses the published pair until either its pixels or its model catalog changes.
    fn with_entities(
        &self,
        artwork: Option<Arc<PreparedBlockEntityArtwork>>,
        entities: Option<&assets::SessionEntityPack>,
        changes: &pack_runtime::reload_diff::Changes,
    ) -> Option<Arc<PreparedBlockEntityArtwork>> {
        if !changes.block_entity_art && !changes.entities {
            return artwork;
        }
        artwork.map(|artwork| {
            Arc::new(
                self.preparer
                    .with_chalkboard_assets(&artwork, entities.map(|pack| pack.assets.as_ref())),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pack_runtime::reload_diff::Changes;

    /// Builds an independent startup snapshot using only original solid-colour pixels.
    fn base_artwork() -> BaseArtwork {
        let width = render_model::block_entity::MIN_DYNAMIC_ATLAS_WIDTH;
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            width,
            1,
            &vec![255; width as usize * 4],
            &[],
        )
        .unwrap();
        let assets = Arc::new(RuntimeBlockEntityAssets::decode(&bytes).unwrap());
        let preparer = BlockEntityArtworkPreparer::default();
        let prepared = Arc::new(preparer.prepare(&assets).unwrap());
        BaseArtwork {
            assets,
            preparer,
            prepared,
        }
    }

    #[test]
    fn chalkboard_pairing_preserves_artwork_for_unchanged_subscribers() {
        let base = base_artwork();
        let previous = base
            .with_entities(Some(base.prepared.clone()), None, &Changes::all())
            .unwrap();
        let mut changes = Changes::all();
        changes.block_entity_art = false;
        changes.entities = false;
        for contents in [false, true] {
            changes.contents = contents;
            let paired = base
                .with_entities(Some(previous.clone()), None, &changes)
                .unwrap();
            assert!(Arc::ptr_eq(&previous, &paired));
        }
    }

    #[test]
    fn chalkboard_pairing_replaces_artwork_when_either_subscriber_changes() {
        let base = base_artwork();
        for (pixels, models) in [(true, false), (false, true), (true, true)] {
            let mut changes = Changes::all();
            changes.block_entity_art = pixels;
            changes.entities = models;
            let paired = base
                .with_entities(Some(base.prepared.clone()), None, &changes)
                .unwrap();
            assert!(!Arc::ptr_eq(&base.prepared, &paired));
        }
    }
}
