//! Compiles the session's server block visuals into an overlay resolved after
//! the vanilla carrier. Anything missing or unsupported degrades to the
//! diagnostic look for that block and is counted, never failing the session.

mod condition;
mod geometry;
mod legacy;
mod textures;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use assets::{
    Animation, BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties,
    MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE,
    MATERIAL_FLAG_DISABLE_AO, MATERIAL_FLAG_DISABLE_FACE_DIMMING, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_WATER_TINT, MODEL_QUAD_FLAG_TWO_SIDED, Material, MaterialKeys, MaterialOverride,
    ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, TextureArray, TextureMip,
    TextureRef, VisualKind, VisualSupport,
};
use protocol::{CustomBlocks, CustomVisualComponents};
use resource_pack::LayeredPackView;

use self::{
    condition::StateVisual,
    geometry::{FACE_NAMES, FaceQuad, Geometry, geometry_catalog},
    textures::{
        DecodedTexture, TextureCatalog, admit_static_rectangle, flipbook_frames, shrink_to_max,
        source_mip_chain,
    },
};

const FULL_BLOCK: &str = "minecraft:geometry.full_block";
const FULL_BLOCK_V1: &str = "minecraft:geometry.full_block_v1";
const MIN_TILE: u32 = assets::TILE_SIZE;
const MAX_TILE: u32 = 128;
const MAX_OVERLAY_LAYERS: usize = 2048;
const MAX_OVERLAY_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
// Held decoded sources are already shrunk to MAX_TILE; bound their total too.
const MAX_OVERLAY_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const DIAGNOSTIC_MATERIAL: u32 = 0;

/// Counted reasons some server block visuals are incomplete this session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OverlayGaps {
    pub(crate) missing_textures: u32,
    pub(crate) missing_geometry: u32,
    pub(crate) unevaluated_permutations: u32,
    pub(crate) skipped_cubes: u32,
    pub(crate) truncated_models: u32,
    pub(crate) approximated_materials: u32,
    pub(crate) incomplete_state_identities: u32,
    pub(crate) invalid_legacy_bindings: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledBlockOverlay {
    pub(crate) overlay: BlockOverlay,
    pub(crate) gaps: OverlayGaps,
}

/// Returns visuals for every custom block state (palette order, or
/// `hashed_states` order for hashed ids), or `None` without custom blocks.
pub(super) fn compile_block_overlay(
    view: &LayeredPackView,
    blocks: &CustomBlocks,
    hashed: bool,
    vanilla_keys: Option<&MaterialKeys>,
) -> Option<CompiledBlockOverlay> {
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
    let mut builder = Builder {
        catalog: TextureCatalog::new(view, vanilla_keys),
        geometries: geometry_catalog(view, &wanted),
        overlay: BlockOverlay::default(),
        sources: Vec::new(),
        source_bytes: 0,
        textures: HashMap::new(),
        materials: HashMap::new(),
        visuals: HashMap::new(),
        gaps: OverlayGaps {
            invalid_legacy_bindings: legacy.invalid,
            ..OverlayGaps::default()
        },
    };
    builder.sources.push(Source::Diagnostic);
    builder.overlay.materials.push(Material {
        texture: TextureRef::new(1, 0).expect("layer zero"),
        flags: 0,
        animation: NO_ANIMATION,
        ..assets::Material::unvaried()
    });
    for block in blocks.blocks.iter() {
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
                visual.components.random_offset =
                    block.physics_for_state(index as u32).random_offset;
                legacy.apply(&block.name, &mut visual.components);
                builder.push_state(&visual);
                builder.overlay.hashes.push(Some(state.hash));
            }
            continue;
        }
        for state in 0..block.state_count {
            let values = block.state_values(state);
            let mut visual =
                condition::state_visual(block, &expressions, values.as_deref(), &mut builder.gaps);
            visual.components.random_offset = block.physics_for_state(state).random_offset;
            legacy.apply(&block.name, &mut visual.components);
            builder.push_state(&visual);
            builder
                .overlay
                .hashes
                .push(identities_complete.then(|| states[state as usize].hash));
        }
    }
    if let Some(keys) = vanilla_keys {
        builder.override_vanilla_materials(keys);
    }
    if blocks.blocks.is_empty() && builder.overlay.material_overrides.is_empty() {
        return None;
    }
    builder.finish()
}

enum Source {
    Diagnostic,
    Image(DecodedTexture),
    GridImage(DecodedTexture, u8),
}

#[derive(Clone)]
struct TextureSlot {
    layer: u32,
    animation: u32,
}

