//! Immutable startup artwork pages. Pixel decoding and hashing never run per frame.
use assets::RuntimeActorCatalog;
use bevy::prelude::Resource;
use render_model::EntityRigId;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[cfg(test)]
#[path = "artwork/color_mask_tests.rs"]
mod color_mask_tests;
#[path = "artwork/multitexture.rs"]
mod multitexture;
#[cfg(test)]
#[path = "artwork/page_capacity_tests.rs"]
mod page_capacity_tests;

/// CPU draw routing selects an artwork page independently of the shader's texture layer.
pub type ActorArtworkPageId = u16;
/// Player skins occupy page zero; generic artwork occupies the remaining page IDs.
pub const MAX_ACTOR_TEXTURE_PAGES: usize = ActorArtworkPageId::MAX as usize + 1;
/// Layers per generic entity page, within every backend's array-layer limit.
const MAX_ACTOR_PAGE_LAYERS: usize = 256;
// Cinnabar declared RGBA allocation ceiling, not retail or measured driver memory: vanilla
// startup art takes about 20 MiB. A page past it is downscaled to fit, never dropped.
pub const MAX_ACTOR_GPU_PIXEL_BYTES: usize = 512 * 1024 * 1024;

/// One equipment raster (item sprite or attachable texture) to place on a generic page.
#[derive(Clone, Debug)]
pub struct EquipmentRaster {
    pub width: u16,
    pub height: u16,
    pub rgba8: Arc<[u8]>,
}

fn within_page_budget(generic_pages: usize, declared_pixel_bytes: usize) -> bool {
    generic_pages < MAX_ACTOR_TEXTURE_PAGES && declared_pixel_bytes <= MAX_ACTOR_GPU_PIXEL_BYTES
}

/// Appends `page`, box-filtered down until it fits the byte budget, and returns its page id;
/// `None` only once every page id is taken.
fn push_page(
    pages: &mut Vec<ActorTexturePage>,
    gpu_bytes: &mut usize,
    page: ActorTexturePage,
) -> Option<ActorArtworkPageId> {
    let mut page = page;
    while !within_page_budget(pages.len() + 1, gpu_bytes.saturating_add(page.rgba8.len())) {
        let longest = u32::from(page.width.max(page.height));
        if pages.len() + 1 >= MAX_ACTOR_TEXTURE_PAGES || longest <= 1 {
            return None;
        }
        page = page.fit_within(longest / 2).into_owned();
    }
    *gpu_bytes += page.rgba8.len();
    pages.push(page);
    ActorArtworkPageId::try_from(pages.len()).ok()
}

/// Copies ordered texture layers into one allocation without a per-byte iterator.
fn concatenate_layers<'a>(layers: impl Iterator<Item = &'a [u8]> + Clone) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(layers.clone().map(<[u8]>::len).sum());
    for layer in layers {
        pixels.extend_from_slice(layer);
    }
    pixels
}

