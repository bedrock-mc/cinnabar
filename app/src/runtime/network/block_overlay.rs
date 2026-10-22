//! Compiles the session's server block visuals into an overlay resolved after
//! the vanilla carrier. Anything missing or unsupported degrades to the
//! diagnostic look for that block and is counted, never failing the session.

use assets::material_keys::MaterialKeys;

mod condition;
mod items;
pub(super) use items::compile_block_item_overlay;
mod geometry_inputs;
use pack_compiler::block_geometry as geometry;
mod legacy;
mod storage;
mod texture_admission;
mod textures;
use texture_admission::TextureSlots;
pub(super) use textures::SharedTextures;
mod vanilla;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties,
    MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE,
    MATERIAL_FLAG_DISABLE_AO, MATERIAL_FLAG_DISABLE_FACE_DIMMING, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_WATER_TINT, MODEL_QUAD_FLAG_TWO_SIDED, Material, MaterialOverride, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, TextureArray, TextureMip, TextureRef,
    VisualKind, VisualSupport,
};
use protocol::{CustomBlocks, CustomVisualComponents};
use resource_pack::LayeredPackView;

use self::{
    condition::StateVisual,
    geometry::{FACE_NAMES, FaceQuad, Geometry},
    geometry_inputs::geometry_catalog,
    textures::{TextureCatalog, admit_static_rectangle, source_mip_chain},
};
use assets::texture::policy::{TexturePolicy, terrain_texture_policy};
use pack_runtime::pack_textures::DecodedTexture;

const FULL_BLOCK: &str = "minecraft:geometry.full_block";
const FULL_BLOCK_V1: &str = "minecraft:geometry.full_block_v1";
const MIN_TILE: u32 = assets::TILE_SIZE;
const MAX_OVERLAY_LAYERS: usize = assets::MAX_TEXTURE_LAYERS;
/// Base carrier page plus this many overlay shards fill [`assets::MAX_TEXTURE_PAGES`].
const MAX_OVERLAY_SHARDS: usize = assets::MAX_TEXTURE_PAGES - 1;
const DIAGNOSTIC_MATERIAL: u32 = 0;

/// Counted reasons some server block visuals are incomplete this session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OverlayGaps {
    pub(crate) missing_textures: u32,
    pub(crate) missing_geometry: u32,
    pub(crate) unevaluated_permutations: u32,
    pub(crate) skipped_cubes: u32,
    pub(crate) truncated_models: u32,
    pub(crate) approximated_materials: u32,
    pub(crate) incomplete_state_identities: u32,
    pub(crate) invalid_legacy_bindings: u32,
    /// Routes refused because the overlay texture array had no layer left.
    pub(crate) saturated_textures: u32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CompiledBlockOverlay {
    #[serde(with = "super::resource_packs::disk_cache::blocks::BlockOverlay")]
    pub(crate) overlay: BlockOverlay,
    pub(crate) gaps: OverlayGaps,
    pub(crate) vanilla_carried: Vec<CarriedBlock>,
}

impl assets::RetainedBuffers for CompiledBlockOverlay {
    fn visit_buffers(&self, visit: &mut assets::BufferVisitor<'_>) {
        let Self {
            overlay,
            gaps: _,
            vanilla_carried: _,
        } = self;
        overlay.visit_buffers(visit);
    }
}

/// Carried textures for one eligible vanilla full cube.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CarriedBlock {
    pub(crate) visual: u32,
    pub(crate) faces: [u32; 6],
}

/// Returns visuals for the admitted custom prefix in palette or hashed-state order.
/// Rejected runs stay unbacked; vanilla overrides may produce an overlay without custom blocks.
/// `base_assets` supplies the base carrier so faces naming
/// vanilla textures the server stack does not ship can reuse the base page
/// instead of degrading; server rasters still win whenever they decode.
#[cfg(test)]
pub(super) fn compile_block_overlay(
    view: &LayeredPackView,
    blocks: &CustomBlocks,
    hashed: bool,
    vanilla_keys: Option<&MaterialKeys>,
    max_texture_array_layers: u32,
    base_assets: Option<&assets::RuntimeAssets>,
) -> Option<CompiledBlockOverlay> {
    compile_block_overlay_with_textures(
        view,
        blocks,
        hashed,
        vanilla_keys,
        max_texture_array_layers,
        base_assets,
        None,
    )
}

