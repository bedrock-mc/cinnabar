//! The session's server-pack attachable layer, consulted before the startup catalog.

use super::*;

/// Separates armor mappings sourced from a replaceable pack.
pub(super) const ARMOR_CACHE_PREFIX: &str = "\u{1}pack:";

/// The selected model and the owner-admitted startup definition for missing artwork.
pub(super) struct BindingSource {
    pub catalog: Arc<RuntimeEquipmentCatalog>,
    pub from_pack: bool,
    pub index: usize,
    pub startup: Option<usize>,
}

/// A placed raster and its index in the catalog that owns its pixels.
#[derive(Clone, Copy)]
pub(super) struct TextureArtwork {
    pub texture_index: usize,
    pub location: ActorArtworkLocation,
}

/// Binds catalog indices before discarding textures without an artwork placement.
pub(super) fn placed_textures(
    textures: &[assets::EquipmentTexture],
    locations: impl IntoIterator<Item = Option<ActorArtworkLocation>>,
) -> BTreeMap<Box<str>, TextureArtwork> {
    textures
        .iter()
        .enumerate()
        .zip(locations)
        .filter_map(|((texture_index, texture), location)| {
            Some((
                texture.identifier.clone(),
                TextureArtwork {
                    texture_index,
                    location: location?,
                },
            ))
        })
        .collect()
}

/// A server pack's entity catalog, its attachables if it has any, and their artwork locations.
pub type PackEquipmentLayer = (
    Arc<RuntimeEntityAssets>,
    Option<Arc<RuntimeEquipmentCatalog>>,
    Vec<Option<ActorArtworkLocation>>,
);

/// A pack's entity catalog, which also owns the bodies of pack-rigged actors, and its attachables.
pub struct PackEquipment {
    pub(super) assets: Arc<RuntimeEntityAssets>,
    pub(super) catalog: Option<Arc<RuntimeEquipmentCatalog>>,
    pub(super) native_items: BTreeMap<Box<str>, super::native::NativeArtwork>,
    pub(super) texture_locations: BTreeMap<Box<str>, TextureArtwork>,
    pub(super) armor_geometry: BTreeMap<Box<str>, Option<Arc<ArmorGeometry>>>,
    pub(super) attachables: actor_animation::AttachablesRuntime,
}

/// Body-bone and armor-map keys of pack geometries sit in the pack rig range, apart from startup
/// geometry indices and skin rig ids.
fn is_pack_body_key(key: u32) -> bool {
    render_model::is_pack_rig_id(EntityRigId(key))
}

impl EquipmentRuntime {
    /// Selects worn equipment for its owner, admitting only ordinary definitions without one.
    pub(super) fn worn_binding_source(
        &mut self,
        item: &WornItem,
        equipment: &ActorEquipmentInput,
        animation: Option<EquipmentAnimation<'_>>,
        slot: ArmorSlot,
    ) -> Option<BindingSource> {
        match animation {
            Some(animation) => self.selected_binding_source(
                item,
                animation,
                animation.worn_input(equipment, slot, &[]),
            ),
            None => self.unowned_binding_source(item),
        }
    }

    /// Selects held variants with the same owner timing and hand context as animation.
    pub(super) fn held_binding_source(
        &mut self,
        item: &WornItem,
        equipment: &ActorEquipmentInput,
        animation: Option<EquipmentAnimation<'_>>,
        off_hand: bool,
    ) -> Option<BindingSource> {
        if let Some(animation) = animation {
            return self.selected_binding_source(
                item,
                animation,
                animation.held_input(equipment, off_hand),
            );
        }
        self.unowned_binding_source(item)
    }

    /// Admits ordinary definitions only, retaining the same startup choice for fallback.
    pub(super) fn unowned_binding_source(&self, item: &WornItem) -> Option<BindingSource> {
        let mut sources = [
            self.pack
                .as_ref()
                .and_then(|pack| Some((pack.catalog.as_ref()?, &pack.assets, true))),
            self.catalog
                .as_ref()
                .map(|catalog| (catalog, &self.assets, false)),
        ]
        .into_iter()
        .flatten()
        .filter_map(|(catalog, assets, from_pack)| {
            let start = catalog
                .bindings()
                .partition_point(|binding| binding.identifier.as_ref() < item.identifier.as_ref());
            let offset = catalog
                .bindings_for(&item.identifier)
                .iter()
                .rposition(|binding| {
                    binding.attachable.is_none()
                        && assets
                            .attachable_rig_binding(binding.attachable_identifier())
                            .is_none_or(|rig| assets.rig_bindings()[rig].item_condition.is_none())
                })?;
            Some((Arc::clone(catalog), from_pack, start + offset))
        });
        let (catalog, from_pack, index) = sources.next()?;
        let startup = if from_pack {
            sources.next().map(|(_, _, index)| index)
        } else {
            Some(index)
        };
        Some(BindingSource {
            catalog,
            from_pack,
            index,
            startup,
        })
    }

