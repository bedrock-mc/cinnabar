//! Pack workers prepare immutable artwork and meshes before session publication.
use crate::presentation::equipment::{EquipmentRuntime, PreparedBlockItems};
use assets::SessionEntityPack;
use render_model::{ActorArtworkLocation, ActorArtworkPages};
use render_model::{
    ActorRigGeometry, EntityRigId, PreparedBlockEntityArtwork, block_entity::HeldBlockEntity,
};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

/// Source snapshots prevent a concurrent reload from publishing stale routes or geometry.
#[derive(Debug)]
pub struct PreparedActorArtwork {
    base: ActorArtworkPages,
    pack: Option<Arc<SessionEntityPack>>,
    block_entity_art: Option<Arc<PreparedBlockEntityArtwork>>,
    resources: PreparedSessionResources,
}

/// All pack-owned pixels and meshes; copies retain their immutable backing allocations.
#[derive(Clone, Debug)]
pub(crate) struct PreparedSessionResources {
    pub(crate) pages: ActorArtworkPages,
    pub(crate) locations: Vec<Option<ActorArtworkLocation>>,
    pub(crate) entities: Vec<ActorRigGeometry>,
    pub(crate) equipment: Vec<ActorRigGeometry>,
    pub(crate) held_block_entities: Option<PreparedHeldBlockEntities>,
    pub(crate) block_items: Option<PlacedBlockItems>,
}

/// Held terrain inputs paired with their locations in one published artwork atlas.
#[derive(Clone, Debug)]
pub(crate) struct PlacedBlockItems {
    pub(crate) source: Arc<PreparedBlockItems>,
    pub(crate) locations: Vec<Option<ActorArtworkLocation>>,
}

/// One atlas generation's held bindings and immutable geometry, prepared together.
#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedHeldBlockEntities {
    pub(crate) identity: Option<[u8; 32]>,
    pub(crate) models: BTreeMap<HeldBlockEntity, (EntityRigId, ActorArtworkLocation, [f32; 2])>,
    pub(crate) geometries: Vec<ActorRigGeometry>,
}

impl PreparedSessionResources {
    /// Runs texture packing and mesh construction on the worker that owns the accepted pack.
    fn new(
        base: &ActorArtworkPages,
        pack: Option<&SessionEntityPack>,
        block_entity_art: Option<&PreparedBlockEntityArtwork>,
    ) -> Self {
        let mut resources = Self {
            pages: base.clone(),
            locations: Vec::new(),
            entities: Vec::new(),
            equipment: Vec::new(),
            held_block_entities: None,
            block_items: None,
        };
        if let Some(pack) = pack {
            {
                let _span = bevy::log::info_span!(
                    "actor.pack_entity_artwork",
                    textures = pack.textures.len()
                )
                .entered();
                resources.pages = resources
                    .pages
                    .with_pack_artwork(&pack.textures, &pack.bindings);
            }
            {
                let _span = bevy::log::info_span!(
                    "actor.pack_entity_meshes",
                    bindings = pack.assets.rig_geometries().len()
                )
                .entered();
                resources.entities = render_model::pack_geometries(&pack.assets);
            }
            if let Some(catalog) = &pack.equipment {
                {
                    let _span = bevy::log::info_span!(
                        "actor.pack_equipment_artwork",
                        textures = catalog.textures().len()
                    )
                    .entered();
                    (resources.pages, resources.locations) = resources
                        .pages
                        .with_equipment_rasters(&EquipmentRuntime::pack_rasters(catalog));
                }
                if let Some(glint) = EquipmentRuntime::actor_glint(catalog) {
                    resources.pages = resources.pages.with_actor_glint(glint);
                }
                if let Some(glint) = EquipmentRuntime::item_glint(catalog) {
                    resources.pages = resources.pages.with_item_glint(glint);
                }
                let _span = bevy::log::info_span!(
                    "actor.pack_equipment_meshes",
                    bindings = catalog.bindings().len()
                )
                .entered();
                resources.equipment = EquipmentRuntime::pack_geometries(&pack.assets, catalog);
            }
            // The frame's catalog layout then matches duplicates by pointer, not by bytes.
            render_model::share_identical_vertices(
                resources
                    .entities
                    .iter_mut()
                    .chain(&mut resources.equipment),
            );
        }
        if let Some(artwork) = block_entity_art {
            let (pages, held) =
                EquipmentRuntime::prepare_block_entity_artwork(&resources.pages, artwork);
            resources.pages = pages;
            resources.held_block_entities = Some(held);
        }
        resources
    }
}

