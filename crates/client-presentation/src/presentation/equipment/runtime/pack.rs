//! The session's server-pack attachable layer, consulted before the startup catalog.

use super::*;

/// Separates armor mappings sourced from a replaceable pack.
pub(super) const ARMOR_CACHE_PREFIX: &str = "\u{1}pack:";

/// A server pack's entity catalog, its attachables if it has any, and their artwork locations.
pub type PackEquipmentLayer = (
    Arc<RuntimeEntityAssets>,
    Option<Arc<RuntimeEquipmentCatalog>>,
    Vec<Option<ActorArtworkLocation>>,
);

/// A pack's entity catalog, which also owns the bodies of pack-rigged actors, and its attachables.
pub struct PackEquipment {
    pub(super) assets: Arc<RuntimeEntityAssets>,
    catalog: Option<Arc<RuntimeEquipmentCatalog>>,
    texture_locations: BTreeMap<Box<str>, ActorArtworkLocation>,
    armor_geometry: BTreeMap<Box<str>, Option<Arc<ArmorGeometry>>>,
    pub(super) attachables: actor_animation::AttachablesRuntime,
}

/// Body-bone and armor-map keys of pack geometries sit in the pack rig range, apart from startup
/// geometry indices and skin rig ids.
fn is_pack_body_key(key: u32) -> bool {
    render_model::is_pack_rig_id(EntityRigId(key))
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

    /// The catalog's shared enchantment raster, which binds apart from equipment pages.
    pub fn actor_glint(catalog: &RuntimeEquipmentCatalog) -> Option<EquipmentRaster> {
        Self::glint_raster(catalog, assets::ACTOR_GLINT_TEXTURE_IDENTIFIER)
    }

    /// The catalog's held-item enchantment raster, which binds beside the actor glint.
    pub fn item_glint(catalog: &RuntimeEquipmentCatalog) -> Option<EquipmentRaster> {
        Self::glint_raster(catalog, assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER)
    }

    /// Returns the catalog raster for one glint texture identifier.
    fn glint_raster(
        catalog: &RuntimeEquipmentCatalog,
        identifier: &str,
    ) -> Option<EquipmentRaster> {
        catalog
            .textures()
            .iter()
            .find(|texture| texture.identifier.as_ref() == identifier)
            .map(|texture| EquipmentRaster {
                width: texture.width,
                height: texture.height,
                rgba8: Arc::clone(&texture.rgba8),
            })
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
                render_model::equipment_geometry(
                    assets,
                    index as usize,
                    render_model::pack_equipment_rig_id(index),
                )
            })
            .collect()
    }

    /// Installs the session's pack layer, or removes it. `locations` parallel the catalog's
    /// textures (the pages `pack_rasters` produced).
    pub fn set_pack_layer(&mut self, layer: Option<PackEquipmentLayer>) {
        self.java_rasters.retain(|(from_pack, _, _), _| !from_pack);
        self.attachable_meshes.retain(|(from_pack, _, _), rig| {
            if *from_pack {
                self.free_meshes.push(*rig);
            }
            !from_pack
        });
        // Pack equipment ids name the replaced pack's geometry indices.
        self.pending.retain(|geometry| {
            !self.free_meshes.contains(&geometry.id)
                && !render_model::is_pack_equipment_rig_id(geometry.id)
        });
        self.selected_geometries
            .retain(|rig| !render_model::is_pack_equipment_rig_id(*rig));
        self.armor_maps.retain(|(body, geometry), _| {
            !geometry.starts_with(ARMOR_CACHE_PREFIX) && !is_pack_body_key(*body)
        });
        self.body_bones.retain(|body, _| !is_pack_body_key(*body));
        self.pack = layer.map(|(assets, catalog, locations)| PackEquipment {
            attachables: actor_animation::AttachablesRuntime::new(Arc::clone(&assets)),
            texture_locations: catalog
                .iter()
                .flat_map(|catalog| catalog.textures())
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
        if let Some(catalog) = self.pack.as_ref().and_then(|pack| pack.catalog.as_ref())
            && catalog.binding(identifier).is_some()
        {
            return Some((Arc::clone(catalog), true));
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

    /// The pack geometry and bones of a pack-rigged body, under its pack body key.
    pub(super) fn pack_body_bones(&mut self, rig: EntityRigId) -> Option<(u32, Arc<BodyBones>)> {
        let binding = rig.0.checked_sub(assets::PACK_RIG_ID_BASE)?;
        let pack = self.pack.as_ref()?;
        let geometry = pack.assets.rig_geometries().get(binding as usize)?.geometry;
        let key = assets::PACK_RIG_ID_BASE.checked_add(geometry)?;
        let assets = &pack.assets;
        let entry = self.body_bones.entry(key).or_insert_with(|| {
            Some(Arc::new(body_bones(geometry_bone_names(
                assets,
                geometry as usize,
            )?)))
        });
        entry.clone().map(|bones| (key, bones))
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
                rig: render_model::pack_equipment_rig_id(index),
                names: geometry_bone_names(&pack.assets, index as usize)?,
                pivots: geometry_bone_pivots(&pack.assets, index as usize)?,
                binding_expressions: geometry_bone_binding_expressions(
                    &pack.assets,
                    index as usize,
                )?,
            }))
        });
        let entry = from_pack.or_else(|| {
            find_geometry_index(vanilla, identifier).and_then(|index| {
                Some(Arc::new(ArmorGeometry {
                    rig: equipment_rig_id(index),
                    names: geometry_bone_names(vanilla, index as usize)?,
                    pivots: geometry_bone_pivots(vanilla, index as usize)?,
                    binding_expressions: geometry_bone_binding_expressions(
                        vanilla,
                        index as usize,
                    )?,
                }))
            })
        });
        pack.armor_geometry.insert(identifier.into(), entry.clone());
        entry
    }
}
