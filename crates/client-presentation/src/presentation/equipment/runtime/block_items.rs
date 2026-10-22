//! Shared startup and worker preparation for held block artwork and models.

use super::*;

/// Immutable startup inputs used to rebuild held blocks away from the frame thread.
#[derive(Clone)]
pub struct BlockItemPreparer {
    world: Arc<RuntimeAssets>,
    entities: Arc<RuntimeEntityAssets>,
    icons: Arc<RuntimeIconCatalog>,
}

/// A complete held-block generation, including its geometry, pixels and alpha policy.
#[derive(Clone)]
pub struct PreparedBlockItems {
    world: Option<Arc<RuntimeAssets>>,
    pub(super) sheets: BTreeMap<u32, usize>,
    pub(super) models: BTreeMap<u32, Vec<render_model::ActorRigVertex>>,
    pub(super) alpha: BTreeMap<u32, render_model::HandItemAlphaMode>,
    pub(super) placements: Vec<Option<Placement>>,
    pub(crate) rasters: Vec<EquipmentRaster>,
}

impl assets::compiled::RetainedBuffers for PreparedBlockItems {
    fn visit_buffers(&self, visit: &mut assets::compiled::BufferVisitor<'_>) {
        // The terrain carrier is the session's world, owned and counted elsewhere.
        let Self {
            world: _,
            sheets: _,
            models,
            alpha: _,
            placements: _,
            rasters,
        } = self;
        for model in models.values() {
            assets::compiled::visit_slice(model, visit);
        }
        assets::compiled::RetainedBuffers::visit_buffers(rasters, visit);
    }
}

impl std::fmt::Debug for PreparedBlockItems {
    /// Summarizes prepared routes without printing the retained terrain carrier.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedBlockItems")
            .field("sheets", &self.sheets)
            .finish_non_exhaustive()
    }
}

impl BlockItemPreparer {
    /// Rebuilds terrain-derived resources; the startup carrier uses its existing resources.
    pub fn prepare(&self, world: &Arc<RuntimeAssets>) -> Option<Arc<PreparedBlockItems>> {
        if Arc::ptr_eq(&self.world, world) {
            return None;
        }
        Some(PreparedBlockItems::build(
            Some(Arc::clone(world)),
            &self.entities,
            &self.icons,
        ))
    }
}

impl PreparedBlockItems {
    /// Builds every held-block resource identically at startup and after a pack switch.
    pub(super) fn build(
        world: Option<Arc<RuntimeAssets>>,
        entities: &RuntimeEntityAssets,
        icons: &RuntimeIconCatalog,
    ) -> Arc<Self> {
        let mut blocks = world.as_deref().map_or_else(
            || BlockSheets {
                sheets: Vec::new(),
                by_visual: BTreeMap::new(),
                models: BTreeMap::new(),
            },
            |world| {
                let mut blocks =
                    render_model::equipment::blocks::collect_held(world, entities, icons);
                blocks.prefer_carried_faces(world, icons);
                blocks
            },
        );
        let mut carried = BTreeMap::new();
        for sheet in carried_sheets(entities, icons, &blocks.models) {
            let index = *carried.entry(sheet.sprite).or_insert_with(|| {
                let index = blocks.sheets.len();
                blocks
                    .sheets
                    .push(icons.sprites()[sheet.sprite as usize].clone());
                index
            });
            blocks.by_visual.insert(sheet.visual.0, index);
        }
        let BlockSheets {
            sheets,
            by_visual,
            models,
        } = blocks;
        let alpha = alpha::block_alpha_modes(world.as_deref(), &by_visual);
        let atlas = SpriteAtlas::pack(&sheets);
        Arc::new(Self {
            world,
            sheets: by_visual,
            models,
            alpha,
            placements: atlas.placements,
            rasters: atlas.layers,
        })
    }
}

/// Selects compatible authored carried routes without replacing a shaped block model.
fn carried_sheets<'a>(
    entities: &RuntimeEntityAssets,
    icons: &'a RuntimeIconCatalog,
    models: &'a BTreeMap<u32, Vec<render_model::ActorRigVertex>>,
) -> impl Iterator<Item = &'a assets::IconBlockSheet> {
    let compatible = icons.source_manifest_sha256() == entities.source_manifest_sha256();
    let visual_count = entities.block_visual_count();
    icons.block_sheets().iter().filter(move |sheet| {
        compatible && sheet.visual.0 < visual_count && !models.contains_key(&sheet.visual.0)
    })
}

impl PreparedBlockItems {
    /// The retained carrier allocation identifies the exact prepared terrain generation.
    pub fn matches(&self, world: &Arc<RuntimeAssets>) -> bool {
        self.world
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(source, world))
    }
}

impl EquipmentRuntime {
    /// Shares only the immutable inputs needed by the resource-pack worker.
    pub fn block_item_preparer(&self) -> Option<BlockItemPreparer> {
        Some(BlockItemPreparer {
            world: Arc::clone(self.startup_blocks.source.world.as_ref()?),
            entities: Arc::clone(&self.assets),
            icons: Arc::clone(&self.icons),
        })
    }

    /// Replaces all terrain-derived mesh caches together with their artwork locations.
    pub(crate) fn set_block_items(
        &mut self,
        prepared: Option<&crate::prepared_actor_artwork::PlacedBlockItems>,
    ) {
        let blocks = prepared.unwrap_or(&self.startup_blocks);
        if Arc::ptr_eq(&self.blocks.source, &blocks.source)
            && self.blocks.locations == blocks.locations
        {
            return;
        }
        self.meshes.retain(|key, rig| {
            if matches!(key, MeshKey::Block(_)) {
                self.free_meshes.extend(*rig);
                false
            } else {
                true
            }
        });
        self.pending
            .retain(|geometry| !self.free_meshes.contains(&geometry.id));
        self.blocks = blocks.clone();
    }

    /// Resolves a block's sheet and location from the same generation as its model.
    pub(super) fn block_placement(
        &self,
        visual: u32,
    ) -> Option<(usize, Placement, ActorArtworkLocation)> {
        let index = *self.blocks.source.sheets.get(&visual)?;
        let placement = self
            .blocks
            .source
            .placements
            .get(index)
            .copied()
            .flatten()?;
        Some((
            index,
            placement,
            self.blocks
                .locations
                .get(placement.layer)
                .copied()
                .flatten()?,
        ))
    }

    /// Returns only the active terrain generation's model, including the absence of one.
    pub(super) fn block_model(&self, visual: u32) -> Option<&[render_model::ActorRigVertex]> {
        self.blocks.source.models.get(&visual).map(Vec::as_slice)
    }

    /// Reads alpha policy from the carrier that supplied the active block's pixels.
    pub(super) fn block_alpha_mode(&self, visual: u32) -> Option<render_model::HandItemAlphaMode> {
        self.blocks.source.alpha.get(&visual).copied()
    }
}