impl PreparedActorArtwork {
    /// Builds the immutable resources before the main frame can observe this pack.
    pub fn new(base: &ActorArtworkPages, pack: &Arc<SessionEntityPack>) -> Self {
        Self::new_with_block_entity_artwork(base, Some(pack), None)
    }

    /// Prepares entity and held block-entity artwork from the same accepted pack generation.
    pub fn new_with_block_entity_artwork(
        base: &ActorArtworkPages,
        pack: Option<&Arc<SessionEntityPack>>,
        block_entity_art: Option<&Arc<PreparedBlockEntityArtwork>>,
    ) -> Self {
        Self::new_with_block_items(base, pack, block_entity_art, None)
    }

    /// Packs terrain-derived held models into the same publication as actor artwork.
    pub fn new_with_block_items(
        base: &ActorArtworkPages,
        pack: Option<&Arc<SessionEntityPack>>,
        block_entity_art: Option<&Arc<PreparedBlockEntityArtwork>>,
        blocks: Option<Arc<PreparedBlockItems>>,
    ) -> Self {
        let mut prepared = Self {
            base: base.clone(),
            pack: pack.cloned(),
            block_entity_art: block_entity_art.cloned(),
            resources: PreparedSessionResources::new(
                base,
                pack.map(AsRef::as_ref),
                block_entity_art.map(AsRef::as_ref),
            ),
        };
        if let Some(blocks) = blocks {
            let (pages, locations) = prepared
                .resources
                .pages
                .with_equipment_rasters(&blocks.rasters);
            prepared.resources.pages = pages;
            prepared.resources.block_items = Some(PlacedBlockItems {
                source: blocks,
                locations,
            });
        }
        prepared
    }

    /// Reuses held-block preparation only for the exact retained terrain carrier.
    pub fn block_items_for(
        &self,
        world: &Arc<assets::RuntimeAssets>,
    ) -> Option<Arc<PreparedBlockItems>> {
        self.resources
            .block_items
            .as_ref()
            .map(|blocks| &blocks.source)
            .filter(|blocks| blocks.matches(world))
            .cloned()
    }

    /// Includes terrain-derived held resources in the worker snapshot's reuse decision.
    pub fn matches_block_items(&self, blocks: Option<&Arc<PreparedBlockItems>>) -> bool {
        same_snapshot(
            self.resources
                .block_items
                .as_ref()
                .map(|blocks| &blocks.source),
            blocks,
        )
    }

    /// Returns shared pages only while both immutable inputs are still the same snapshots.
    pub fn pages_for(
        &self,
        base: &ActorArtworkPages,
        pack: &Arc<SessionEntityPack>,
    ) -> Option<ActorArtworkPages> {
        self.resources_for(base, Some(pack))
            .map(|resources| resources.pages.clone())
    }

    /// Artwork routes and meshes always come from the same accepted source generation.
    fn resources_for(
        &self,
        base: &ActorArtworkPages,
        pack: Option<&Arc<SessionEntityPack>>,
    ) -> Option<&PreparedSessionResources> {
        (same_snapshot(self.pack.as_ref(), pack) && self.base.shares_storage_with(base))
            .then_some(&self.resources)
    }

    /// Checks every worker input before a caller reuses prepared pixels, bindings or meshes.
    pub fn matches_sources(
        &self,
        base: &ActorArtworkPages,
        pack: Option<&Arc<SessionEntityPack>>,
        block_entity_art: Option<&Arc<PreparedBlockEntityArtwork>>,
    ) -> bool {
        self.resources_for(base, pack).is_some()
            && same_snapshot(self.block_entity_art.as_ref(), block_entity_art)
    }
}

/// Compares optional immutable source allocations without scanning their contents.
fn same_snapshot<T>(left: Option<&Arc<T>>, right: Option<&Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

/// Worker snapshots are borrowed; callers without one retain the synchronous fallback.
pub(crate) fn session_resources<'a>(
    base: &ActorArtworkPages,
    pack: Option<&Arc<SessionEntityPack>>,
    prepared: Option<&'a PreparedActorArtwork>,
) -> Cow<'a, PreparedSessionResources> {
    if let Some(resources) = prepared.and_then(|prepared| prepared.resources_for(base, pack)) {
        Cow::Borrowed(resources)
    } else {
        Cow::Owned(PreparedSessionResources::new(
            base,
            pack.map(AsRef::as_ref),
            None,
        ))
    }
}
