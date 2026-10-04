//! Pack workers prepare immutable artwork; publication accepts only the exact source snapshot.

use std::sync::Arc;

use render::ActorArtworkPages;

use assets::SessionEntityPack;

/// Packed pages retain their source snapshots so a concurrent reload cannot install stale routes.
#[derive(Debug)]
pub struct PreparedActorArtwork {
    base: ActorArtworkPages,
    pack: Arc<SessionEntityPack>,
    pages: ActorArtworkPages,
}

impl PreparedActorArtwork {
    /// Packs the same pages as synchronous publication, before the main frame needs them.
    pub fn new(base: &ActorArtworkPages, pack: &Arc<SessionEntityPack>) -> Self {
        Self {
            base: base.clone(),
            pack: pack.clone(),
            pages: base
                .clone()
                .with_pack_artwork(&pack.textures, &pack.bindings),
        }
    }

    /// Returns shared pages only while both immutable inputs are still the same snapshots.
    pub fn pages_for(
        &self,
        base: &ActorArtworkPages,
        pack: &Arc<SessionEntityPack>,
    ) -> Option<ActorArtworkPages> {
        (Arc::ptr_eq(&self.pack, pack) && self.base.shares_storage_with(base))
            .then(|| self.pages.clone())
    }
}

/// Uses prepared pages when current; stale or missing preparation retains the existing packing path.
pub(super) fn session_pages(
    base: &ActorArtworkPages,
    pack: Option<&Arc<SessionEntityPack>>,
    prepared: Option<&PreparedActorArtwork>,
) -> ActorArtworkPages {
    let Some(pack) = pack else {
        return base.clone();
    };
    prepared
        .and_then(|prepared| prepared.pages_for(base, pack))
        .unwrap_or_else(|| {
            base.clone()
                .with_pack_artwork(&pack.textures, &pack.bindings)
        })
}