/// Compiles world or inventory visuals using the same parsed terrain tables when available.
pub(super) fn compile_block_overlay_with_textures(
    view: &LayeredPackView,
    blocks: &CustomBlocks,
    hashed: bool,
    vanilla_keys: Option<&MaterialKeys>,
    max_texture_array_layers: u32,
    base_assets: Option<&assets::RuntimeAssets>,
    shared: Option<&SharedTextures>,
) -> Option<CompiledBlockOverlay> {
    if max_texture_array_layers == 0
        || base_assets.is_some_and(|base| base.materials().len() >= assets::MAX_MATERIALS)
    {
        return None;
    }
    let legacy = legacy::TextureBindings::new(view);
    let wanted = blocks
        .blocks
        .iter()
        .flat_map(|block| {
            std::iter::once(&block.visual.base).chain(
                block
                    .visual
                    .permutations
                    .iter()
                    .map(|permutation| &permutation.components),
            )
        })
        .filter_map(|components| components.geometry.as_deref())
        .filter(|identifier| !identifier.starts_with("minecraft:"))
        .collect::<HashSet<_>>();
    let max_layers = (max_texture_array_layers as usize).min(MAX_OVERLAY_LAYERS);
    let texture_policy = terrain_texture_policy();
    let mut catalog = TextureCatalog::reusing(view, vanilla_keys, shared);
    catalog.texture_policy = texture_policy;
    let geometries = geometry_catalog(view, &wanted);
    let mut builder = Builder {
        catalog,
        base_assets,
        base_keys: vanilla_keys,
        vanilla_carried: Vec::new(),
        terrain_materials: HashMap::new(),
        overlay: BlockOverlay {
            max_page_layers: max_layers as u32,
            ..BlockOverlay::default()
        },
        fallback: vanilla_keys
            .zip(base_assets)
            .map(|(keys, base)| assets::VanillaFallback::new(keys, base.materials())),
        sources: Vec::new(),
        source_bytes: 0,
        tiles: Vec::new(),
        upload_bytes: texture_policy.terrain_bytes(MIN_TILE, 1)?,
        texture_policy,
        max_layers,
        slots: TextureSlots::default(),
        materials: HashMap::new(),
        visuals: HashMap::new(),
        gaps: OverlayGaps {
            invalid_legacy_bindings: legacy.invalid,
            ..OverlayGaps::default()
        },
    };
    builder.sources.push((Source::Diagnostic, 0));
    builder.overlay.materials.push(Material {
        texture: TextureRef::new(1, 0).expect("layer zero"),
        flags: 0,
        animation: NO_ANIMATION,
        ..assets::Material::unvaried()
    });
    for (block_index, block) in blocks.blocks.iter().enumerate() {
        if block.visual.materialization_disabled {
            builder.gaps.incomplete_state_identities += (blocks.blocks.len() - block_index) as u32;
            break;
        }
        let expressions = condition::BlockExpressions::new(block);
        let states = block.hashed_states();
        let identities_complete = states.len() == block.state_count as usize;
        if !identities_complete {
            builder.gaps.incomplete_state_identities += 1;
        }
        if hashed {
            for (index, state) in states.into_iter().enumerate() {
                let mut visual = condition::state_visual(
                    block,
                    &expressions,
                    Some(&state.values),
                    &mut builder.gaps,
                );
                let physics = block.physics_for_state(index as u32);
                visual.components.random_offset = physics.random_offset;
                legacy.apply(&block.name, &mut visual.components);
                builder.push_state(
                    &visual,
                    &geometries,
                    block,
                    block
                        .mining_for_state(index as u32)
                        .map(|mining| mining.seconds_to_destroy),
                );
                builder.overlay.hashes.push(Some(state.hash));
            }
            continue;
        }
        for state in 0..block.state_count {
            let values = block.state_values(state);
            let mut visual =
                condition::state_visual(block, &expressions, values.as_deref(), &mut builder.gaps);
            let physics = block.physics_for_state(state);
            visual.components.random_offset = physics.random_offset;
            legacy.apply(&block.name, &mut visual.components);
            builder.push_state(
                &visual,
                &geometries,
                block,
                block
                    .mining_for_state(state)
                    .map(|mining| mining.seconds_to_destroy),
            );
            builder
                .overlay
                .hashes
                .push(identities_complete.then(|| states[state as usize].hash));
        }
    }
    if let Some(keys) = vanilla_keys {
        let _zone = bevy::log::info_span!("pack.blocks.vanilla_overrides").entered();
        builder.override_vanilla_materials(keys);
        builder.override_vanilla_bindings(keys, &legacy);
    }
    if blocks.blocks.is_empty()
        && builder.overlay.material_overrides.is_empty()
        && builder.overlay.face_overrides.is_empty()
        && builder.vanilla_carried.is_empty()
    {
        builder.catalog.emit_diagnostics();
        return None;
    }
    builder.catalog.emit_diagnostics();
    let _zone = bevy::log::info_span!("pack.blocks.finish").entered();
    builder.finish()
}

