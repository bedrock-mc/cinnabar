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

    /// The item glint raster retained independently for legacy worn equipment.
    pub fn item_glint(catalog: &RuntimeEquipmentCatalog) -> Option<EquipmentRaster> {
        Self::glint_raster(catalog, assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER)
    }

    /// Finds a validated enchantment image by its shared runtime identifier.
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
        // Startup texture meshes also depend on the replaced artwork layer.
        self.java_rasters.clear();
        self.free_meshes
            .extend(self.attachable_meshes.values().copied());
        self.attachable_meshes.clear();
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

    /// Prefers pack declarations; drawing resolves controller artwork before falling back.
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

    /// Resolves selected pixels by path without changing the binding or controller result.
    /// Authored models with missing artwork may use their startup item's default raster.
    pub(super) fn draw_artwork(
        &self,
        identifier: &str,
        texture: &str,
        (catalog, from_pack): (Arc<RuntimeEquipmentCatalog>, bool),
    ) -> Option<(Arc<RuntimeEquipmentCatalog>, usize, ActorArtworkLocation)> {
        if let Some(pack) = &self.pack
            && let Some(artwork) = &pack.catalog
            && let Some(location) = pack.texture_locations.get(texture)
            && let Some(index) = artwork
                .textures()
                .iter()
                .position(|image| image.identifier.as_ref() == texture)
        {
            return Some((Arc::clone(artwork), index, *location));
        }
        let base = self.catalog.as_ref().unwrap_or(&catalog);
        if let Some(index) = base
            .textures()
            .iter()
            .position(|image| image.identifier.as_ref() == texture)
            && let Some(location) = self.texture_locations.get(texture)
        {
            return Some((Arc::clone(base), index, *location));
        }
        if !from_pack {
            return None;
        }
        let catalog = self.catalog.as_ref()?;
        let binding = catalog.binding(identifier)?;
        self.draw_artwork(
            identifier,
            &binding.texture.identifier,
            (Arc::clone(catalog), false),
        )
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
