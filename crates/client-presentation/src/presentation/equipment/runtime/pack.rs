//! The session's server-pack attachable layer, consulted before the startup catalog.

use super::*;

/// Separates armor mappings sourced from a replaceable pack.
pub(super) const ARMOR_CACHE_PREFIX: &str = "\u{1}pack:";

/// A server pack's entity catalog, attachable catalog and artwork locations.
pub type PackEquipmentLayer = (
    Arc<RuntimeEntityAssets>,
    Arc<RuntimeEquipmentCatalog>,
    Vec<Option<ActorArtworkLocation>>,
);

/// A pack's attachable bindings with its own entity catalog and artwork locations.
pub struct PackEquipment {
    pub(super) assets: Arc<RuntimeEntityAssets>,
    catalog: Arc<RuntimeEquipmentCatalog>,
    texture_locations: BTreeMap<Box<str>, ActorArtworkLocation>,
    armor_geometry: BTreeMap<Box<str>, Option<Arc<ArmorGeometry>>>,
    pub(super) attachables: client_world::AttachablesRuntime,
}

impl EquipmentRuntime {
    /// Rasters of the pack's attachable textures, in catalog order.
    pub fn pack_rasters(catalog: &RuntimeEquipmentCatalog) -> Vec<EquipmentRaster> {
        catalog
            .textures()
            .iter()
            .map(|texture| EquipmentRaster {
                width: texture.width,
                height: texture.height,
                rgba8: Arc::clone(&texture.rgba8),
            })
            .collect()
    }

    /// Geometries the actor scene must register for the pack's attachables, under pack
    /// equipment rig ids.
    pub fn pack_geometries(
        assets: &RuntimeEntityAssets,
        catalog: &RuntimeEquipmentCatalog,
    ) -> Vec<ActorRigGeometry> {
        let mut indices = catalog
            .bindings()
            .iter()
            .filter(|binding| binding.geometry.resolution == EntityDependencyResolution::Catalog)
            .filter_map(|binding| find_geometry_index(assets, &binding.geometry.identifier))
            .collect::<Vec<_>>();
        indices.sort_unstable();
        indices.dedup();
        indices
            .into_iter()
            .filter_map(|index| {
                render::equipment_geometry(
                    assets,
                    index as usize,
                    render::pack_equipment_rig_id(index),
                )
            })
            .collect()
    }

    /// Installs the session's pack layer, or removes it. `locations` parallel the catalog's
    /// textures (the pages `pack_rasters` produced).
    pub fn set_pack_layer(&mut self, layer: Option<PackEquipmentLayer>) {
        self.attachable_meshes.retain(|(from_pack, _, _), rig| {
            if *from_pack {
                self.free_meshes.push(*rig);
            }
            !from_pack
        });
        self.pending
            .retain(|geometry| !self.free_meshes.contains(&geometry.id));
        self.armor_maps
            .retain(|(_, geometry), _| !geometry.starts_with(ARMOR_CACHE_PREFIX));
        self.pack = layer.map(|(assets, catalog, locations)| PackEquipment {
            attachables: client_world::AttachablesRuntime::new(Arc::clone(&assets)),
            texture_locations: catalog
                .textures()
                .iter()
                .zip(locations)
                .filter_map(|(texture, location)| Some((texture.identifier.clone(), location?)))
                .collect(),
            assets,
            catalog,
            armor_geometry: BTreeMap::new(),
        });
    }

    /// The catalog binding an item resolves through, the pack's first, and whether it is the pack's.
    pub(super) fn binding_source(
        &self,
        identifier: &str,
    ) -> Option<(Arc<RuntimeEquipmentCatalog>, bool)> {
        if let Some(pack) = &self.pack
            && pack.catalog.binding(identifier).is_some()
        {
            return Some((Arc::clone(&pack.catalog), true));
        }
        let catalog = self.catalog.as_ref()?;
        catalog
            .binding(identifier)
            .map(|_| (Arc::clone(catalog), false))
    }

    pub(super) fn texture_location(
        &self,
        identifier: &str,
        from_pack: bool,
    ) -> Option<ActorArtworkLocation> {
        let locations = if from_pack {
            &self.pack.as_ref()?.texture_locations
        } else {
            &self.texture_locations
        };
        locations.get(identifier).copied()
    }

    /// A pack attachable's geometry: the pack catalog's, else the startup catalog's.
    pub(super) fn pack_armor_geometry_for(
        &mut self,
        identifier: &str,
    ) -> Option<Arc<ArmorGeometry>> {
        let vanilla = &self.assets;
        let pack = self.pack.as_mut()?;
        if let Some(entry) = pack.armor_geometry.get(identifier) {
            return entry.clone();
        }
        let from_pack = find_geometry_index(&pack.assets, identifier).and_then(|index| {
            Some(Arc::new(ArmorGeometry {
                rig: render::pack_equipment_rig_id(index),
                names: geometry_bone_names(&pack.assets, index as usize)?,
                pivots: geometry_bone_pivots(&pack.assets, index as usize)?,
            }))
        });
        let entry = from_pack.or_else(|| {
            find_geometry_index(vanilla, identifier).and_then(|index| {
                Some(Arc::new(ArmorGeometry {
                    rig: equipment_rig_id(index),
                    names: geometry_bone_names(vanilla, index as usize)?,
                    pivots: geometry_bone_pivots(vanilla, index as usize)?,
                }))
            })
        });
        pack.armor_geometry.insert(identifier.into(), entry.clone());
        entry
    }
}