enum Source {
    Diagnostic,
    Image(DecodedTexture),
    GridImage(DecodedTexture, u8),
}

#[derive(Clone)]
struct TextureSlot {
    /// Page zero borrows the carrier; higher pages address overlay shards.
    page: u32,
    layer: u32,
    animation: u32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum TextureRoute {
    Catalog {
        key: String,
        variant: u32,
        choice: u32,
    },
    Path {
        key: String,
        variant: u32,
        path: String,
        choice: u32,
    },
}

impl TextureRoute {
    /// The catalog key used for diagnostics and inherited animation metadata.
    fn key(&self) -> &str {
        match self {
            Self::Catalog { key, .. } | Self::Path { key, .. } => key,
        }
    }
}

struct Builder<'a> {
    catalog: TextureCatalog<'a>,
    base_assets: Option<&'a assets::RuntimeAssets>,
    base_keys: Option<&'a MaterialKeys>,
    vanilla_carried: Vec<CarriedBlock>,
    terrain_materials: HashMap<(String, u32, u32), u32>,
    overlay: BlockOverlay,
    /// Base-carrier textures for vanilla keys the server stack does not ship.
    fallback: Option<assets::VanillaFallback<'a>>,
    /// Flat sources tagged with their shard; shards group equal-scale layers
    /// so one physical tile serves each page without inflating small art.
    sources: Vec<(Source, usize)>,
    source_bytes: usize,
    /// Current physical tile per open shard, bounding admitted layer bytes.
    tiles: Vec<u32>,
    /// Estimated upload bytes across shards, kept under the shared budget.
    upload_bytes: usize,
    texture_policy: TexturePolicy,
    max_layers: usize,
    slots: TextureSlots,
    materials: HashMap<(String, u32), u32>,
    visuals: HashMap<String, BlockVisual>,
    gaps: OverlayGaps,
}

