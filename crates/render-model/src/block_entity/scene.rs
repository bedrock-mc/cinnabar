//! Submission types and the scene that turns them into per-frame vertex buffers.

use std::sync::Arc;

use assets::block_entity_geometry::heads::HeadModel;

#[path = "scene/cache.rs"]
mod cache;
use cache::{CachedSubmission, PreviousFragments};

use super::{
    artwork::{BlockEntityArtworkPreparer, PreparedBlockEntityArtwork},
    atlas::{AtlasRect, BlockEntityAtlas, DynamicCells},
    banner::BannerModel,
    beam::BeaconModel,
    bed::BedModel,
    bell::BellModel,
    chest::ChestModel,
    conduit::ConduitModel,
    crack::{CrackShape, emit_crack},
    crystal_beam::CrystalBeamModel,
    dragon_death::DragonDeathModel,
    frame::ItemFrameModel,
    heads::HeadModels,
    mesh::{BlockEntityVertex, MeshBuilder},
    pot::DecoratedPotModel,
    shulker::ShulkerModel,
    sign::SignModel,
    skull::SkullModel,
    statue::StatueModel,
};

/// What one block entity draws.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockEntityKind {
    Chest(ChestModel),
    Shulker(ShulkerModel),
    Skull(SkullModel),
    Banner(BannerModel),
    Bed(BedModel),
    Sign(SignModel),
    EnchantTable {
        book: super::book::BookPose,
    },
    Lectern {
        facing_yaw_degrees: f32,
        has_book: bool,
    },
    Bell(BellModel),
    ItemFrame(ItemFrameModel),
    Conduit(ConduitModel),
    DecoratedPot(DecoratedPotModel),
    Beacon(BeaconModel),
    CrystalBeam(CrystalBeamModel),
    DragonDeath(DragonDeathModel),
    Statue(StatueModel),
    EndPortal,
    EndGateway,
}

/// Blocks past its own block, on every side, that a block-anchored model may draw into.
pub const BLOCK_ENTITY_REACH_BLOCKS: i32 = 2;

impl BlockEntityKind {
    /// Whether it is a beam that may draw beyond [`BLOCK_ENTITY_REACH_BLOCKS`] of its block.
    #[must_use]
    pub const fn renders_globally(&self) -> bool {
        matches!(
            self,
            Self::Beacon(_) | Self::CrystalBeam(_) | Self::DragonDeath(_)
        )
    }

    /// Whether the authored model has a fixed block reach independent of installed geometry.
    pub const fn has_fixed_cull_reach(&self) -> bool {
        match self {
            Self::Skull(_)
            | Self::Bed(_)
            | Self::Statue(_)
            | Self::Beacon(_)
            | Self::CrystalBeam(_)
            | Self::DragonDeath(_) => false,
            Self::Chest(_)
            | Self::Shulker(_)
            | Self::Banner(_)
            | Self::Sign(_)
            | Self::EnchantTable { .. }
            | Self::Lectern { .. }
            | Self::Bell(_)
            | Self::ItemFrame(_)
            | Self::Conduit(_)
            | Self::DecoratedPot(_)
            | Self::EndPortal
            | Self::EndGateway => true,
        }
    }