struct Builder<'a> {
    catalog: TextureCatalog<'a>,
    geometries: HashMap<String, Geometry>,
    overlay: BlockOverlay,
    sources: Vec<Source>,
    source_bytes: usize,
    textures: HashMap<String, Option<TextureSlot>>,
    materials: HashMap<(String, u32), u32>,
    visuals: HashMap<String, BlockVisual>,
    gaps: OverlayGaps,
}

impl Builder<'_> {
    /// Repoints every base material whose terrain key the pack redefines.
    fn override_vanilla_materials(&mut self, keys: &MaterialKeys) {
        let mut candidates = self
            .catalog
            .terrain_keys()
            .filter(|key| !keys.materials(key).is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        let mut claimed = HashSet::new();
        for key in candidates {
            let Some(slot) = self.texture_slot(&key) else {
                continue;
            };
            for &material in keys.materials(&key) {
                if material != DIAGNOSTIC_MATERIAL && claimed.insert(material) {
                    self.overlay.material_overrides.push(MaterialOverride {
                        material,
                        texture: TextureRef::new(1, slot.layer).expect("bounded layer"),
                        animation: slot.animation,
                    });
                }
            }
        }
    }

    fn push_state(&mut self, state: &StateVisual) {
        let light = state_light(&state.components);
        let visual = self.visual(state);
        self.overlay.visuals.push(visual);
        self.overlay.light_properties.push(light);
    }

    fn visual(&mut self, state: &StateVisual) -> BlockVisual {
        let components = &state.components;
        let key = format!("{components:?}{:?}", state.hidden_bones);
        if let Some(visual) = self.visuals.get(&key) {
            return *visual;
        }
        let visual = match components.geometry.as_deref() {
            None | Some(FULL_BLOCK | FULL_BLOCK_V1) if components.random_offset.is_none() => {
                self.cube(components)
            }
            None | Some(FULL_BLOCK | FULL_BLOCK_V1) => {
                self.model(components, &geometry::Geometry::full_block(), &[])
            }
            Some(identifier) => match self.geometries.get(identifier).cloned() {
                Some(geometry) => self.model(components, &geometry, &state.hidden_bones),
                None => {
                    self.gaps.missing_geometry += 1;
                    diagnostic_visual()
                }
            },
        };
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
                render::MATERIAL_UV_ROTATE_180
            } else {
                0
            };
            let (material, flags, _) =
                self.face_material(components, FACE_NAMES[source], None, true, uv_flags);
            opaque &= flags == 0;
            *slot = material;
        }
        if faces.contains(&DIAGNOSTIC_MATERIAL) {
            self.gaps.missing_textures += 1;
            return diagnostic_visual();
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
        }
    }

    fn model(
        &mut self,
        components: &CustomVisualComponents,
        geometry: &Geometry,
        hidden_bones: &[Arc<str>],
    ) -> BlockVisual {
        self.gaps.skipped_cubes += geometry.skipped_cubes;
        let mut quads = Vec::new();
        let mut first_material = None;
        let shown = geometry
            .cubes
            .iter()
            .filter(|cube| !hidden_bones.iter().any(|bone| **bone == *cube.bone))
            .collect::<Vec<_>>();
        if shown.is_empty() && !geometry.cubes.is_empty() {
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
                if material == DIAGNOSTIC_MATERIAL {
                    self.gaps.missing_textures += 1;
                    return diagnostic_visual();
                }
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
            faces: [material; 6],
            flags: BlockFlags::empty(),
            kind: VisualKind::Model,
            support: VisualSupport::VanillaFallback,
            contributor_role: ContributorRole::Primary,
            model_template: template,
            animation: NO_ANIMATION,
            variant: 0,
        }
    }

    /// Resolves a face's material instance: an explicit geometry name, then the
    /// face, then `side` for horizontal faces, then `*`.
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
        let Some(slot) = self.texture_slot(&texture) else {
            return (DIAGNOSTIC_MATERIAL, 0, false);
        };
        let material = self.overlay.materials.len() as u32;
        self.overlay.materials.push(Material {
            texture: TextureRef::new(1, slot.layer).expect("bounded layer"),
            flags,
            animation: slot.animation,
            ..assets::Material::unvaried()
        });
        self.materials.insert((texture, flags), material);
        (material, alpha_flags, two_sided)
    }

    fn texture_slot(&mut self, key: &str) -> Option<TextureSlot> {
        if let Some(slot) = self.textures.get(key) {
            return slot.clone();
        }
        let slot = self.load_texture(key);
        self.textures.insert(key.to_owned(), slot.clone());
        slot
    }

    fn load_texture(&mut self, key: &str) -> Option<TextureSlot> {
        let available = MAX_OVERLAY_LAYERS.saturating_sub(self.sources.len());
        if available == 0 {
            return None;
        }
        let mut texture = self.catalog.decode(key)?;
        let grid = self.catalog.grid(key);
        // Bound frame count before cutting copies, and shrink each to MAX_TILE.
        let frames = match self.catalog.flipbook(key) {
            Some(flipbook) => flipbook_frames(&texture, flipbook, available.min(256), MAX_TILE),
            None => {
                admit_static_rectangle(&mut texture);
                vec![shrink_to_max(&texture, MAX_TILE)]
            }
        };
        drop(texture);
        if frames
            .iter()
            .any(|frame| frame.width >> grid == 0 || frame.height >> grid == 0)
        {
            return None;
        }
        let added_bytes: usize = frames.iter().map(|frame| frame.rgba8.len()).sum();
        if frames.is_empty() || self.source_bytes + added_bytes > MAX_OVERLAY_SOURCE_BYTES {
            return None;
        }
        self.source_bytes += added_bytes;
        let first = self.sources.len() as u32;
        let frame_count = frames.len() as u32;
        self.sources.extend(frames.into_iter().map(|frame| {
            if grid == 0 {
                Source::Image(frame)
            } else {
                Source::GridImage(frame, grid)
            }
        }));
        let animation = match self.catalog.flipbook(key) {
            Some(flipbook) if frame_count > 1 => {
                let frame_start = self.overlay.animation_frames.len() as u32;
                for layer in first..first + frame_count {
                    self.overlay
                        .animation_frames
                        .push(TextureRef::new(1, layer).expect("bounded layer"));
                }
                self.overlay.animations.push(Animation {
                    frame_start,
                    frame_count,
                    ticks_per_frame: flipbook.ticks_per_frame,
                    atlas_index: 0,
                    atlas_tile_variant: 0,
                    replicate: 1,
                    flags: u32::from(flipbook.blend) * assets::ANIMATION_FLAG_BLEND,
                });
                self.overlay.animations.len() as u32 - 1
            }
            _ => NO_ANIMATION,
        };
        Some(TextureSlot {
            layer: first,
            animation,
        })
    }

    fn finish(mut self) -> Option<CompiledBlockOverlay> {
        for source in &mut self.sources {
            if let Source::Image(texture) | Source::GridImage(texture, _) = source {
                admit_static_rectangle(texture);
            }
        }
        let largest = self
            .sources
            .iter()
            .filter_map(|source| match source {
                Source::Image(texture) | Source::GridImage(texture, _) => {
                    Some(texture.width.max(texture.height))
                }
                Source::Diagnostic => None,
            })
            .max()
            .unwrap_or(MIN_TILE);
        let mut tile = largest.next_power_of_two().clamp(MIN_TILE, MAX_TILE);
        // A full mip chain costs about 4/3 of its base level.
        let page_bytes = |tile: u32| self.sources.len() * (tile * tile) as usize * 16 / 3;
        while tile > MIN_TILE && page_bytes(tile) > MAX_OVERLAY_TEXTURE_BYTES {
            tile /= 2;
        }
        let mut mips: Vec<Vec<u8>> = Vec::new();
        for source in &self.sources {
            let (dimensions, chain) = match source {
                Source::Diagnostic => (
                    [tile as u16; 2],
                    assets::build_legacy_terrain_mip_chain(&diagnostic_pixels(tile), tile).ok()?,
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
            let dimensions = dimensions.map(|size| size >> grid);
            if dimensions.contains(&0) {
                return None;
            }
            self.overlay.texture_source_sizes.push(dimensions);
            self.overlay.texture_source_grids.push(grid);
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
            .collect();
        self.overlay.texture = Some(TextureArray {
            layers: self.sources.len() as u32,
            mips,
        });
        Some(CompiledBlockOverlay {
            overlay: self.overlay,
            gaps: self.gaps,
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

/// A model whose bone visibility hides every cube draws nothing.
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
            normal = rotate_quarter(normal, axis);
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

/// One right-handed quarter turn about `axis`; +90 degrees about Y turns north to west.
fn rotate_quarter<T: Copy + std::ops::Neg<Output = T>>(value: [T; 3], axis: usize) -> [T; 3] {
    let [x, y, z] = value;
    match axis {
        0 => [x, -z, y],
        1 => [z, y, -x],
        _ => [-y, x, z],
    }
}

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
            for (value, scale) in point.iter_mut().zip(transform.scale) {
                *value *= scale;
            }
            for (axis, turns) in transform.rotation.into_iter().enumerate() {
                for _ in 0..turns.rem_euclid(4) {
                    point = rotate_quarter(point, axis);
                }
            }
            for (value, offset) in point.iter_mut().zip(transform.translation) {
                *value += offset * 16.0;
            }
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
    let on_boundary = positions
        .iter()
        .all(|corner| i32::from(corner[axis]) == boundary);
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