impl Builder<'_> {
    /// Appends a state's mining value after reusing equivalent visual geometry.
    fn push_state(
        &mut self,
        state: &StateVisual,
        geometries: &HashMap<String, Geometry>,
        block: &protocol::CustomBlock,
        seconds_to_destroy: Option<f32>,
    ) {
        let light = state_light(&state.components);
        let mut visual = self.visual(state, geometries);
        visual.destroy = seconds_to_destroy.map_or(
            assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
            |seconds| assets::compiled::block_destroy::BlockDestroyFacts::Custom {
                seconds_to_destroy_bits: seconds.to_bits(),
                effective_tools: assets::facts::block::custom_block_effective_tools(
                    block.tags.iter().map(AsRef::as_ref),
                ),
            },
        );
        if state.components.random_offset.is_some()
            && matches!(
                state.components.geometry.as_deref(),
                None | Some(FULL_BLOCK | FULL_BLOCK_V1)
            )
        {
            let checkpoint = self.texture_checkpoint();
            let carried = self.cube(&state.components);
            // Diagnostic faces have no carried sheet; admitting them would
            // fail session validation and drop the whole overlay.
            if carried.support == VisualSupport::Exact
                && !carried.faces.contains(&DIAGNOSTIC_MATERIAL)
            {
                self.overlay
                    .carried_cube_faces
                    .push((self.overlay.visuals.len() as u32, carried.faces));
            } else {
                self.rollback_textures(checkpoint);
            }
        }
        self.overlay.visuals.push(visual);
        self.overlay.light_properties.push(light);
    }

    /// Deduplicates equivalent visuals before compiling against borrowed authored geometry.
    fn visual(
        &mut self,
        state: &StateVisual,
        geometries: &HashMap<String, Geometry>,
    ) -> BlockVisual {
        let components = &state.components;
        let key = format!("{components:?}{:?}", state.hidden_bones);
        if let Some(visual) = self.visuals.get(&key) {
            return *visual;
        }
        let checkpoint = self.texture_checkpoint();
        // Quarter turns keep a full cube; positive scale and translation move its faces. A
        // reflecting or zero scale would turn model faces inside out, so it stays a cube.
        let cube_transform = components.transformation.is_none_or(|transform| {
            (transform.scale == [1.0; 3] && transform.translation == [0.0; 3])
                || transform.scale.iter().any(|&axis| axis <= 0.0)
        });
        let visual = match components.geometry.as_deref() {
            None | Some(FULL_BLOCK | FULL_BLOCK_V1)
                if components.random_offset.is_none() && cube_transform =>
            {
                self.cube(components)
            }
            None | Some(FULL_BLOCK | FULL_BLOCK_V1) => {
                self.model(components, &geometry::Geometry::full_block(), &[])
            }
            Some(identifier) => match geometries.get(identifier) {
                Some(geometry) => self.model(components, geometry, &state.hidden_bones),
                None => {
                    self.gaps.missing_geometry += 1;
                    diagnostic_visual()
                }
            },
        };
        if visual.kind == VisualKind::Diagnostic {
            self.rollback_textures(checkpoint);
        }
        self.visuals.insert(key, visual);
        visual
    }

    fn cube(&mut self, components: &CustomVisualComponents) -> BlockVisual {
        let rotation = components
            .transformation
            .map_or([0; 3], |transform| transform.rotation);
        let mut faces = [DIAGNOSTIC_MATERIAL; 6];
        let mut opaque = true;
        for (face, slot) in faces.iter_mut().enumerate() {
            // The face showing on world side `face` was model face `source`.
            let source = rotate_face_inverse(face, rotation);
            let uv_flags = if components.geometry.as_deref() == Some(FULL_BLOCK_V1)
                && source == assets::BlockFace::Down as usize
            {
                render_scene::MATERIAL_UV_ROTATE_180
            } else {
                0
            };
            let (material, flags, _) =
                self.face_material(components, FACE_NAMES[source], None, true, uv_flags);
            opaque &= flags == 0;
            *slot = material;
        }
        // Failed faces keep the diagnostic material so the cube still
        // renders; one missing texture never sinks the other five faces.
        if faces.contains(&DIAGNOSTIC_MATERIAL) {
            self.gaps.missing_textures += 1;
        }
        let mut flags = BlockFlags::CUBE_GEOMETRY;
        flags.set(BlockFlags::OCCLUDES_FULL_FACE, opaque);
        let exact = components.transformation.is_none_or(|transform| {
            transform.rotation == [0; 3]
                && transform.scale == [1.0; 3]
                && transform.translation == [0.0; 3]
        }) && components.materials.as_deref().is_some_and(|materials| {
            materials.iter().all(|material| {
                matches!(material.render_method.as_deref(), None | Some("opaque"))
                    && matches!(material.tint_method.as_deref(), None | Some("none"))
            })
        });
        BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces,
            flags,
            kind: VisualKind::Cube,
            support: if exact {
                VisualSupport::Exact
            } else {
                VisualSupport::VanillaFallback
            },
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }
    }

    fn model(
        &mut self,
        components: &CustomVisualComponents,
        geometry: &Geometry,
        hidden_bones: &[Arc<str>],
    ) -> BlockVisual {
        self.gaps.skipped_cubes += geometry.skipped_cubes;
        if geometry.cubes.is_empty() && geometry.skipped_cubes > 0 {
            return diagnostic_visual();
        }
        let mut quads = Vec::new();
        let mut first_material = None;
        let mut failed = false;
        let shown = geometry
            .cubes
            .iter()
            .filter(|cube| !hidden_bones.iter().any(|bone| **bone == *cube.bone))
            .collect::<Vec<_>>();
        // A geometry with no cubes draws nothing: pack-authored null markers
        // pair cubeless geometry with a null texture to hide a state.
        if shown.is_empty() {
            return invisible_visual();
        }
        for cube in shown {
            for (mut face_quad, instance) in cube.quads() {
                if components.geometry.as_deref() == Some(FULL_BLOCK_V1)
                    && face_quad.face == assets::BlockFace::Down as usize
                {
                    // Model surfaces carry their final UVs, including the versioned cube bottom.
                    face_quad.uvs = face_quad.uvs.map(|uv| {
                        std::array::from_fn(|axis| geometry.texture_size[axis] - uv[axis])
                    });
                }
                let (material, _, two_sided) =
                    self.face_material(components, FACE_NAMES[face_quad.face], instance, false, 0);
                // Failed quads keep the diagnostic material so the model
                // still renders; one missing texture never sinks the mesh.
                failed |= material == DIAGNOSTIC_MATERIAL;
                first_material.get_or_insert(material);
                if let Some(quad) = quantize(&face_quad, geometry, components, material, two_sided)
                {
                    quads.push(quad);
                }
            }
        }
        let Some(material) = first_material.filter(|_| !quads.is_empty()) else {
            return diagnostic_visual();
        };
        if failed {
            self.gaps.missing_textures += 1;
        }
        let template = self.overlay.model_templates.len() as u32;
        let mut start = self.overlay.model_quads.len() as u32;
        let count = quads.len().div_ceil(assets::MAX_MODEL_TEMPLATE_QUADS);
        for (index, part) in quads.chunks(assets::MAX_MODEL_TEMPLATE_QUADS).enumerate() {
            if let Some(component) = components.random_offset {
                self.overlay
                    .model_random_offsets
                    .push((self.overlay.model_templates.len() as u32, component));
            }
            self.overlay.model_templates.push(ModelTemplate {
                quad_start: start,
                quad_count: part.len() as u32,
                flags: if index + 1 < count {
                    assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
                } else {
                    0
                },
            });
            start += part.len() as u32;
        }
        self.overlay.model_quads.extend(quads);
        BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: [material; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            support: VisualSupport::VanillaFallback,
            contributor_role: ContributorRole::Primary,
            model_template: template,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }
    }

    /// Resolves a face's material instance: an explicit geometry name, then the
    /// face, then `side` for horizontal faces, then `*`.
    /// Diagnostic fallback keeps the selected instance's sidedness.
    fn face_material(
        &mut self,
        components: &CustomVisualComponents,
        face: &str,
        instance: Option<&str>,
        full_block: bool,
        uv_flags: u32,
    ) -> (u32, u32, bool) {
        let Some(materials) = components.materials.as_deref() else {
            return (DIAGNOSTIC_MATERIAL, 0, false);
        };
        let horizontal = !matches!(face, "up" | "down");
        let chosen = instance
            .into_iter()
            .chain(std::iter::once(face))
            .chain(horizontal.then_some("side"))
            .chain(std::iter::once("*"))
            .find_map(|name| {
                materials
                    .iter()
                    .find(|material| material.name.as_ref() == name)
            });
        let Some(chosen) = chosen else {
            return (DIAGNOSTIC_MATERIAL, 0, false);
        };
        let two_sided = matches!(
            chosen.render_method.as_deref(),
            Some(
                "alpha_test"
                    | "alpha_test_to_opaque"
                    | "double_sided"
                    | "blend"
                    | "blend_to_opaque"
            )
        );
        let mut flags = match chosen.render_method.as_deref() {
            None | Some("opaque" | "double_sided") => 0,
            Some("blend") | Some("blend_to_opaque") => MATERIAL_FLAG_ALPHA_BLEND,
            Some(_) => MATERIAL_FLAG_ALPHA_CUTOUT,
        };
        if full_block && flags == MATERIAL_FLAG_ALPHA_BLEND {
            // Blended full cubes need a transparent-cube template; cutout is the
            // closest supported look.
            self.gaps.approximated_materials += 1;
            flags = MATERIAL_FLAG_ALPHA_CUTOUT;
        }
        let alpha_flags = flags;
        flags |= uv_flags | tint_flags(chosen.tint_method.as_deref());
        if chosen.ambient_occlusion == Some(0.0) {
            flags |= MATERIAL_FLAG_DISABLE_AO;
        } else if chosen.ambient_occlusion.is_some_and(|value| value != 1.0) {
            self.gaps.approximated_materials += 1;
        }
        if chosen.face_dimming == Some(false) {
            flags |= MATERIAL_FLAG_DISABLE_FACE_DIMMING;
        }
        let texture = chosen.texture.to_string();
        if let Some(&material) = self.materials.get(&(texture.clone(), flags)) {
            return (material, alpha_flags, two_sided);
        }
        if !self.can_admit_materials(1) {
            return (DIAGNOSTIC_MATERIAL, 0, two_sided);
        }
        let Some(slot) = self.texture_slot(&texture) else {
            return (DIAGNOSTIC_MATERIAL, 0, two_sided);
        };
        let material = self.overlay.materials.len() as u32;
        self.overlay.materials.push(Material {
            texture: TextureRef::new(slot.page, slot.layer).expect("bounded layer"),
            flags,
            animation: slot.animation,
            ..assets::Material::unvaried()
        });
        self.materials.insert((texture, flags), material);
        (material, alpha_flags, two_sided)
    }

    /// Resolves a custom block's default choice, borrowing only unchanged base routes.
    fn texture_slot(&mut self, key: &str) -> Option<TextureSlot> {
        let route = TextureRoute::Catalog {
            key: key.to_owned(),
            variant: 0,
            choice: 0,
        };
        // Custom aliases resolve their selected filename against the complete pack stack,
        // including carrier donors, with destination tint and UV sampling retained.
        if self.catalog.redefined(key, 0)
            && let Some(keys) = self.base_keys
        {
            return self.carrier_slot(route, keys);
        }
        self.route_slot(route.clone()).or_else(|| {
            if self.catalog.redefined(key, 0) {
                return None;
            }
            let storage_key = self.catalog.storage_key(&route, None)?;
            if !matches!(
                self.slots.frames(&storage_key, false),
                Some(Err(texture_admission::FrameFailure::Missing))
            ) {
                return None;
            }
            let reference = self.fallback.as_ref()?.texture(key, storage_key.tint)?;
            Some(TextureSlot {
                page: reference.page(),
                layer: reference.layer(),
                animation: NO_ANIMATION,
            })
        })
    }

    /// Reuses raster-equivalent pack requests before admitting more pixels.
    fn route_slot(&mut self, route: TextureRoute) -> Option<TextureSlot> {
        self.request_slot(route, None)
    }

    /// Allows a static carrier donor while reusing any equivalent admitted pack raster.
    fn carrier_slot(&mut self, route: TextureRoute, keys: &MaterialKeys) -> Option<TextureSlot> {
        self.request_slot(route, Some(keys))
    }

    /// Uses one raster identity for caching, independently of the caller's route representation.
    fn request_slot(
        &mut self,
        route: TextureRoute,
        keys: Option<&MaterialKeys>,
    ) -> Option<TextureSlot> {
        let identity = self.catalog.identity(&route)?;
        if let Some(slot) = self.slots.get(&identity, keys.is_some()) {
            return slot;
        }
        let loaded = self.load_texture(&route, keys);
        self.slots.insert(identity, loaded, keys.is_some())
    }

    /// Resolves a material's complete atlas destination using the same route admission.
    #[cfg(test)]
    fn texture_slot_for(&mut self, key: &str, selector: (u32, u32)) -> Option<TextureSlot> {
        self.route_slot(TextureRoute::Catalog {
            key: key.to_owned(),
            variant: selector.0,
            choice: selector.1,
        })
    }

    /// Layers currently assigned to one shard.
    fn shard_len(&self, shard: usize) -> usize {
        self.sources
            .iter()
            .filter(|(_, assigned)| *assigned == shard)
            .count()
    }

    fn finish(mut self) -> Option<CompiledBlockOverlay> {
        for (source, _) in &mut self.sources {
            if let Source::Image(texture) | Source::GridImage(texture, _) = source {
                admit_static_rectangle(texture);
            }
        }
        // Shards own contiguous runs of the tagged stream; directly pushed
        // fixtures share shard zero. Each page takes the tile of its largest
        // source so small art never subsidizes HD neighbours.
        let shards = self
            .sources
            .iter()
            .map(|(_, shard)| *shard)
            .max()
            .map_or(0, |last| last + 1);
        if shards > MAX_OVERLAY_SHARDS {
            return None;
        }
        let mut tiles = Vec::with_capacity(shards);
        let mut total = 0usize;
        for shard in 0..shards {
            let largest = self
                .sources
                .iter()
                .filter(|(_, assigned)| *assigned == shard)
                .filter_map(|(source, _)| match source {
                    Source::Image(texture) | Source::GridImage(texture, _) => {
                        Some(texture.width.max(texture.height))
                    }
                    Source::Diagnostic => None,
                })
                .max()
                .unwrap_or(MIN_TILE);
            let tile = largest.checked_next_power_of_two()?.max(MIN_TILE);
            let layers = self
                .sources
                .iter()
                .filter(|(_, assigned)| *assigned == shard)
                .count();
            if layers == 0 || layers > self.max_layers {
                return None;
            }
            total = total.checked_add(self.texture_policy.terrain_bytes(tile, layers)?)?;
            if total > self.texture_policy.storage_bytes() {
                return None;
            }
            tiles.push(tile);
        }
        let mut sizes = Vec::with_capacity(self.sources.len());
        let mut grids = Vec::with_capacity(self.sources.len());
        let mut textures = Vec::with_capacity(shards);
        for (shard, tile) in tiles.iter().copied().enumerate() {
            let mut mips: Vec<Vec<u8>> = Vec::new();
            let mut layers = 0u32;
            for (source, assigned) in &self.sources {
                if *assigned != shard {
                    continue;
                }
                let (dimensions, chain) = match source {
                    Source::Diagnostic => (
                        [tile as u16; 2],
                        assets::build_legacy_terrain_mip_chain(&diagnostic_pixels(tile), tile)
                            .ok()?,
                    ),
                    Source::Image(texture) | Source::GridImage(texture, _) => {
                        source_mip_chain(texture, tile)?
                    }
                };
                let grid = if let Source::GridImage(_, grid) = source {
                    *grid
                } else {
                    0
                };
                let (dimensions, chain, grid) = if dimensions.iter().any(|&size| {
                    size.checked_shr(u32::from(grid))
                        .is_none_or(|size| size == 0)
                }) {
                    self.gaps.missing_textures = self.gaps.missing_textures.saturating_add(1);
                    (
                        [tile as u16; 2],
                        assets::build_legacy_terrain_mip_chain(&diagnostic_pixels(tile), tile)
                            .ok()?,
                        0,
                    )
                } else {
                    (dimensions.map(|size| size >> grid), chain, grid)
                };
                sizes.push(dimensions);
                grids.push(grid);
                layers += 1;
                mips.resize(chain.len(), Vec::new());
                for (level, mip) in chain.iter().enumerate() {
                    mips[level].extend_from_slice(&mip.rgba8);
                }
            }
            let mut size = tile;
            let mips = mips
                .into_iter()
                .map(|rgba8| {
                    let mip = TextureMip {
                        size,
                        rgba8: rgba8.into_boxed_slice(),
                    };
                    size = (size / 2).max(1);
                    mip
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            textures.push(TextureArray { layers, mips });
        }
        self.overlay.texture_source_sizes = sizes;
        self.overlay.texture_source_grids = grids;
        self.overlay.textures = textures;
        Some(CompiledBlockOverlay {
            overlay: self.overlay,
            gaps: self.gaps,
            vanilla_carried: self.vanilla_carried,
        })
    }
}

/// Biome tint of a material instance's `tint_method`; unknown methods do not tint.
fn tint_flags(method: Option<&str>) -> u32 {
    match method {
        Some("default_foliage") => MATERIAL_FLAG_FOLIAGE_TINT,
        Some("birch_foliage") => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE,
        Some("evergreen_foliage") => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE,
        Some("dry_foliage") => MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE,
        Some("grass") => MATERIAL_FLAG_GRASS_TINT,
        Some("water") => MATERIAL_FLAG_WATER_TINT,
        _ => 0,
    }
}

/// Explicit light components override geometry's zero absorption or legacy type fallback.
fn state_light(components: &CustomVisualComponents) -> LightProperties {
    let emission = components.light_emission.unwrap_or(0).min(15);
    let dampening = components.effective_light_dampening();
    LightProperties::new(emission, dampening).unwrap_or(LightProperties::OPAQUE_DARK)
}

fn diagnostic_visual() -> BlockVisual {
    BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary)
}