    /// Whether its mesh animates with [`SceneClock`] rather than only with its model.
    const fn is_clock_driven(&self) -> bool {
        matches!(
            self,
            Self::Banner(_) | Self::EnchantTable { .. } | Self::Conduit(_) | Self::Beacon(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockEntitySubmission {
    pub block: [i32; 3],
    pub light: BlockEntityLight,
    pub kind: BlockEntityKind,
}

/// Scalar-lit legacy models or the native entity material's block/sky light coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockEntityLight {
    /// Existing authored face shading and a combined linear multiplier.
    Scalar(f32),
    /// Native entity material: retained levels into the shared environment lightmap.
    Actor { block: u8, sky: u8 },
}

impl From<f32> for BlockEntityLight {
    fn from(value: f32) -> Self {
        Self::Scalar(value)
    }
}

impl BlockEntityLight {
    fn apply(self, builder: &mut MeshBuilder) {
        (builder.light, builder.actor_light) = match self {
            Self::Scalar(value) => (value.clamp(0.0, 1.0), 0),
            Self::Actor { block, sky } => (1.0, crate::pack_actor_light(block, sky)),
        };
    }
}

/// A block with a break-crack overlay at destroy stage `stage` (`0..=9`).
#[derive(Clone, Debug, PartialEq)]
pub struct CrackInstance {
    pub block: [i32; 3],
    pub stage: u8,
    pub shape: CrackShape,
}

/// Animation time and the position texture scrolls are measured against.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneClock {
    /// Game ticks including the partial tick.
    pub ticks: f64,
}

/// Extracted per-frame draw data; textured models require an installed atlas.
#[derive(Clone, Debug, Default)]

pub struct BlockEntityFrame {
    pub revision: u64,
    pub atlas: Option<Arc<BlockEntityAtlasImage>>,
    pub dynamic_revision: u64,
    pub dynamic_rgba8: Arc<[u8]>,
    pub solid: Arc<[BlockEntityVertex]>,
    pub overlay: Arc<[BlockEntityVertex]>,
    pub crack: Arc<[BlockEntityVertex]>,
    pub portal: Arc<[BlockEntityVertex]>,
    pub additive: Arc<[BlockEntityVertex]>,
    /// Normalized star texture bounds; wrapping happens within this atlas placement.
    pub portal_star_rect: [f32; 4],
    pub portal_time_seconds: f32,
}

/// Static atlas pixels plus dimensions for GPU upload; a new size means a new texture.
#[derive(Debug)]
pub struct BlockEntityAtlasImage {
    pub identity: [u8; 32],
    pub size: [u32; 2],
    pub static_height: u32,
    pub static_rgba8: Arc<[u8]>,
}

impl BlockEntityAtlasImage {
    fn of(atlas: &BlockEntityAtlas) -> Self {
        Self {
            identity: atlas.identity(),
            size: atlas.size(),
            static_height: atlas.static_height(),
            static_rgba8: Arc::clone(atlas.static_rgba8()),
        }
    }
}

#[derive(Debug, Default)]
pub struct BlockEntityScene {
    atlas: Option<Arc<BlockEntityAtlas>>,
    image: Option<Arc<BlockEntityAtlasImage>>,
    prepared: Option<PreparedBlockEntityArtwork>,
    text: Option<DynamicCells>,
    frame: BlockEntityFrame,
    heads: HeadModels,
    bed: Option<HeadModel>,
    rejected_quads: u64,
    /// Inputs of the current frame when it holds no clock-driven kind; unchanged inputs reuse it.
    reusable: Option<(Vec<CrackInstance>, Vec<BlockEntitySubmission>)>,
    /// Static geometry in the last frame's submission order; see [`PreviousFragments`].
    cached_submissions: Vec<Option<Box<CachedSubmission>>>,
    /// An emptied fragment list kept so each rebuild reuses its allocation.
    spare_submissions: Vec<Option<Box<CachedSubmission>>>,
    /// Layer buffers kept between rebuilds; each frame copies them into its shared layers.
    mesh: Option<MeshBuilder>,
    #[cfg(test)]
    static_rebuilds: usize,
}

impl BlockEntityScene {
    /// Returns the artwork preparer for a join or reload worker.
    pub fn artwork_preparer(&self) -> BlockEntityArtworkPreparer {
        BlockEntityArtworkPreparer::new(&[]).with_heads(self.heads.clone())
    }

    /// Shares the installed static artwork as the startup fallback for pack workers.
    pub fn prepared_artwork(&self) -> Option<PreparedBlockEntityArtwork> {
        self.prepared.clone()
    }

    /// Borrows the static held models prepared with the accepted artwork, without frame copies.
    pub fn held_models(&self) -> Option<&super::held::PreparedHeldBlockModels> {
        self.prepared
            .as_ref()
            .map(PreparedBlockEntityArtwork::held_models)
    }

    /// Prepares and installs startup artwork; runtime workers use [`Self::install_prepared_assets`].
    pub fn install_assets(&mut self, assets: &assets::RuntimeBlockEntityAssets) {
        if let Some(prepared) = self.artwork_preparer().prepare(assets) {
            self.install_prepared_assets(&prepared);
        }
    }

    /// Installs worker-built artwork for a new session, clearing cached sign and map canvases.
    pub fn install_prepared_assets(&mut self, artwork: &PreparedBlockEntityArtwork) {
        if let Some(text) = self.text.as_mut() {
            text.reset(artwork.atlas.size()[0])
                .expect("prepared artwork admits dynamic atlas rows");
        }
        self.reload_prepared_assets(artwork);
    }

    /// Prepares artwork on the caller; runtime reloads use [`Self::reload_prepared_assets`].
    pub fn reload_assets(&mut self, assets: &assets::RuntimeBlockEntityAssets) {
        if let Some(prepared) = self.artwork_preparer().prepare(assets) {
            self.reload_prepared_assets(&prepared);
        }
    }

    /// Publishes shared static pixels and invalidates meshes while retaining dynamic canvases.
    pub fn reload_prepared_assets(&mut self, artwork: &PreparedBlockEntityArtwork) {
        self.prepared = Some(artwork.clone());
        let mut atlas = artwork.atlas.clone();
        let text = self.text.get_or_insert_with(|| {
            DynamicCells::new(atlas.size()[0]).expect("prepared artwork admits dynamic atlas rows")
        });
        text.resize_width(atlas.size()[0])
            .expect("prepared artwork admits dynamic atlas rows");
        atlas.set_text_pages(text.text_pages());
        self.image = Some(Arc::new(BlockEntityAtlasImage::of(&atlas)));
        self.atlas = Some(Arc::new(atlas));
        self.frame = BlockEntityFrame {
            revision: self.frame.revision,
            dynamic_revision: self.frame.dynamic_revision,
            dynamic_rgba8: Arc::clone(&self.frame.dynamic_rgba8),
            ..Default::default()
        };
        self.reusable = None;
        self.cached_submissions.clear();
    }

    /// Builds block models from the entity catalog's geometry.
    pub fn install_entity_assets(&mut self, assets: &assets::RuntimeEntityAssets) {
        self.heads = HeadModels::from_assets(assets);
        self.prepared = self
            .prepared
            .as_ref()
            .map(|prepared| prepared.with_heads(&self.heads));
        self.bed = assets
            .geometries()
            .iter()
            .find(|geometry| geometry.identifier.as_ref() == assets::BED_GEOMETRY_IDENTIFIER)
            .and_then(|geometry| HeadModel::build_tree(geometry, None, 1.0, false));
        self.reusable = None;
        self.cached_submissions.clear();
    }

    #[must_use]
    pub const fn has_assets(&self) -> bool {
        self.atlas.is_some()
    }

    #[must_use]
    pub fn atlas(&self) -> Option<&Arc<BlockEntityAtlas>> {
        self.atlas.as_ref()
    }

    #[must_use]
    pub const fn rejected_quads(&self) -> u64 {
        self.rejected_quads
    }

    /// The atlas rect of the text canvas for `key`, rasterizing `make` on a miss.
    pub fn text_rect(&mut self, key: u64, make: impl FnOnce() -> Vec<u8>) -> Option<AtlasRect> {
        let text = self.text.as_mut()?;
        let slot = text.text_slot(key, make)?;
        let pages = text.text_pages();
        let atlas = self.atlas.as_mut()?;
        if atlas.text_pages() != pages {
            // A taller atlas renormalizes every UV, so no cached geometry survives.
            Arc::make_mut(atlas).set_text_pages(pages);
            self.image = Some(Arc::new(BlockEntityAtlasImage::of(atlas)));
            self.reusable = None;
            self.cached_submissions.clear();
        }
        self.atlas.as_ref()?.text_cell(slot)
    }

    /// The atlas rect of the 128x128 map canvas for `key`, rasterizing `make` on a miss.
    pub fn map_rect(&mut self, key: u64, make: impl FnOnce() -> Vec<u8>) -> Option<AtlasRect> {
        let slot = self.text.as_mut()?.map_slot(key, make)?;
        self.atlas.as_ref()?.map_cell(slot)
    }

    pub fn update(
        &mut self,
        clock: SceneClock,
        cracks: &[CrackInstance],
        submissions: &[BlockEntitySubmission],
    ) -> &BlockEntityFrame {
        // This frame's text and map rects are all requested before its update.
        if let Some(text) = self.text.as_mut() {
            text.begin_frame();
        }
        let (Some(atlas), Some(text)) = (self.atlas.as_ref(), self.text.as_ref()) else {
            return self.update_untextured(submissions);
        };
        self.frame.portal_time_seconds =
            (clock.ticks / f64::from(simulation_clock::TICKS_PER_SECOND)) as f32;
        self.frame.portal_star_rect = super::portal::star_rect(atlas);
        // Rebuilding would emit the same vertices; keeping the revision spares the GPU upload.
        if self.frame.dynamic_revision == text.revision()
            && self
                .reusable
                .as_ref()
                .is_some_and(|(previous_cracks, previous)| {
                    previous_cracks.as_slice() == cracks && previous.as_slice() == submissions
                })
        {
            return &self.frame;
        }
        if submissions
            .iter()
            .any(|submission| submission.kind.is_clock_driven())
        {
            self.reusable = None;
        } else {
            let (previous_cracks, previous) = self.reusable.get_or_insert_default();
            previous_cracks.clear();
            previous_cracks.extend_from_slice(cracks);
            previous.clear();
            previous.extend_from_slice(submissions);
        }
        let builder = self
            .mesh
            .get_or_insert_with(|| MeshBuilder::new(atlas.size()));
        builder.reset(atlas.size());
        let mut previous_fragments = PreviousFragments::new(std::mem::replace(
            &mut self.cached_submissions,
            std::mem::take(&mut self.spare_submissions),
        ));
        for submission in submissions {
            let is_static = !submission.kind.is_clock_driven();
            if is_static
                && let Some(previous) = previous_fragments.take(submission)
                && previous.matches(submission, builder)
            {
                previous.append_to(builder);
                self.cached_submissions.push(Some(previous));
                continue;
            }
            let start = cache::vertex_counts(builder);
            let rejected_before = builder.rejected_quads;
            submission.light.apply(builder);
            emit_submission(
                builder,
                atlas,
                (&self.heads, self.bed.as_ref()),
                submission,
                clock,
            );
            if is_static {
                #[cfg(test)]
                {
                    self.static_rebuilds += 1;
                }
                self.cached_submissions
                    .push(Some(Box::new(CachedSubmission::capture(
                        submission,
                        start,
                        rejected_before,
                        builder,
                    ))));
            }
        }
        self.spare_submissions = previous_fragments.into_cleared_list();
        BlockEntityLight::Scalar(1.0).apply(builder);
        for crack in cracks {
            emit_crack(builder, atlas, crack);
        }
        self.rejected_quads = builder.rejected_quads;
        let dynamic_changed = self.frame.dynamic_revision != text.revision();
        self.frame = BlockEntityFrame {
            revision: self.frame.revision.wrapping_add(1),
            atlas: self.image.clone(),
            dynamic_revision: text.revision(),
            dynamic_rgba8: if dynamic_changed || self.frame.dynamic_rgba8.is_empty() {
                Arc::from(text.pixels())
            } else {
                Arc::clone(&self.frame.dynamic_rgba8)
            },
            solid: shared_layer(&self.frame.solid, &builder.solid),
            overlay: shared_layer(&self.frame.overlay, &builder.overlay),
            crack: shared_layer(&self.frame.crack, &builder.crack),
            portal: shared_layer(&self.frame.portal, &builder.portal),
            additive: shared_layer(&self.frame.additive, &builder.additive),
            portal_star_rect: self.frame.portal_star_rect,
            portal_time_seconds: self.frame.portal_time_seconds,
        };
        &self.frame
    }

    #[must_use]
    pub const fn frame(&self) -> &BlockEntityFrame {
        &self.frame
    }

    fn update_untextured(&mut self, submissions: &[BlockEntitySubmission]) -> &BlockEntityFrame {
        let mut builder = MeshBuilder::new([1; 2]);
        for submission in submissions {
            if let BlockEntityKind::DragonDeath(model) = &submission.kind {
                submission.light.apply(&mut builder);
                super::dragon_death::emit(&mut builder, model);
            }
        }
        if builder.additive.as_slice() != self.frame.additive.as_ref() {
            self.frame = BlockEntityFrame {
                revision: self.frame.revision.wrapping_add(1),
                additive: builder.additive.into(),
                ..Default::default()
            };
        }
        self.rejected_quads = builder.rejected_quads;
        &self.frame
    }
}

/// `previous` itself when it holds exactly the bytes of `vertices`, so consumers that compare
/// allocations skip an unchanged layer; otherwise a new shared copy of `vertices`.
fn shared_layer(
    previous: &Arc<[BlockEntityVertex]>,
    vertices: &[BlockEntityVertex],
) -> Arc<[BlockEntityVertex]> {
    if bytemuck::cast_slice::<_, u8>(previous) == bytemuck::cast_slice::<_, u8>(vertices) {
        Arc::clone(previous)
    } else {
        Arc::from(vertices)
    }
}

#[cfg(test)]
#[path = "scene/cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
#[path = "scene/skull_tests.rs"]
mod skull_tests;

#[cfg(test)]
#[path = "scene/reach_tests.rs"]
mod reach_tests;

fn emit_submission(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    (heads, bed): (&HeadModels, Option<&HeadModel>),
    submission: &BlockEntitySubmission,
    clock: SceneClock,
) {
    let block = submission.block;
    match &submission.kind {
        BlockEntityKind::Chest(model) => super::chest::emit(builder, atlas, block, model),
        BlockEntityKind::Shulker(model) => super::shulker::emit(builder, atlas, block, model),
        BlockEntityKind::Skull(model) => super::skull::emit(builder, atlas, heads, block, model),
        BlockEntityKind::Banner(model) => super::banner::emit(builder, atlas, block, model, clock),
        BlockEntityKind::Bed(model) => super::bed::emit(builder, atlas, block, model, bed),
        BlockEntityKind::Sign(model) => super::sign::emit(builder, block, model),
        BlockEntityKind::EnchantTable { book } => {
            super::book::emit_enchant_table(builder, atlas, block, *book);
        }
        BlockEntityKind::Lectern {
            facing_yaw_degrees,
            has_book,
        } => super::book::emit_lectern(builder, atlas, block, *facing_yaw_degrees, *has_book),
        BlockEntityKind::Bell(model) => super::bell::emit(builder, atlas, block, model),
        BlockEntityKind::ItemFrame(model) => super::frame::emit(builder, atlas, block, model),
        BlockEntityKind::Conduit(model) => {
            super::conduit::emit(builder, atlas, block, model, clock)
        }
        BlockEntityKind::DecoratedPot(model) => super::pot::emit(builder, atlas, block, model),
        BlockEntityKind::Beacon(model) => super::beam::emit(builder, atlas, block, model, clock),
        BlockEntityKind::CrystalBeam(model) => super::crystal_beam::emit(builder, atlas, model),
        BlockEntityKind::DragonDeath(model) => super::dragon_death::emit(builder, model),
        BlockEntityKind::Statue(model) => super::statue::emit(builder, atlas, heads, block, model),
        BlockEntityKind::EndPortal => super::portal::emit(builder, atlas, block, false, clock),
        BlockEntityKind::EndGateway => super::portal::emit(builder, atlas, block, true, clock),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_entity::{
        MIN_DYNAMIC_ATLAS_WIDTH,
        chest::{ChestModel, ChestPair, ChestVariant},
        mesh::Facing,
    };

    fn scene_with_chest_and_crack_textures() -> BlockEntityScene {
        let placement =
            |name: &str, x: u32, width: u32, height: u32| assets::BlockEntityPlacement {
                name: name.into(),
                x,
                y: 0,
                width,
                height,
            };
        let bytes = assets::encode_block_entity_catalog(
            b"{}",
            MIN_DYNAMIC_ATLAS_WIDTH,
            64,
            &vec![255u8; MIN_DYNAMIC_ATLAS_WIDTH as usize * 64 * 4],
            &[
                placement("textures/entity/chest/normal", 0, 64, 64),
                placement("textures/environment/destroy_stage_0", 64, 16, 16),
            ],
        )
        .unwrap();
        let mut scene = BlockEntityScene::default();
        scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap());
        scene
    }

    #[test]
    fn chest_and_crack_fill_the_solid_and_crack_lists() {
        let mut scene = scene_with_chest_and_crack_textures();
        let chest = BlockEntitySubmission {
            block: [1, 2, 3],
            light: 1.0.into(),
            kind: BlockEntityKind::Chest(ChestModel {
                variant: ChestVariant::Normal,
                facing: Facing::North,
                pair: ChestPair::Single,
                lid: 0.0,
            }),
        };
        let crack = CrackInstance {
            block: [1, 2, 3],
            stage: 0,
            shape: CrackShape::Cube,
        };
        let frame = scene.update(SceneClock::default(), &[crack], &[chest]);
        // Body, lid and latch boxes: three boxes of six two-triangle faces.
        assert_eq!(frame.solid.len(), 3 * 6 * 6);
        assert_eq!(frame.crack.len(), 6 * 6);
        assert!(frame.overlay.is_empty());
        assert_eq!(frame.revision, 1);
        assert!(frame.atlas.is_some());
    }

    /// Static block entities must not re-mesh and re-upload every frame; changes still rebuild.
    #[test]
    fn unchanged_static_submissions_keep_the_frame_revision() {
        let mut scene = scene_with_chest_and_crack_textures();
        let chest = |lid: f32| BlockEntitySubmission {
            block: [1, 2, 3],
            light: 1.0.into(),
            kind: BlockEntityKind::Chest(ChestModel {
                variant: ChestVariant::Normal,
                facing: Facing::North,
                pair: ChestPair::Single,
                lid,
            }),
        };
        let first = scene
            .update(SceneClock::default(), &[], &[chest(0.0)])
            .clone();
        for tick in 1..100 {
            let clock = SceneClock {
                ticks: f64::from(tick),
            };
            let frame = scene.update(clock, &[], &[chest(0.0)]);
            assert_eq!(frame.revision, first.revision);
            assert!(Arc::ptr_eq(&frame.solid, &first.solid));
        }
        assert_eq!(
            scene
                .update(SceneClock::default(), &[], &[chest(0.5)])
                .revision,
            first.revision + 1
        );
        let portal = BlockEntitySubmission {
            block: [0; 3],
            light: 1.0.into(),
            kind: BlockEntityKind::EndPortal,
        };
        let animated = scene
            .update(SceneClock::default(), &[], std::slice::from_ref(&portal))
            .revision;
        assert_eq!(
            scene
                .update(SceneClock { ticks: 1.0 }, &[], &[portal])
                .revision,
            animated,
            "portal animation updates the shader clock without rebuilding geometry"
        );
        assert_eq!(
            scene.frame.portal_time_seconds,
            1.0 / simulation_clock::TICKS_PER_SECOND as f32
        );
    }

    #[test]
    fn missing_textures_skip_a_model_without_failing_the_frame() {
        let mut scene = scene_with_chest_and_crack_textures();
        let frame = scene.update(
            SceneClock::default(),
            &[CrackInstance {
                block: [0; 3],
                stage: 7,
                shape: CrackShape::Cube,
            }],
            &[],
        );
        assert!(frame.crack.is_empty());
    }

    #[test]
    fn update_without_assets_keeps_the_empty_frame() {
        let mut scene = BlockEntityScene::default();
        let frame = scene.update(
            SceneClock::default(),
            &[CrackInstance {
                block: [0; 3],
                stage: 0,
                shape: CrackShape::Cube,
            }],
            &[],
        );
        assert_eq!(frame.revision, 0);
        assert!(
            frame.solid.is_empty()
                && frame.overlay.is_empty()
                && frame.crack.is_empty()
                && frame.portal.is_empty()
        );
        assert!(!scene.has_assets());
    }
}
