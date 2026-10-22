//! Immutable static artwork prepared before publication to the frame scene.

use std::{collections::BTreeSet, sync::Arc};

use super::atlas::{AtlasRect, BlockEntityAtlas, DynamicCells, MAX_TEXT_PAGES, dynamic_height};

/// wgpu's default 2D texture side, which every adapter must support.
pub(super) const MAX_ATLAS_SIDE: u32 = 8192;

/// A static texture appended to the block-entity atlas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StaticTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

/// Static textures retained by join and reload workers.
#[derive(Clone, Debug, Default)]
pub struct BlockEntityArtworkPreparer {
    textures: Arc<[StaticTexture]>,
    heads: super::heads::HeadModels,
    chalkboards: Arc<super::chalkboard::ChalkboardModels>,
}

/// Appended pixels, placements and identity; publication shares the pixel allocation.
#[derive(Clone, Debug)]
pub struct PreparedBlockEntityArtwork {
    pub(super) atlas: BlockEntityAtlas,
    pub(super) chalkboards: Arc<super::chalkboard::ChalkboardModels>,
    held: Arc<super::held::PreparedHeldBlockModels>,
}

impl assets::compiled::RetainedBuffers for PreparedBlockEntityArtwork {
    fn visit_buffers(&self, visit: &mut assets::compiled::BufferVisitor<'_>) {
        // Chalkboard and held models are small beside the shared atlas pixels.
        let Self {
            atlas,
            chalkboards: _,
            held: _,
        } = self;
        assets::compiled::RetainedBuffers::visit_buffers(atlas, visit);
    }
}

impl PreparedBlockEntityArtwork {
    /// Shares held geometry prepared from these exact static pixels and placements.
    pub fn held_models(&self) -> &super::held::PreparedHeldBlockModels {
        &self.held
    }

    /// Returns the immutable atlas built on the preparation worker.
    pub fn atlas(&self) -> &BlockEntityAtlas {
        &self.atlas
    }

    /// Refreshes held head geometry while sharing the already prepared static pixels.
    pub(super) fn with_heads(&self, heads: &super::heads::HeadModels) -> Self {
        Self {
            atlas: self.atlas.clone(),
            chalkboards: self.chalkboards.clone(),
            held: Arc::new(super::held::PreparedHeldBlockModels::with_heads(
                &self.atlas,
                heads,
            )),
        }
    }
}

impl BlockEntityArtworkPreparer {
    /// Snapshots immutable static textures without copying their pixels.
    pub(super) fn new(textures: &[StaticTexture]) -> Self {
        Self {
            textures: textures.into(),
            heads: Default::default(),
            chalkboards: Default::default(),
        }
    }

    /// Snapshots placed-head geometry before preparation moves onto a worker.
    pub(super) fn with_heads(mut self, heads: super::heads::HeadModels) -> Self {
        self.heads = heads;
        self
    }

    /// Snapshots startup board geometry for complete pack replacement and removal.
    pub(super) fn with_chalkboards(
        mut self,
        models: Arc<super::chalkboard::ChalkboardModels>,
    ) -> Self {
        self.chalkboards = models;
        self
    }

    /// Publishes active board models over the startup fallback with these exact pixels.
    pub fn with_chalkboard_assets(
        &self,
        artwork: &PreparedBlockEntityArtwork,
        assets: Option<&assets::RuntimeEntityAssets>,
    ) -> PreparedBlockEntityArtwork {
        let mut prepared = artwork.clone();
        prepared.chalkboards = assets.map_or_else(
            || self.chalkboards.clone(),
            |assets| Arc::new(self.chalkboards.overlay(assets)),
        );
        prepared
    }

    /// Admits carrier dimensions only when static textures, maps and every text page also fit.
    pub fn accepts_size(&self, [width, height]: [u32; 2]) -> bool {
        let limit = MAX_ATLAS_SIDE;
        DynamicCells::accepts_width(width)
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
        let held = Arc::new(super::held::PreparedHeldBlockModels::with_heads(
            &atlas,
            &self.heads,
        ));
        Some(PreparedBlockEntityArtwork {
            atlas,
            held,
            chalkboards: self.chalkboards.clone(),
        })
    }
}

/// Computes relative texture shelves without allocating or hashing any atlas pixels.
/// Invalid textures and duplicate names are skipped, matching the append operation.
pub(super) fn appended_layout(
    width: u32,
    textures: &[StaticTexture],
    contains: impl Fn(&str) -> bool,
) -> Option<(u32, Vec<(&StaticTexture, AtlasRect)>)> {
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
    fn carrier_admission_requires_the_dynamic_row_width_before_preparation() {
        let preparer = BlockEntityArtworkPreparer::default();
        let minimum = super::super::atlas::MIN_DYNAMIC_ATLAS_WIDTH;
        for width in [1, minimum - 1, minimum] {
            let blob = assets::encode_block_entity_catalog(
                b"{}",
                width,
                1,
                &vec![0; width as usize * 4],
                &[],
            )
            .unwrap();
            let carrier = assets::RuntimeBlockEntityAssets::decode(&blob).unwrap();
            assert_eq!(preparer.accepts_size([width, 1]), width == minimum);
            assert_eq!(preparer.prepare(&carrier).is_some(), width == minimum);
        }
    }

    #[test]
    fn admission_counts_mob_shelves_and_gutters() {
        let width = super::super::atlas::MIN_DYNAMIC_ATLAS_WIDTH;
        let textures: Vec<_> = (0..3)
            .map(|index| StaticTexture {
                name: format!("mob/{index}"),
                width: width / 2,
                height: 64,
                rgba8: Arc::from(vec![7; (width / 2 * 64 * 4) as usize]),
            })
            .collect();
        let preparer = BlockEntityArtworkPreparer::new(&textures);
        let appended = 3 * 64 + 2;
        let limit = MAX_ATLAS_SIDE;
        let carrier_height = limit - dynamic_height(MAX_TEXT_PAGES as u32) - appended;
        assert!(preparer.accepts_size([width, carrier_height]));
        assert!(!preparer.accepts_size([width, carrier_height + 1]));
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            width,
            carrier_height,
            &vec![255; (width * carrier_height * 4) as usize],
            &[],
        )
        .unwrap();
        let assets = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
        let prepared = preparer.prepare(&assets).unwrap();
        assert_eq!(prepared.atlas().static_height(), carrier_height + appended);
        assert_eq!(
            prepared
                .atlas()
                .texture("mob/2", [(width / 2) as f32, 64.0])
                .unwrap()
                .rect
                .y,
            (carrier_height + 2 * 65) as f32
        );
        assert_eq!(prepared.atlas().texel("mob/2", 0, 0), Some([7; 4]));
    }
}
