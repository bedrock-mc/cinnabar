//! Equipped texture sources and visible gallery thumbnails share the bounded art worker.
use cosmetics::Item;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui_runtime::presentation) struct CosmeticArtwork {
    pub(super) item: Item,
    pub(super) thumbnail: bool,
}

/// Requests equipped textures and only the thumbnails exposed by the current gallery viewport.
pub(in crate::ui_runtime::presentation) fn requested(
    equipped: &[Item],
    visible: &[usize],
    gallery: bool,
) -> Vec<CosmeticArtwork> {
    let mut result: Vec<_> = equipped
        .iter()
        .take(cosmetics::SLOT_COUNT)
        .map(|&item| CosmeticArtwork {
            item,
            thumbnail: false,
        })
        .collect();
    if gallery {
        let items = Item::all();
        for &index in visible {
            if let Some(&item) = items.get(index) {
                let source = CosmeticArtwork {
                    item,
                    thumbnail: true,
                };
                if !result.contains(&source) {
                    result.push(source);
                }
            }
        }
    }
    result
}

impl super::UiPresentationRuntime {
    /// Starts required texture work when a selection changes even while the menu is hidden.
    pub(in crate::ui_runtime::presentation) fn sync_equipped_cosmetic_artwork(&mut self) {
        let mut set = self.menu_artwork_set.clone();
        set.cosmetics = requested(&self.menu_preview_model.cosmetics, &[], false);
        self.sync_artwork_set(set);
    }
}
