//! Immutable static artwork prepared before publication to the frame scene.

use std::{collections::BTreeSet, sync::Arc};

use super::{
    atlas::{AtlasRect, BlockEntityAtlas, MAX_TEXT_PAGES, dynamic_height},
    mob::MobTexture,
};

/// wgpu's default 2D texture side, which every adapter must support.
pub(super) const MAX_ATLAS_SIDE: u32 = 8192;

/// Startup mob textures retained by join and reload workers.
#[derive(Clone, Debug, Default)]
pub struct BlockEntityArtworkPreparer {
    textures: Arc<[MobTexture]>,
}

/// Appended pixels, placements and identity; publication shares the pixel allocation.
#[derive(Clone, Debug)]
pub struct PreparedBlockEntityArtwork {
    pub(super) atlas: BlockEntityAtlas,
}

impl PreparedBlockEntityArtwork {
    /// Returns the immutable atlas built on the preparation worker.
    pub fn atlas(&self) -> &BlockEntityAtlas {
        &self.atlas
    }
}

impl BlockEntityArtworkPreparer {
    /// Snapshots immutable mob textures without copying their pixels.
    pub(super) fn new(textures: &[MobTexture]) -> Self {
        Self {
            textures: textures.into(),
        }
    }

    /// Admits carrier dimensions only when mobs, maps and every text page also fit.
    pub fn accepts_size(&self, [width, height]: [u32; 2]) -> bool {
        let limit = MAX_ATLAS_SIDE;
        width > 0
            && width <= limit
            && appended_layout(width, &self.textures, |_| false)
                .and_then(|(added, _)| height.checked_add(added))
                .and_then(|height| height.checked_add(dynamic_height(MAX_TEXT_PAGES as u32)))
                .is_some_and(|height| height <= limit)
    }

    /// Copies appended pixels and computes their identity on the calling worker.
    /// Oversized carriers are rejected before allocating appended pixels.
    pub fn prepare(
        &self,
        assets: &assets::RuntimeBlockEntityAssets,
    ) -> Option<PreparedBlockEntityArtwork> {
        if !self.accepts_size(assets.atlas_size()) {
            return None;
        }
        let mut atlas = BlockEntityAtlas::from_assets(assets);
        atlas.append_textures(&self.textures);
        Some(PreparedBlockEntityArtwork { atlas })
    }
}

/// Computes relative mob shelves without allocating or hashing any atlas pixels.
/// Invalid textures and duplicate names are skipped, matching the append operation.
pub(super) fn appended_layout(
    width: u32,
    textures: &[MobTexture],
    contains: impl Fn(&str) -> bool,
) -> Option<(u32, Vec<(&MobTexture, AtlasRect)>)> {
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    let mut placements = Vec::new();
    let mut names = BTreeSet::new();
    for texture in textures {
        let expected = (texture.width as usize)
            .checked_mul(texture.height as usize)?
            .checked_mul(4)?;
        if texture.width == 0
            || texture.width > width
            || texture.height == 0
            || texture.rgba8.len() != expected
            || contains(&texture.name)
            || !names.insert(texture.name.as_str())
        {
            continue;
        }
        if x.checked_add(texture.width)? > width {
            x = 0;
            y = y.checked_add(shelf)?.checked_add(1)?;
            shelf = 0;
        }
        placements.push((
            texture,
            AtlasRect {
                x: x as f32,
                y: y as f32,
                width: texture.width as f32,
                height: texture.height as f32,
            },
        ));
        x = x.checked_add(texture.width)?.checked_add(1)?;
        shelf = shelf.max(texture.height);
    }
    Some((y.checked_add(shelf)?, placements))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_counts_mob_shelves_and_gutters() {
        let textures: Vec<_> = (0..3)
            .map(|index| MobTexture {
                name: format!("mob/{index}"),
                width: 512,
                height: 64,
                rgba8: Arc::from(vec![7; 512 * 64 * 4]),
            })
            .collect();
        let preparer = BlockEntityArtworkPreparer::new(&textures);
        let appended = 3 * 64 + 2;
        let limit = MAX_ATLAS_SIDE;
        let carrier_height = limit - dynamic_height(MAX_TEXT_PAGES as u32) - appended;
        assert!(preparer.accepts_size([1024, carrier_height]));
        assert!(!preparer.accepts_size([1024, carrier_height + 1]));
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            1024,
            carrier_height,
            &vec![255; (1024 * carrier_height * 4) as usize],
            &[],
        )
        .unwrap();
        let assets = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
        let prepared = preparer.prepare(&assets).unwrap();
        assert_eq!(prepared.atlas().static_height(), carrier_height + appended);
        assert_eq!(
            prepared
                .atlas()
                .texture("mob/2", [512.0, 64.0])
                .unwrap()
                .rect
                .y,
            (carrier_height + 2 * 65) as f32
        );
        assert_eq!(prepared.atlas().texel("mob/2", 0, 0), Some([7; 4]));
    }
}
