//! Worker-prepared terrain artwork and models for held block items.

use super::*;

/// Immutable startup inputs used to rebuild held blocks away from the frame thread.
#[derive(Clone)]
pub struct BlockItemPreparer {
    world: Arc<RuntimeAssets>,
    entities: Arc<RuntimeEntityAssets>,
    icons: Arc<RuntimeIconCatalog>,
}

/// A complete held-block generation, including its geometry, pixels and alpha policy.
pub struct PreparedBlockItems {
    world: Arc<RuntimeAssets>,
    sheets: BTreeMap<u32, usize>,
    models: BTreeMap<u32, Vec<render_model::ActorRigVertex>>,
    alpha: BTreeMap<u32, render_model::HandItemAlphaMode>,
    placements: Vec<Option<Placement>>,
    pub(crate) rasters: Vec<EquipmentRaster>,
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
        let BlockSheets {
            sheets,
            by_visual,
            models,
        } = render_model::equipment::blocks::collect_held(world, &self.entities, &self.icons);
        let alpha = alpha::block_alpha_modes(Some(world), &by_visual);
        let atlas = SpriteAtlas::pack(&sheets);
        Some(Arc::new(PreparedBlockItems {
            world: Arc::clone(world),
            sheets: by_visual,
            models,
            alpha,
            placements: atlas.placements,
            rasters: atlas.layers,
        }))
    }
}

impl PreparedBlockItems {
    /// The retained carrier allocation identifies the exact prepared terrain generation.
    pub fn matches(&self, world: &Arc<RuntimeAssets>) -> bool {
        Arc::ptr_eq(&self.world, world)
    }
}

impl EquipmentRuntime {
    /// Shares only the immutable inputs needed by the resource-pack worker.
    pub fn block_item_preparer(&self) -> Option<BlockItemPreparer> {
        Some(BlockItemPreparer {
            world: Arc::clone(self.block_world.as_ref()?),
            entities: Arc::clone(&self.assets),
            icons: Arc::clone(&self.icons),
        })
    }

    /// Replaces all terrain-derived mesh caches together with their artwork locations.
    pub(crate) fn set_block_items(
        &mut self,
        prepared: Option<&crate::prepared_actor_artwork::PlacedBlockItems>,
    ) {
        let unchanged = match (&self.reloaded_blocks, prepared) {
            (None, None) => true,
            (Some(old), Some(next)) => {
                Arc::ptr_eq(&old.source, &next.source) && old.locations == next.locations
            }
            _ => false,
        };
        if unchanged {
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
        self.reloaded_blocks = prepared.cloned();
    }

    /// Resolves a block's sheet and location from the same generation as its model.
    pub(super) fn block_placement(
        &self,
        visual: u32,
    ) -> Option<(usize, Placement, ActorArtworkLocation)> {
        let (sheets, placements, locations) = match &self.reloaded_blocks {
            Some(blocks) => (
                &blocks.source.sheets,
                &blocks.source.placements,
                &blocks.locations,
            ),
            None => (&self.block_sheets, &self.placements, &self.atlas_locations),
        };
        let index = *sheets.get(&visual)?;
        let placement = placements.get(index).copied().flatten()?;
        Some((
            index,
            placement,
            locations.get(placement.layer).copied().flatten()?,
        ))
    }

    /// Returns only the active terrain generation's model, including the absence of one.
    pub(super) fn block_model(&self, visual: u32) -> Option<&[render_model::ActorRigVertex]> {
        self.reloaded_blocks
            .as_ref()
            .map_or(&self.block_models, |blocks| &blocks.source.models)
            .get(&visual)
            .map(Vec::as_slice)
    }

    /// Reads alpha policy from the carrier that supplied the active block's pixels.
    pub(super) fn block_alpha_mode(&self, visual: u32) -> Option<render_model::HandItemAlphaMode> {
        self.reloaded_blocks
            .as_ref()
            .map_or(&self.block_alpha, |blocks| &blocks.source.alpha)
            .get(&visual)
            .copied()
    }
}
