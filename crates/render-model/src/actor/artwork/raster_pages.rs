//! Immutable single-layer pages let persona arrivals replace only their own slots.
use super::page_budget::within_page_budget;
use super::*;

/// Adds the immutable source's exact pixel bytes to its content digest.
pub(super) fn hash_pixels(hasher: &mut Sha256, pixels: &[u8]) {
    #[cfg(test)]
    super::raster_page_tests::record_hashed_bytes(pixels.len());
    hasher.update(pixels);
}

impl ActorArtworkPages {
    /// Updates owned single-layer equipment pages at zero-based generic-page indices.
    /// An index at the end appends a page; existing entity and multilayer pages are protected.
    /// Invalid or over-budget rasters return no location and leave their slot unchanged.
    /// Call `with_equipment_rasters` when grouped, downscaled admission is required instead.
    #[must_use]
    pub fn with_equipment_raster_page_updates(
        mut self,
        updates: &[(usize, EquipmentRaster)],
    ) -> (Self, Vec<Option<ActorArtworkLocation>>) {
        let mut pages = self.pages.to_vec();
        let mut equipment = (*self.equipment).clone();
        let mut bytes = pages
            .iter()
            .fold(player_page_bytes(), |sum, page| sum + page.rgba8.len());
        let mut hasher = Sha256::new();
        hasher.update(self.identity);
        hasher.update(b"equipment-raster-page-updates-v1");
        let mut locations = Vec::with_capacity(updates.len());
        let mut changed = false;
        for (index, raster) in updates {
            let Some(page_id) = index
                .checked_add(1)
                .and_then(|id| ActorArtworkPageId::try_from(id).ok())
            else {
                locations.push(None);
                continue;
            };
            let replacement = pages.get(*index);
            let owned = replacement
                .is_none_or(|page| page.layers == 1 && equipment.contains(&(page_id, 0)));
            let next_bytes = bytes
                .saturating_sub(replacement.map_or(0, |page| page.rgba8.len()))
                .saturating_add(raster.rgba8.len());
            if *index > pages.len()
                || !owned
                || raster.width == 0
                || raster.height == 0
                || raster.rgba8.len() != usize::from(raster.width) * usize::from(raster.height) * 4
                || !within_page_budget(pages.len().max(index + 1), next_bytes)
            {
                locations.push(None);
                continue;
            }
            let page = ActorTexturePage {
                width: raster.width,
                height: raster.height,
                layers: 1,
                rgba8: Arc::clone(&raster.rgba8),
                color_mask: false,
                multitexture: false,
            };
            hasher.update(page_id.to_le_bytes());
            hasher.update(page.width.to_le_bytes());
            hasher.update(page.height.to_le_bytes());
            hash_pixels(&mut hasher, &page.rgba8);
            if *index == pages.len() {
                pages.push(page);
            } else {
                pages[*index] = page;
            }
            equipment.insert((page_id, 0));
            locations.push(Some(ActorArtworkLocation {
                page: page_id,
                layer: 0,
                pose_mode: assets::ActorPoseMode::CompiledLiteral,
                multitexture: None,
            }));
            bytes = next_bytes;
            changed = true;
        }
        if changed {
            self.identity = hasher.finalize().into();
            self.replace_pages(pages, &[]);
            self.equipment = Arc::new(equipment);
        }
        (self, locations)
    }
}