    /// Admits one drawable definition using the caller's complete inputs and a shared budget.
    pub(super) fn selected_binding_source(
        &mut self,
        item: &WornItem,
        animation: EquipmentAnimation<'_>,
        input: actor_animation::AttachableAnimationInput<'_>,
    ) -> Option<BindingSource> {
        let enchanted = (
            assets::entity::EngineVariable::IsEnchanted.name(),
            f32::from(item.enchanted),
        );
        let variables = if input.owner_variables.is_empty() {
            std::borrow::Cow::Borrowed(std::slice::from_ref(&enchanted))
        } else {
            let mut values = input.owner_variables.to_vec();
            values.push(enchanted);
            std::borrow::Cow::Owned(values)
        };
        let input = actor_animation::AttachableAnimationInput {
            owner_variables: &variables,
            ..input
        };
        let mut operations_left = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK;
        let families: std::collections::BTreeSet<Box<str>> = self
            .pack
            .as_ref()
            .and_then(|pack| pack.catalog.as_ref())
            .into_iter()
            .flat_map(|catalog| catalog.bindings_for(&item.identifier))
            .filter(|binding| {
                input.worn
                    && binding.geometry.resolution == EntityDependencyResolution::External
                    && self.external_armor.contains(&binding.geometry.identifier)
            })
            .map(|binding| binding.geometry.identifier.clone())
            .collect();
        let external: std::collections::BTreeSet<_> = families
            .into_iter()
            .filter(|family| {
                self.external_armor_binding(family, animation, input, &mut operations_left)
                    .is_some()
            })
            .collect();
        let pack_selection = self.pack.as_mut().and_then(|pack| {
            let catalog = pack.catalog.as_ref()?;
            let index = pack.attachables.select_equipment_binding(
                catalog,
                &item.identifier,
                animation.owner,
                animation.rig,
                input,
                &mut operations_left,
                |binding| {
                    !pack
                        .assets
                        .geometry_candidates(&binding.geometry.identifier)
                        .is_empty()
                        || !self
                            .assets
                            .geometry_candidates(&binding.geometry.identifier)
                            .is_empty()
                        || (binding.geometry.resolution == EntityDependencyResolution::External
                            && external.contains(&binding.geometry.identifier))
                },
            )?;
            Some((Arc::clone(catalog), index))
        });
        let startup = self.catalog.as_ref().and_then(|catalog| {
            self.attachables.select_equipment_binding(
                catalog,
                &item.identifier,
                animation.owner,
                animation.rig,
                input,
                &mut operations_left,
                |binding| {
                    !self
                        .assets
                        .geometry_candidates(&binding.geometry.identifier)
                        .is_empty()
                        || (binding.attachable.is_none()
                            && binding.geometry.resolution == EntityDependencyResolution::External)
                },
            )
        });
        let (catalog, from_pack, index) = if let Some((catalog, index)) = pack_selection {
            (catalog, true, index)
        } else {
            (Arc::clone(self.catalog.as_ref()?), false, startup?)
        };
        Some(BindingSource {
            catalog,
            from_pack,
            index,
            startup,
        })
    }

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

    /// The shared item glint for held items and legacy worn equipment.
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
        // Startup texture meshes also depend on the replaced artwork layer.
        self.java_rasters.clear();
        self.free_meshes
            .extend(self.attachable_meshes.values().copied());
        self.attachable_meshes.clear();
        self.free_meshes
            .extend(self.native_meshes.values().copied());
        self.native_meshes.clear();
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
            native_items: super::native::recognize(
                &assets,
                catalog.as_deref(),
                &self.assets,
                self.catalog.as_deref(),
            ),
            attachables: actor_animation::AttachablesRuntime::new(Arc::clone(&assets)),
            texture_locations: placed_textures(
                catalog.as_ref().map_or(&[], |catalog| catalog.textures()),
                locations,
            ),
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
        texture: &str,
        source: &BindingSource,
    ) -> Option<(Arc<RuntimeEquipmentCatalog>, usize, ActorArtworkLocation)> {
        let base = self.catalog.as_ref().unwrap_or(&source.catalog);
        self.artwork_for_texture(texture, base).or_else(|| {
            if !source.from_pack {
                return None;
            }
            let catalog = self.catalog.as_ref()?;
            let binding = catalog.bindings().get(source.startup?)?;
            self.artwork_for_texture(&binding.texture.identifier, catalog)
        })
    }

    /// Looks up one texture with pack precedence, without selecting another fallback binding.
    fn artwork_for_texture(
        &self,
        texture: &str,
        base: &Arc<RuntimeEquipmentCatalog>,
    ) -> Option<(Arc<RuntimeEquipmentCatalog>, usize, ActorArtworkLocation)> {
        if let Some(pack) = &self.pack
            && let Some(artwork) = &pack.catalog
            && let Some(placement) = pack.texture_locations.get(texture)
        {
            return Some((
                Arc::clone(artwork),
                placement.texture_index,
                placement.location,
            ));
        }
        if let Some(placement) = self.texture_locations.get(texture) {
            return Some((
                Arc::clone(base),
                placement.texture_index,
                placement.location,
            ));
        }
        None
    }

    /// Returns only the artwork placement from the selected catalog layer.
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
        locations
            .get(identifier)
            .map(|placement| placement.location)
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