const fn player_page_bytes() -> usize {
    render_model::PLAYER_SKIN_BUDGET_BYTES
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActorArtworkLocation {
    pub(crate) page: ActorArtworkPageId,
    pub(crate) layer: u32,
    pub(crate) pose_mode: assets::ActorPoseMode,
    pub(crate) multitexture: Option<[u32; 2]>,
}
impl ActorArtworkLocation {
    pub fn pose_mode(self) -> assets::ActorPoseMode {
        self.pose_mode
    }
    pub fn page(self) -> ActorArtworkPageId {
        self.page
    }
    pub fn layer(self) -> u32 {
        self.layer
    }

    /// Keeps the draw's page identity when its hand pass supplies a separate one-layer image.
    pub fn single_layer_texture(self) -> Self {
        Self {
            layer: 0,
            multitexture: None,
            ..self
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorTexturePage {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) layers: u32,
    pub(crate) rgba8: Arc<[u8]>,
    /// Native USE_COLOR_MASK rasters cannot share a neutral-opacity material binding.
    pub(crate) color_mask: bool,
    pub(crate) multitexture: bool,
}
impl ActorTexturePage {
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    pub fn layers(&self) -> u32 {
        self.layers
    }
    pub fn pixels(&self) -> &[u8] {
        &self.rgba8
    }
    pub fn shared_pixels(&self) -> Arc<[u8]> {
        Arc::clone(&self.rgba8)
    }

    /// This page box-filtered down by the smallest power of two that fits `limit` per side.
    pub(crate) fn fit_within(&self, limit: u32) -> std::borrow::Cow<'_, Self> {
        let longest = u32::from(self.width.max(self.height));
        if longest <= limit || limit == 0 {
            return std::borrow::Cow::Borrowed(self);
        }
        let factor = longest.div_ceil(limit).next_power_of_two() as usize;
        let (width, height) = (usize::from(self.width), usize::from(self.height));
        let (out_width, out_height) = (width.div_ceil(factor), height.div_ceil(factor));
        let mut rgba8 = Vec::with_capacity(out_width * out_height * 4 * self.layers as usize);
        for layer in self.rgba8.chunks_exact(width * height * 4) {
            for y in 0..out_height {
                for x in 0..out_width {
                    let mut sum = [0u32; 4];
                    let mut count = 0;
                    for source_y in y * factor..((y + 1) * factor).min(height) {
                        for source_x in x * factor..((x + 1) * factor).min(width) {
                            let at = (source_y * width + source_x) * 4;
                            for (total, value) in sum.iter_mut().zip(&layer[at..at + 4]) {
                                *total += u32::from(*value);
                            }
                            count += 1;
                        }
                    }
                    rgba8.extend(sum.map(|total| (total / count) as u8));
                }
            }
        }
        std::borrow::Cow::Owned(Self {
            width: out_width as u16,
            height: out_height as u16,
            layers: self.layers,
            rgba8: rgba8.into(),
            color_mask: self.color_mask,
            multitexture: self.multitexture,
        })
    }
}

#[derive(Clone, Debug, Default, Resource)]
pub struct ActorArtworkPages {
    pub(crate) identity: [u8; 32],
    pub(crate) entity_identity: [u8; 32],
    pub(crate) actor_glint: Option<EquipmentRaster>,
    pub(crate) pages: Arc<[ActorTexturePage]>,
    routes: Arc<BTreeMap<EntityRigId, ActorArtworkLocation>>,
    /// Location of every catalog texture by entity-catalog source index.
    source_locations: Arc<BTreeMap<u32, ActorArtworkLocation>>,
    /// `(page, layer)` of every catalog texture; any entity rig may draw these variants.
    entity_locations: Arc<BTreeSet<(ActorArtworkPageId, u32)>>,
    /// `(page, layer)` of every equipment raster; equipment rigs are not entity routes.
    equipment: Arc<BTreeSet<(ActorArtworkPageId, u32)>>,
    /// Session pack textures by pack-catalog source index, a separate index space.
    pack_source_locations: Arc<BTreeMap<u32, ActorArtworkLocation>>,
    pack_locations: Arc<BTreeSet<(ActorArtworkPageId, u32)>>,
    rejected_bindings: usize,
}
impl ActorArtworkPages {
    /// Installs the shared actor glint image once, independently of skin and armor pages.
    #[must_use]
    pub fn with_actor_glint(mut self, raster: EquipmentRaster) -> Self {
        if raster.width == 0
            || raster.height == 0
            || raster.rgba8.len() != usize::from(raster.width) * usize::from(raster.height) * 4
        {
            return self;
        }
        let mut hasher = Sha256::new();
        hasher.update(self.identity);
        hasher.update(raster.width.to_le_bytes());
        hasher.update(raster.height.to_le_bytes());
        hasher.update(&raster.rgba8);
        self.identity = hasher.finalize().into();
        self.actor_glint = Some(raster);
        self
    }

    pub fn new(catalog: &RuntimeActorCatalog) -> Self {
        let dimensions = multitexture::page_dimensions(catalog);
        let mut groups = BTreeMap::<(u16, u16, bool, bool), Vec<usize>>::new();
        for (index, texture) in catalog.textures().iter().enumerate() {
            let multitexture = catalog.texture_uses_multitexture(index);
            let (width, height) = if multitexture {
                dimensions
            } else {
                (texture.width, texture.height)
            };
            groups
                .entry((
                    width,
                    height,
                    catalog.texture_uses_color_mask(index),
                    multitexture,
                ))
                .or_default()
                .push(index);
        }
        let mut pages = Vec::new();
        let mut locations = BTreeMap::new();
        // The existing player page retains all 128 layers and its full byte budget.
        let mut gpu_bytes = player_page_bytes();
        for ((width, height, color_mask, multitexture), indices) in groups {
            for indices in indices.chunks(MAX_ACTOR_PAGE_LAYERS) {
                let pixels = multitexture::page_pixels(catalog, indices, width, height);
                let page = ActorTexturePage {
                    width,
                    height,
                    layers: indices.len() as u32,
                    rgba8: pixels.into(),
                    color_mask,
                    multitexture,
                };
                let Some(page) = push_page(&mut pages, &mut gpu_bytes, page) else {
                    continue;
                };
                for (layer, index) in indices.iter().enumerate() {
                    locations.insert(
                        *index as u32,
                        ActorArtworkLocation {
                            page,
                            layer: layer as u32,
                            pose_mode: assets::ActorPoseMode::CompiledLiteral,
                            multitexture: None,
                        },
                    );
                }
            }
        }
        let routes: BTreeMap<_, _> = catalog
            .bindings()
            .iter()
            .filter_map(|binding| {
                locations
                    .get(&binding.texture)
                    .copied()
                    .map(|mut location| {
                        location.pose_mode = binding.pose_mode;
                        (EntityRigId(binding.geometry_candidate), location)
                    })
            })
            .collect();
        let rejected_bindings = catalog.bindings().len() - routes.len();
        let source_locations: BTreeMap<u32, ActorArtworkLocation> = catalog
            .textures()
            .iter()
            .enumerate()
            .filter_map(|(index, texture)| {
                Some((texture.source, locations.get(&(index as u32)).copied()?))
            })
            .collect();
        let entity_locations: BTreeSet<(ActorArtworkPageId, u32)> = source_locations
            .values()
            .map(|location| (location.page, location.layer))
            .collect();
        Self {
            source_locations: Arc::new(source_locations),
            entity_locations: Arc::new(entity_locations),
            identity: catalog.identity(),
            entity_identity: catalog.entity_identity(),
            actor_glint: None,
            pages: pages.into(),
            routes: Arc::new(routes),
            equipment: Arc::new(BTreeSet::new()),
            pack_source_locations: Arc::default(),
            pack_locations: Arc::default(),
            rejected_bindings,
        }
    }

    /// Appends pages for `rasters`, grouped by size, and returns each raster's location (`None`
    /// when its group would exceed the page or byte budget). The identity changes to cover them.
    #[must_use]
    pub fn with_equipment_rasters(
        self,
        rasters: &[EquipmentRaster],
    ) -> (Self, Vec<Option<ActorArtworkLocation>>) {
        self.with_raster_materials(rasters, &[])
    }

    fn with_raster_materials(
        mut self,
        rasters: &[EquipmentRaster],
        color_masks: &[bool],
    ) -> (Self, Vec<Option<ActorArtworkLocation>>) {
        let mut groups = BTreeMap::<(u16, u16, bool), Vec<usize>>::new();
        for (index, raster) in rasters.iter().enumerate() {
            if raster.width != 0
                && raster.height != 0
                && raster.rgba8.len() == usize::from(raster.width) * usize::from(raster.height) * 4
            {
                groups
                    .entry((
                        raster.width,
                        raster.height,
                        color_masks.get(index).copied().unwrap_or(false),
                    ))
                    .or_default()
                    .push(index);
            }
        }
        let mut pages = self.pages.to_vec();
        let mut gpu_bytes = pages
            .iter()
            .fold(player_page_bytes(), |total, page| total + page.rgba8.len());
        let mut locations = vec![None; rasters.len()];
        let mut equipment = (*self.equipment).clone();
        let mut hasher = Sha256::new();
        hasher.update(self.identity);
        for ((width, height, color_mask), indices) in groups {
            for indices in indices.chunks(MAX_ACTOR_PAGE_LAYERS) {
                let pixels =
                    concatenate_layers(indices.iter().map(|index| rasters[*index].rgba8.as_ref()));
                hasher.update(width.to_le_bytes());
                hasher.update(height.to_le_bytes());
                hasher.update((indices.len() as u32).to_le_bytes());
                hasher.update((pixels.len() as u64).to_le_bytes());
                hasher.update([u8::from(color_mask)]);
                hasher.update([0u8]); // Equipment/overrides are never three-sampler pages.
                hasher.update(&pixels);
                let page = ActorTexturePage {
                    width,
                    height,
                    layers: indices.len() as u32,
                    rgba8: pixels.into(),
                    color_mask,
                    multitexture: false,
                };
                let Some(page) = push_page(&mut pages, &mut gpu_bytes, page) else {
                    continue;
                };
                for (layer, index) in indices.iter().enumerate() {
                    locations[*index] = Some(ActorArtworkLocation {
                        page,
                        layer: layer as u32,
                        pose_mode: assets::ActorPoseMode::CompiledLiteral,
                        multitexture: None,
                    });
                    equipment.insert((page, layer as u32));
                }
            }
        }
        if pages.len() != self.pages.len() {
            self.identity = hasher.finalize().into();
            self.pages = pages.into();
            self.equipment = Arc::new(equipment);
        }
        (self, locations)
    }
    /// Appends session artwork under pack rig IDs, replacing its previous variant table.
    /// Call on startup pages so removed packs release their routes and pixel budget.
    #[must_use]
    pub fn with_pack_artwork(
        mut self,
        textures: &[assets::ActorTexture],
        bindings: &[assets::ActorArtworkBinding],
    ) -> Self {
        let mut groups = BTreeMap::<(u16, u16), Vec<usize>>::new();
        for (index, texture) in textures.iter().enumerate() {
            groups
                .entry((texture.width, texture.height))
                .or_default()
                .push(index);
        }
        let mut pages = self.pages.to_vec();
        let mut gpu_bytes = pages
            .iter()
            .fold(player_page_bytes(), |total, page| total + page.rgba8.len());
        let mut locations = BTreeMap::new();
        let mut hasher = Sha256::new();
        hasher.update(self.identity);
        for ((width, height), indices) in groups {
            for indices in indices.chunks(MAX_ACTOR_PAGE_LAYERS) {
                let pixels =
                    concatenate_layers(indices.iter().map(|index| textures[*index].rgba8.as_ref()));
                hasher.update(width.to_le_bytes());
                hasher.update(height.to_le_bytes());
                hasher.update((indices.len() as u32).to_le_bytes());
                hasher.update((pixels.len() as u64).to_le_bytes());
                hasher.update([0u8]); // Pack pages use literal RGBA, like unmasked rasters.
                hasher.update([0u8]); // No native three-sampler contract for arbitrary packs.
                hasher.update(&pixels);
                let page = ActorTexturePage {
                    width,
                    height,
                    layers: indices.len() as u32,
                    rgba8: pixels.into(),
                    color_mask: false,
                    multitexture: false,
                };
                let Some(page) = push_page(&mut pages, &mut gpu_bytes, page) else {
                    continue;
                };
                for (layer, index) in indices.iter().enumerate() {
                    locations.insert(
                        *index as u32,
                        ActorArtworkLocation {
                            page,
                            layer: layer as u32,
                            pose_mode: assets::ActorPoseMode::CompiledLiteral,
                            multitexture: None,
                        },
                    );
                }
            }
        }
        let pack_source_locations: BTreeMap<u32, ActorArtworkLocation> = textures
            .iter()
            .enumerate()
            .filter_map(|(index, texture)| {
                Some((texture.source, locations.get(&(index as u32)).copied()?))
            })
            .collect();
        self.pack_locations = Arc::new(
            pack_source_locations
                .values()
                .map(|location| (location.page, location.layer))
                .collect(),
        );
        self.pack_source_locations = Arc::new(pack_source_locations);
        let mut routes = (*self.routes).clone();
        let mut accepted = 0;
        for binding in bindings {
            let Some(mut location) = locations.get(&binding.texture).copied() else {
                continue;
            };
            location.pose_mode = binding.pose_mode;
            routes.insert(
                render_model::pack_rig_id(binding.geometry_candidate),
                location,
            );
            accepted += 1;
        }
        self.rejected_bindings += bindings.len() - accepted;
        if pages.len() != self.pages.len() {
            self.identity = hasher.finalize().into();
            self.pages = pages.into();
        }
        self.routes = Arc::new(routes);
        self
    }

    /// Replaces base texture routes while retaining the new images' original resolution.
    pub fn with_source_texture_overrides(self, overrides: &[(u32, EquipmentRaster)]) -> Self {
        let overrides: Vec<_> = overrides
            .iter()
            .filter(|(source, _)| self.source_locations.contains_key(source))
            .collect();
        let rasters: Vec<_> = overrides.iter().map(|(_, raster)| raster.clone()).collect();
        let color_masks: Vec<_> = overrides
            .iter()
            .map(|(source, _)| {
                let location = self.source_locations[source];
                self.pages[usize::from(location.page) - 1].color_mask
            })
            .collect();
        let (mut pages, locations) = self.with_raster_materials(&rasters, &color_masks);
        let mut sources = (*pages.source_locations).clone();
        let mut routes = (*pages.routes).clone();
        let mut variants = (*pages.entity_locations).clone();
        for ((source, _), replacement) in overrides.into_iter().zip(locations) {
            let (Some(old), Some(mut new)) = (sources.get(source).copied(), replacement) else {
                continue;
            };
            for route in routes.values_mut() {
                if (route.page, route.layer) == (old.page, old.layer) {
                    new.pose_mode = route.pose_mode;
                    *route = new;
                }
            }
            sources.insert(*source, new);
            variants.insert((new.page, new.layer));
        }
        pages.source_locations = Arc::new(sources);
        pages.routes = Arc::new(routes);
        pages.entity_locations = Arc::new(variants);
        pages
    }

    pub fn route(&self, rig: EntityRigId) -> Option<ActorArtworkLocation> {
        self.routes.get(&rig).copied()
    }
    /// Where the catalog texture drawn from entity-catalog source `source` lives, for a body of
    /// rig `rig`; `None` when the rig has no artwork or the source was not built.
    pub fn variant_location(&self, rig: EntityRigId, source: u32) -> Option<ActorArtworkLocation> {
        let route = self.route(rig)?;
        let sources = if render_model::is_pack_rig_id(rig) {
            &self.pack_source_locations
        } else {
            &self.source_locations
        };
        let mut location = sources.get(&source).copied()?;
        location.pose_mode = route.pose_mode;
        Some(location)
    }
    pub fn rejected_bindings(&self) -> usize {
        self.rejected_bindings
    }
    pub fn identity(&self) -> [u8; 32] {
        self.identity
    }
    pub fn actor_glint(&self) -> Option<&EquipmentRaster> {
        self.actor_glint.as_ref()
    }

    /// Recognizes clones of the exact artwork snapshot without scanning pixels or routes.
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.entity_identity == other.entity_identity
            && self.rejected_bindings == other.rejected_bindings
            && Arc::ptr_eq(&self.pages, &other.pages)
            && Arc::ptr_eq(&self.routes, &other.routes)
            && Arc::ptr_eq(&self.source_locations, &other.source_locations)
            && Arc::ptr_eq(&self.entity_locations, &other.entity_locations)
            && Arc::ptr_eq(&self.equipment, &other.equipment)
            && Arc::ptr_eq(&self.pack_source_locations, &other.pack_source_locations)
            && Arc::ptr_eq(&self.pack_locations, &other.pack_locations)
    }

    pub fn pages(&self) -> &[ActorTexturePage] {
        &self.pages
    }
    pub(crate) fn valid(&self, rig: EntityRigId, location: ActorArtworkLocation) -> bool {
        if !self.valid_multitexture(location) {
            return false;
        }
        if render_model::is_equipment_rig_id(rig) {
            return self.equipment.contains(&(location.page, location.layer));
        }
        let variants = if render_model::is_pack_rig_id(rig) {
            &self.pack_locations
        } else {
            &self.entity_locations
        };
        // A controller's own geometry draws any entity texture of its catalog.
        if render_model::is_layer_geometry_rig_id(rig) {
            return variants.contains(&(location.page, location.layer));
        }
        match self.route(rig) {
            Some(route) => {
                route == location
                    || (route.pose_mode == location.pose_mode
                        && variants.contains(&(location.page, location.layer)))
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_model::MAX_RENDERED_PLAYERS;

    /// Route-only changes can retain the pixel identity but must invalidate prepared artwork.
    #[test]
    fn shared_artwork_snapshot_checks_routes_as_well_as_pixels() {
        let original = ActorArtworkPages::default();
        let mut changed = original.clone();
        assert!(original.shares_storage_with(&changed));
        Arc::make_mut(&mut changed.routes).insert(
            EntityRigId(0),
            ActorArtworkLocation {
                page: 1,
                layer: 0,
                pose_mode: assets::ActorPoseMode::CompiledLiteral,
                multitexture: None,
            },
        );
        assert_eq!(original.identity(), changed.identity());
        assert!(Arc::ptr_eq(&original.pages, &changed.pages));
        assert!(!original.shares_storage_with(&changed));
    }

    #[test]
    fn page_budget_reserves_player_capacity_and_checks_exact_boundaries() {
        assert_eq!(MAX_RENDERED_PLAYERS, 128);
        assert!(player_page_bytes() < MAX_ACTOR_GPU_PIXEL_BYTES);
        assert_eq!(assets::MAX_ACTOR_TEXTURES, 2048);
        assert!(within_page_budget(
            MAX_ACTOR_TEXTURE_PAGES - 1,
            MAX_ACTOR_GPU_PIXEL_BYTES
        ));
        assert!(!within_page_budget(
            MAX_ACTOR_TEXTURE_PAGES,
            MAX_ACTOR_GPU_PIXEL_BYTES
        ));
        assert!(!within_page_budget(
            MAX_ACTOR_TEXTURE_PAGES - 1,
            MAX_ACTOR_GPU_PIXEL_BYTES + 1
        ));
    }

    #[test]
    fn base_source_override_retargets_variants_and_releases_replaced_pixels() {
        let route = ActorArtworkLocation {
            page: 1,
            layer: 0,
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
            multitexture: None,
        };
        let base = ActorArtworkPages {
            pages: vec![ActorTexturePage {
                width: 1,
                height: 1,
                layers: 1,
                rgba8: vec![3; 4].into(),
                color_mask: false,
                multitexture: false,
            }]
            .into(),
            routes: Arc::new(BTreeMap::from([(EntityRigId(0), route)])),
            source_locations: Arc::new(BTreeMap::from([(5, route)])),
            entity_locations: Arc::new(BTreeSet::from([(1, 0)])),
            ..Default::default()
        };
        let replacement = EquipmentRaster {
            width: 2,
            height: 2,
            rgba8: vec![7; 16].into(),
        };
        let applied = base
            .clone()
            .with_source_texture_overrides(&[(5, replacement)]);
        let new_route = applied.variant_location(EntityRigId(0), 5).unwrap();
        assert_eq!(applied.route(EntityRigId(0)), Some(new_route));
        assert_ne!(new_route.page, route.page);
        let page = &applied.pages()[usize::from(new_route.page) - 1];
        assert_eq!((page.width, page.height), (2, 2));
        assert_eq!(&page.rgba8[..], &[7; 16]);
        let pixels = Arc::downgrade(&page.rgba8);
        drop(applied);
        assert!(pixels.upgrade().is_none());
        assert_eq!(base.route(EntityRigId(0)), Some(route));
    }

    #[test]
    /// Different pixel and alpha values stay in their assigned texture layers.
    fn packed_artwork_preserves_layer_pixels_and_equipment_locations() {
        let first: Arc<[u8]> = Arc::from([1, 2, 3, 4]);
        let second: Arc<[u8]> = Arc::from([5, 6, 7, 8]);
        let textures = [first.clone(), second.clone()].map(|rgba8| assets::ActorTexture {
            source: 0,
            width: 1,
            height: 1,
            pixel_sha256: [0; 32],
            rgba8,
        });
        let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &[]);
        assert_eq!(pages.pages()[0].pixels(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(pages.pages()[0].layers(), 2);
        let rasters = [first, second].map(|rgba8| EquipmentRaster {
            width: 1,
            height: 1,
            rgba8,
        });
        let (equipment, locations) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
        assert_eq!(equipment.pages()[0].pixels(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(locations[0].unwrap().layer(), 0);
        assert_eq!(locations[1].unwrap().layer(), 1);
        assert_eq!(locations[0].unwrap().page(), locations[1].unwrap().page());
        assert_eq!(pages.identity(), equipment.identity());
    }

    // A pack with a texture size per page past the old 32-page cap places every texture.
    #[test]
    fn pack_art_of_many_sizes_gets_a_page_per_size() {
        let textures: Vec<_> = (1..=40u16)
            .map(|side| assets::ActorTexture {
                source: u32::from(side),
                width: side,
                height: 1,
                pixel_sha256: [0; 32],
                rgba8: vec![9; usize::from(side) * 4].into(),
            })
            .collect();
        let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &[]);
        assert_eq!(pages.pages().len(), 40);
    }

    // Same-size equipment rasters past one page's layer limit spill onto further pages.
    #[test]
    fn equipment_rasters_past_one_page_spill_onto_more_pages() {
        let raster = EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: vec![9; 4].into(),
        };
        let rasters = vec![raster; MAX_ACTOR_PAGE_LAYERS + 3];
        let (pages, locations) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
        assert_eq!(pages.pages().len(), 2);
        assert!(locations.iter().all(Option::is_some));
    }

    // Past the byte budget a page is downscaled to fit rather than dropped.
    #[test]
    fn a_page_past_the_byte_budget_is_downscaled_not_dropped() {
        let page = ActorTexturePage {
            width: 16,
            height: 16,
            layers: 1,
            rgba8: vec![9; 16 * 16 * 4].into(),
            color_mask: false,
            multitexture: false,
        };
        let mut pages = Vec::new();
        let mut gpu_bytes = MAX_ACTOR_GPU_PIXEL_BYTES - 16 * 16;
        assert_eq!(push_page(&mut pages, &mut gpu_bytes, page), Some(1));
        assert_eq!(pages[0].dimensions(), (8, 8));
        assert_eq!(gpu_bytes, MAX_ACTOR_GPU_PIXEL_BYTES);
    }

    #[test]
    fn fitting_a_page_halves_until_it_fits_and_averages_texels() {
        let page = ActorTexturePage {
            width: 2,
            height: 6,
            layers: 1,
            rgba8: [[0u8, 0, 0, 255], [255, 255, 255, 255]]
                .repeat(6)
                .concat()
                .into(),
            color_mask: true,
            multitexture: false,
        };
        let fitted = page.fit_within(4);
        assert_eq!((fitted.width, fitted.height), (1, 3));
        assert_eq!(&fitted.rgba8[..4], &[127, 127, 127, 255]);
        assert!(
            fitted.color_mask,
            "downscaling retains the material contract"
        );
        assert!(matches!(page.fit_within(6), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn pack_artwork_routes_bindings_under_pack_rig_ids() {
        let texture = |side: u16| assets::ActorTexture {
            source: 0,
            width: side,
            height: side,
            pixel_sha256: [1; 32],
            rgba8: vec![7; usize::from(side) * usize::from(side) * 4].into(),
        };
        let binding = |candidate: u32, texture: u32| assets::ActorArtworkBinding {
            rig: 0,
            geometry_candidate: candidate,
            entity_symbol: 0,
            geometry: 0,
            render_controller: 0,
            texture,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        };
        let pages = ActorArtworkPages::default()
            .with_pack_artwork(&[texture(2)], &[binding(5, 0), binding(6, 9)]);
        assert_eq!(pages.pages().len(), 1);
        let location = pages.route(render_model::pack_rig_id(5)).unwrap();
        assert!(pages.valid(render_model::pack_rig_id(5), location));
        assert_eq!(pages.route(render_model::pack_rig_id(6)), None);
        assert!(!render_model::is_equipment_rig_id(
            render_model::pack_rig_id(5)
        ));
        assert!(render_model::is_pack_rig_id(render_model::pack_rig_id(5)));
        assert_eq!(pages.rejected_bindings(), 1);
        assert_ne!(pages.identity(), [0; 32]);
    }

    // Pack render layers name pack-catalog sources, never vanilla ones.
    #[test]
    fn pack_rig_variants_resolve_in_the_pack_source_space() {
        let texture = |source: u32, fill: u8| assets::ActorTexture {
            source,
            width: 2,
            height: 2,
            pixel_sha256: [fill; 32],
            rgba8: vec![fill; 16].into(),
        };
        let binding = assets::ActorArtworkBinding {
            rig: 0,
            geometry_candidate: 0,
            entity_symbol: 0,
            geometry: 0,
            render_controller: 0,
            texture: 0,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        };
        let pages = ActorArtworkPages::default()
            .with_pack_artwork(&[texture(4, 1), texture(9, 2)], &[binding]);
        let rig = render_model::pack_rig_id(0);
        let variant = pages.variant_location(rig, 9).unwrap();
        assert_eq!((variant.page(), variant.layer()), (1, 1));
        assert!(pages.valid(rig, variant));
        assert_eq!(pages.variant_location(rig, 5), None);
        assert_eq!(pages.variant_location(EntityRigId(0), 9), None);
    }

    #[test]
    fn equipment_rasters_group_by_size_and_validate_by_location() {
        let raster = |side: u16| EquipmentRaster {
            width: side,
            height: side,
            rgba8: vec![9; usize::from(side) * usize::from(side) * 4].into(),
        };
        let bad = EquipmentRaster {
            width: 2,
            height: 2,
            rgba8: vec![0; 3].into(),
        };
        let (pages, locations) = ActorArtworkPages::default().with_equipment_rasters(&[
            raster(2),
            bad,
            raster(4),
            raster(2),
        ]);
        assert_eq!(pages.pages().len(), 2);
        assert_ne!(pages.identity(), [0; 32]);
        assert_eq!(locations[1], None);
        let (first, second) = (locations[0].unwrap(), locations[3].unwrap());
        assert_eq!((first.page(), second.page()), (1, 1));
        assert_eq!((first.layer(), second.layer()), (0, 1));
        assert_eq!(locations[2].unwrap().page(), 2);
        let equipment_rig = render_model::equipment_rig_id(3);
        assert!(pages.valid(equipment_rig, first));
        let unknown = ActorArtworkLocation { layer: 9, ..first };
        assert!(!pages.valid(equipment_rig, unknown));
    }
    #[test]
    fn review_render_artwork_identity_distinguishes_page_boundaries() {
        let marker = EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([2, 0, 1, 0]),
        };
        let pair = EquipmentRaster {
            width: 2,
            height: 1,
            rgba8: Arc::from([9; 8]),
        };
        let pixel = EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([9; 4]),
        };
        let (a, _) = ActorArtworkPages::default().with_equipment_rasters(&[marker.clone(), pair]);
        let (b, _) = ActorArtworkPages::default().with_equipment_rasters(&[
            marker.clone(),
            marker,
            pixel.clone(),
            pixel,
        ]);
        assert_ne!(a.identity(), b.identity());
    }
}