/// A cubeless model or one whose bone visibility hides every cube draws nothing.
fn invisible_visual() -> BlockVisual {
    BlockVisual {
        kind: VisualKind::Invisible,
        support: VisualSupport::Exact,
        ..diagnostic_visual()
    }
}

fn diagnostic_pixels(tile: u32) -> Box<[u8]> {
    let cell = (tile / 16).max(1);
    (0..tile * tile)
        .flat_map(|index| {
            let (x, y) = (index % tile / cell, index / tile / cell);
            if (x + y) % 2 == 0 {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        })
        .collect()
}

/// Maps a face through quarter-turn rotations applied X, then Y, then Z.
fn rotate_face(face: usize, rotation: [i32; 3]) -> usize {
    let mut normal = FACE_NORMALS[face];
    for (axis, turns) in rotation.into_iter().enumerate() {
        for _ in 0..turns.rem_euclid(4) {
            normal = block_transform::rotation::rotate_quarter(normal, axis);
        }
    }
    FACE_NORMALS
        .iter()
        .position(|candidate| *candidate == normal)
        .unwrap_or(face)
}

fn rotate_face_inverse(face: usize, rotation: [i32; 3]) -> usize {
    (0..6)
        .find(|&source| rotate_face(source, rotation) == face)
        .unwrap_or(face)
}

const FACE_NORMALS: [[i32; 3]; 6] = [
    [-1, 0, 0],
    [1, 0, 0],
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
];

/// Applies the block transformation about the block centre and converts to
/// carrier fixed point: 1/256 block positions and 1/4096 tile UVs.
fn quantize(
    quad: &FaceQuad,
    geometry: &Geometry,
    components: &CustomVisualComponents,
    material: u32,
    two_sided: bool,
) -> Option<ModelQuad> {
    let transform = components.transformation;
    let mut positions = [[0i16; 3]; 4];
    for (slot, corner) in positions.iter_mut().zip(quad.positions) {
        let mut point = corner.map(|value| value - 8.0);
        if let Some(transform) = transform {
            point = transform.apply(point);
        }
        for axis in 0..3 {
            let fixed = ((point[axis] + 8.0) * 16.0).round();
            if !fixed.is_finite() || fixed.abs() > f32::from(i16::MAX) {
                return None;
            }
            slot[axis] = fixed as i16;
        }
    }
    let mut uvs = [[0u16; 2]; 4];
    for (slot, uv) in uvs.iter_mut().zip(quad.uvs) {
        for axis in 0..2 {
            let fixed = (uv[axis] / geometry.texture_size[axis] * 4096.0).round();
            slot[axis] = fixed.clamp(0.0, f32::from(u16::MAX)) as u16;
        }
    }
    if quad.rotated {
        return Some(ModelQuad {
            positions,
            uvs,
            material,
            flags: if two_sided {
                MODEL_QUAD_FLAG_TWO_SIDED
            } else {
                0
            },
        });
    }
    let face = rotate_face(
        quad.face,
        transform.map_or([0; 3], |transform| transform.rotation),
    );
    let normal = FACE_NORMALS[face];
    let axis = normal
        .iter()
        .position(|&component| component != 0)
        .unwrap_or(0);
    let boundary = if normal[axis] < 0 { 0 } else { 256 };
    // Only a quad lying wholly within the neighbour's face can be hidden by that neighbour.
    let on_boundary = positions.iter().all(|corner| {
        (0..3).all(|other| {
            let value = i32::from(corner[other]);
            if other == axis {
                value == boundary
            } else {
                (0..=256).contains(&value)
            }
        })
    });
    let face_flag = MODEL_FACE_FLAGS[face];
    // Displaced surfaces can be exposed beside an undisplaced neighbour.
    let mut flags = face_flag
        | if on_boundary && components.random_offset.is_none() {
            face_flag << 4
        } else {
            0
        };
    if two_sided {
        flags |= MODEL_QUAD_FLAG_TWO_SIDED;
    }
    Some(ModelQuad {
        positions,
        uvs,
        material,
        flags,
    })
}

/// Model quad face codes (`1..=6` = down/up/west/east/north/south) by face index.
const MODEL_FACE_FLAGS: [u32; 6] = [3, 4, 1, 2, 5, 6];

#[cfg(all(test, feature = "reports"))]
mod pack_report;
#[cfg(test)]
mod tests;
