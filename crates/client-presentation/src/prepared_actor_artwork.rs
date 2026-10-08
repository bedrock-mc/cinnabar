//! Pack workers prepare immutable artwork and meshes before session publication.
use crate::presentation::equipment::EquipmentRuntime;
use assets::SessionEntityPack;
use render::{ActorArtworkLocation, ActorArtworkPages};
use render_model::ActorRigGeometry;
use std::{borrow::Cow, sync::Arc};

/// Source snapshots prevent a concurrent reload from publishing stale routes or geometry.
#[derive(Debug)]
pub struct PreparedActorArtwork {
    base: ActorArtworkPages,
    pack: Arc<SessionEntityPack>,
    resources: PreparedSessionResources,
}

/// All pack-owned pixels and meshes; copies retain their immutable backing allocations.
#[derive(Clone, Debug)]
pub(crate) struct PreparedSessionResources {
    pub(crate) pages: ActorArtworkPages,
    pub(crate) locations: Vec<Option<ActorArtworkLocation>>,
    pub(crate) entities: Vec<ActorRigGeometry>,
    pub(crate) equipment: Vec<ActorRigGeometry>,
}

impl PreparedSessionResources {
    /// Runs texture packing and mesh construction on the worker that owns the accepted pack.
    fn new(base: &ActorArtworkPages, pack: Option<&SessionEntityPack>) -> Self {
        let mut resources = Self {
            pages: base.clone(),
            locations: Vec::new(),
            entities: Vec::new(),
            equipment: Vec::new(),
        };
        let Some(pack) = pack else {
            return resources;
        };
        {
            let _span =
                bevy::log::info_span!("actor.pack_entity_artwork", textures = pack.textures.len())
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
            let _span = bevy::log::info_span!(
                "actor.pack_equipment_meshes",
                bindings = catalog.bindings().len()
            )
            .entered();
            resources.equipment = EquipmentRuntime::pack_geometries(&pack.assets, catalog);
        }
        resources
    }
}

impl PreparedActorArtwork {
    /// Builds the immutable resources before the main frame can observe this pack.
    pub fn new(base: &ActorArtworkPages, pack: &Arc<SessionEntityPack>) -> Self {
        Self {
            base: base.clone(),
            pack: pack.clone(),
            resources: PreparedSessionResources::new(base, Some(pack)),
        }
    }

    /// Returns shared pages only while both immutable inputs are still the same snapshots.
    pub fn pages_for(
        &self,
        base: &ActorArtworkPages,
        pack: &Arc<SessionEntityPack>,
    ) -> Option<ActorArtworkPages> {
        self.resources_for(base, pack)
            .map(|resources| resources.pages.clone())
    }

    /// Artwork routes and meshes always come from the same accepted source generation.
    fn resources_for(
        &self,
        base: &ActorArtworkPages,
        pack: &Arc<SessionEntityPack>,
    ) -> Option<&PreparedSessionResources> {
        (Arc::ptr_eq(&self.pack, pack) && self.base.shares_storage_with(base))
            .then_some(&self.resources)
    }
}

/// Worker snapshots are borrowed; callers without one retain the synchronous fallback.
pub(crate) fn session_resources<'a>(
    base: &ActorArtworkPages,
    pack: Option<&Arc<SessionEntityPack>>,
    prepared: Option<&'a PreparedActorArtwork>,
) -> Cow<'a, PreparedSessionResources> {
    if let Some(resources) =
        pack.and_then(|pack| prepared.and_then(|prepared| prepared.resources_for(base, pack)))
    {
        Cow::Borrowed(resources)
    } else {
        Cow::Owned(PreparedSessionResources::new(base, pack.map(AsRef::as_ref)))
    }
}
