//! The original gem occupies a pinned atlas cell, independent of name churn.

use std::sync::LazyLock;

use super::*;

static GEM: LazyLock<(u32, u32, Arc<[u8]>)> = LazyLock::new(|| {
    let image = image::load_from_memory(ui_screens::player_list::CINNABAR_BADGE_PNG)
        .expect("embedded Cinnabar gem is a valid PNG")
        .into_rgba8();
    (image.width(), image.height(), image.into_raw().into())
});

impl NametagAtlas {
    /// Reuses one pinned gem cell without decoding, allocating or publishing on cache hits.
    pub(in crate::ui_runtime::presentation) fn badge(&mut self) -> Option<AtlasLine> {
        if let Some(cell) = self.cells.first().filter(|cell| cell.text.is_none()) {
            return Some(cell.line);
        }
        let (width, height, pixels) = &*GEM;
        let mut placement = self.allocate(*width, *height);
        if placement.is_none() && self.reclaim_shelf(Some([*width, *height])) {
            placement = self.allocate(*width, *height);
        }
        let Some((shelf, [x, y])) = placement else {
            self.rebuild_pending |= self.frame_active;
            return None;
        };
        let line = AtlasLine {
            cell: [x, y, *width, *height],
            width_px: *width as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
            top_px: 0.0,
            ink_y_px: [0.0, *height as f32 / FONT_DESIGN_PIXEL_TEXELS as f32],
        };
        // Index zero survives swap-removal of text cells, so its GPU upload stays reusable.
        for index in self.lines.values_mut() {
            *index += 1;
        }
        self.cells.insert(
            0,
            AtlasCell {
                text: None,
                line,
                rgba8: Arc::clone(pixels),
                shelf,
            },
        );
        self.reclamation = None;
        self.published = None;
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Warm calls preserve both the immutable pixels and upload revision without allocating.
    #[test]
    fn gem_is_retained_without_per_frame_allocation_or_upload() {
        let mut atlas = NametagAtlas::default();
        let cell = atlas.badge().unwrap();
        let (published, revision) = atlas.publish();
        let (_, allocations) = test_alloc::count(|| {
            assert_eq!(atlas.badge(), Some(cell));
            let (next, next_revision) = atlas.publish();
            assert!(Arc::ptr_eq(&published, &next));
            assert_eq!(revision, next_revision);
        });
        assert_eq!(allocations, 0);
        atlas.requested.insert("active".into());
        assert!(!atlas.reclaim_shelf(Some([NAMETAG_ATLAS_SIDE, NAMETAG_ATLAS_SIDE])));
        assert_eq!(atlas.badge(), Some(cell));
    }

    /// Evicting names preserves the pinned gem's position, pixels and upload identity.
    #[test]
    fn gem_survives_text_reclamation_without_upload() {
        let mut atlas = NametagAtlas::default();
        let gem = atlas.badge().unwrap();
        let width = NAMETAG_ATLAS_SIDE - gem.cell[2];
        let height = NAMETAG_ATLAS_SIDE;
        let (shelf, [x, y]) = atlas.allocate(width, height).unwrap();
        let text: Arc<str> = "unused".into();
        atlas.lines.insert(Arc::clone(&text), atlas.cells.len());
        atlas.cells.push(AtlasCell {
            text: Some(text),
            line: AtlasLine {
                cell: [x, y, width, height],
                width_px: width as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
                top_px: 0.0,
                ink_y_px: [0.0, height as f32 / FONT_DESIGN_PIXEL_TEXELS as f32],
            },
            rgba8: Arc::from([255; 4]),
            shelf,
        });
        let (previous, _) = atlas.publish();
        atlas.requested.insert("new name".into());
        assert!(atlas.reclaim_shelf(Some([width, height])));
        assert_eq!(atlas.badge(), Some(gem));
        let (current, _) = atlas.publish();
        assert!(
            NametagAtlasRect::updates(&current, &previous)
                .next()
                .is_none()
        );
    }
}
