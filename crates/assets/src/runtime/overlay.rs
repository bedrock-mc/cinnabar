//! Per-session server block visuals appended after the vanilla carrier.

use std::sync::atomic::AtomicU64;

use super::RuntimeAssets;
use crate::{
    Animation, AssetError, BlockFlags, BlockVisual, LightProperties, MAX_MATERIALS,
    MAX_TEXTURE_LAYERS, MAX_TEXTURE_PAGES, MAX_TILE_SIZE, Material, ModelQuad, ModelTemplate,
    NO_ANIMATION, NO_MODEL_TEMPLATE, TextureArray, TexturePage, TextureRef, VisualKind,
    compiled::material_flags_are_valid, compiled::visual_semantics_are_valid,
    material_keys::MaterialKeys, model::model_quad_flags_are_valid,
};

/// Repoints a base material at an overlay texture, keeping its flags and variation range.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MaterialOverride {
    /// Index into the base carrier's material table; never the diagnostic material.
    pub material: u32,
    pub texture: TextureRef,
    /// Overlay-local animation index, or `NO_ANIMATION`.
    pub animation: u32,
    /// Overlay-local positional range; `None` preserves the base selector and weight.
    pub variation: Option<(u32, u32)>,
}

/// Server block visuals whose indices are local to the overlay: face and quad
/// material ids index `materials`, templates index `model_templates`, and
/// texture references use overlay shards on higher pages, or static vanilla-fallback
/// materials on the base carrier's page zero.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockOverlay {
    pub visuals: Vec<BlockVisual>,
    /// Carried unit-cube faces keyed by sorted session visual ids; independent of world offsets.
    pub carried_cube_faces: Vec<(u32, [u32; 6])>,
    pub light_properties: Vec<LightProperties>,
    pub materials: Vec<Material>,
    pub model_templates: Vec<ModelTemplate>,
    /// Sparse, overlay-local template components; no carrier-format change.
    pub model_random_offsets: Vec<(u32, block_transform::random_offset::RandomOffsetComponent)>,
    pub model_quads: Vec<ModelQuad>,
    pub animations: Vec<Animation>,
    pub animation_frames: Vec<TextureRef>,
    /// Overlay texture shards in reference page order, page zero excluded. A
    /// large server palette splits across these to stay inside the device's
    /// per-array layer limit; a single shard is the usual case.
    pub textures: Vec<TextureArray>,
    /// Source pixel dimensions per flat layer. Empty means the physical page
    /// size and legacy terrain mips are rebuilt at upload.
    pub texture_source_sizes: Vec<[u16; 2]>,
    /// Per-layer terrain UV-grid exponents, parallel to source pixel dimensions.
    pub texture_source_grids: Vec<u8>,
    /// Canonical network hashes parallel to `visuals`; incomplete state identities are absent.
    pub hashes: Vec<Option<u32>>,
    pub material_overrides: Vec<MaterialOverride>,
    /// Base visual faces rebound to overlay-local materials; absent faces retain their base.
    pub face_overrides: Vec<(u32, [Option<u32>; 6])>,
    /// Optional pack-defined tint maps and biome appearance rules.
    pub biomes: Option<crate::CompiledBiomeAssets>,
    /// The per-array layer limit the shards were built against, so a carrier
    /// laid out for one adapter never reaches a smaller one.
    pub max_page_layers: u32,
}

/// A hand-built overlay with no device restriction inherits the format
/// reference; only a compiled session overlay records a smaller device limit.
impl Default for BlockOverlay {
    fn default() -> Self {
        Self {
            visuals: Vec::new(),
            carried_cube_faces: Vec::new(),
            light_properties: Vec::new(),
            materials: Vec::new(),
            model_templates: Vec::new(),
            model_random_offsets: Vec::new(),
            model_quads: Vec::new(),
            animations: Vec::new(),
            animation_frames: Vec::new(),
            textures: Vec::new(),
            texture_source_sizes: Vec::new(),
            texture_source_grids: Vec::new(),
            hashes: Vec::new(),
            material_overrides: Vec::new(),
            face_overrides: Vec::new(),
            biomes: None,
            max_page_layers: MAX_TEXTURE_LAYERS as u32,
        }
    }
}

/// Resolves a vanilla terrain key to its base-carrier texture for server
/// blocks that name vanilla art the server stack does not ship. The texture
/// is borrowed from the base page, so fallback admits no overlay layer and
/// consumes no texture budget; only static textures resolve, animated base
/// art keeps its motion on the base blocks alone.
#[derive(Clone, Copy, Debug)]
pub struct VanillaFallback<'a> {
    keys: &'a MaterialKeys,
    materials: &'a [Material],
}

impl<'a> VanillaFallback<'a> {
    /// Borrows matching texture keys and materials from the same base carrier.
    #[must_use]
    pub const fn new(keys: &'a MaterialKeys, materials: &'a [Material]) -> Self {
        Self { keys, materials }
    }

    /// Returns the static variant-zero base reference for `key`, or `None`
    /// when no donor has the requested tint already baked into its pixels.
    #[must_use]
    pub fn texture(&self, key: &str, tint: Option<[u8; 3]>) -> Option<TextureRef> {
        let &id = self.keys.materials(key).iter().find(|&&id| {
            self.keys.flipbook_selector(id) == (0, 0)
                && self
                    .keys
                    .material_source(id)
                    .and_then(|source| source.baked_tint)
                    == tint
        })?;
        if id == crate::DIAGNOSTIC_MATERIAL {
            return None;
        }
        let material = self.materials.get(id as usize)?;
        (material.texture.page() == 0
            && material.animation == NO_ANIMATION
            && material.variation_count == 0)
            .then_some(material.texture)
    }
}

impl RuntimeAssets {
    /// Returns a copy of these assets whose sequential ids from `first_id` on
    /// resolve to the overlay. The base carrier must use only page 0 and end
    /// exactly at `first_id`, so overlay ids never shadow vanilla ids.
    pub fn with_block_overlay(
        &self,
        first_id: u32,
        overlay: &BlockOverlay,
    ) -> Result<Self, AssetError> {
        if let Some(biomes) = &overlay.biomes {
            crate::biome::validate_biome_assets(biomes)?;
        }
        if self.visuals.len() != first_id as usize {
            return Err(invalid("overlay ids do not start after the base visuals"));
        }
        if self.texture_pages.len() != 1 {
            return Err(invalid("overlay requires a single-page base carrier"));
        }
        if overlay.visuals.len() != overlay.light_properties.len() {
            return Err(invalid("overlay visuals and light properties disagree"));
        }
        if !overlay.hashes.is_empty() && overlay.hashes.len() != overlay.visuals.len() {
            return Err(invalid("overlay hashes and visuals disagree"));
        }
        let shards = overlay.textures.len();
        if shards + 1 > MAX_TEXTURE_PAGES {
            return Err(invalid("overlay texture shards are not bounded"));
        }
        let max_page_layers = overlay.max_page_layers;
        if max_page_layers == 0 {
            return Err(invalid("overlay shards were built against no array limit"));
        }
        let shard_layers = |shard: &TextureArray| -> Result<usize, AssetError> {
            let layers = shard.layers as usize;
            if layers == 0 || layers as u32 > max_page_layers || layers > MAX_TEXTURE_LAYERS {
                return Err(invalid(
                    "overlay texture shard exceeds the device or format limit",
                ));
            }
            Ok(layers)
        };
        let mut flat_layers = 0usize;
        for shard in &overlay.textures {
            let layers = shard_layers(shard)?;
            validate_texture(shard)?;
            flat_layers += layers;
        }
        let layer_bases = || {
            overlay
                .textures
                .iter()
                .flat_map(|shard| std::iter::repeat_n(shard.mips[0].size, shard.layers as usize))
        };
        if !overlay.texture_source_sizes.is_empty()
            && (overlay.texture_source_sizes.len() != flat_layers
                || overlay
                    .texture_source_sizes
                    .iter()
                    .zip(layer_bases())
                    .any(|(size, base)| {
                        size.iter()
                            .any(|&axis| !axis.is_power_of_two() || u32::from(axis) > base)
                    }))
        {
            return Err(invalid("overlay source pixel dimensions are invalid"));
        }
        if !overlay.texture_source_grids.is_empty()
            && (overlay.texture_source_grids.len() != flat_layers
                || overlay.texture_source_sizes.len() != flat_layers
                || overlay
                    .texture_source_grids
                    .iter()
                    .zip(&overlay.texture_source_sizes)
                    .zip(layer_bases())
                    .any(|((&grid, size), base)| {
                        u32::from(grid) > crate::TERRAIN_QUAD_SHIFT_MASK
                            || size.iter().any(|&axis| {
                                u32::from(axis)
                                    .checked_mul(1u32 << grid)
                                    .is_none_or(|full| full > base)
                            })
                    }))
        {
            return Err(invalid("overlay source UV grids are invalid"));
        }
        let material_base = offset(self.materials.len())?;
        let template_base = offset(self.model_templates.len())?;
        let quad_base = offset(self.model_quads.len())?;
        let animation_base = offset(self.animations.len())?;
        let frame_base = offset(self.animation_frames.len())?;
        if self.materials.len() + overlay.materials.len() > MAX_MATERIALS {
            return Err(invalid("overlay materials exceed the material limit"));
        }
        let base_layers = self
            .texture_pages
            .first()
            .map_or(0, |page| page.texture.layers);
        let page_ref = |reference: TextureRef| -> Result<TextureRef, AssetError> {
            let shard = reference
                .page()
                .checked_sub(1)
                .filter(|shard| (*shard as usize) < shards)
                .ok_or_else(|| invalid("overlay texture reference is outside its page"))?;
            if reference.layer() >= overlay.textures[shard as usize].layers {
                return Err(invalid("overlay texture reference is outside its shard"));
            }
            Ok(reference)
        };
        let local = |id: u32, len: usize, base: u32, what: &str| -> Result<u32, AssetError> {
            if id as usize >= len {
                return Err(invalid(format!("overlay {what} reference is out of range")));
            }
            Ok(base + id)
        };
        // Templates and animations may be absent; materials never are.
        let optional = |id: u32, len: usize, base: u32, what: &str| {
            if id == u32::MAX {
                Ok(id)
            } else {
                local(id, len, base, what)
            }
        };
        // Vanilla-fallback materials borrow a static base-carrier layer, so
        // they validate against the base page and carry no overlay animation.
        let material_ref = |material: &Material| -> Result<(TextureRef, u32), AssetError> {
            if material.texture.page() == 0 {
                if material.texture.layer() >= base_layers || material.animation != NO_ANIMATION {
                    return Err(invalid("overlay base texture reference is invalid"));
                }
                return Ok((material.texture, NO_ANIMATION));
            }
            Ok((
                page_ref(material.texture)?,
                optional(
                    material.animation,
                    overlay.animations.len(),
                    animation_base,
                    "animation",
                )?,
            ))
        };

        let mut materials = self.materials.to_vec();
        crate::material_variations::validate(&overlay.materials)?;
        for material in &overlay.materials {
            if !material_flags_are_valid(material.flags) {
                return Err(invalid("overlay material flags are invalid"));
            }
            let (texture, animation) = material_ref(material)?;
            materials.push(Material {
                texture,
                flags: material.flags,
                animation,
                variation_start: if material.variation_count == 0 {
                    0
                } else {
                    material_base + material.variation_start
                },
                variation_count: material.variation_count,
                variation_weight: material.variation_weight,
            });
        }
        for replacement in &overlay.material_overrides {
            if replacement.material == 0 || replacement.material >= material_base {
                return Err(invalid(
                    "overlay material override is outside the base table",
                ));
            }
            if let Some((start, count)) = replacement.variation {
                let end = (start as usize)
                    .checked_add(count as usize)
                    .ok_or_else(|| invalid("overlay variation range overflows"))?;
                if (count == 0 && start != 0) || end > overlay.materials.len() {
                    return Err(invalid(
                        "overlay variation range is outside its material table",
                    ));
                }
            }
            materials[replacement.material as usize] = Material {
                texture: page_ref(replacement.texture)?,
                variation_start: match replacement.variation {
                    Some((_, 0)) => 0,
                    Some((start, _)) => material_base + start,
                    None => materials[replacement.material as usize].variation_start,
                },
                variation_count: replacement.variation.map_or(
                    materials[replacement.material as usize].variation_count,
                    |(_, count)| count,
                ),
                animation: optional(
                    replacement.animation,
                    overlay.animations.len(),
                    animation_base,
                    "animation",
                )?,
                ..materials[replacement.material as usize]
            };
        }
        crate::material_variations::validate(&materials)?;
        let mut animation_frames = self.animation_frames.to_vec();
        for &frame in &overlay.animation_frames {
            animation_frames.push(page_ref(frame)?);
        }
        let mut animations = self.animations.to_vec();
        for animation in &overlay.animations {
            let end = animation.frame_start as usize + animation.frame_count as usize;
            if animation.frame_count == 0
                || animation.ticks_per_frame == 0
                || animation.replicate == 0
                || end > overlay.animation_frames.len()
            {
                return Err(invalid("overlay animation is noncanonical"));
            }
            animations.push(Animation {
                frame_start: frame_base + animation.frame_start,
                ..*animation
            });
        }
        let mut model_quads = self.model_quads.to_vec();
        for quad in &overlay.model_quads {
            if !model_quad_flags_are_valid(quad.flags) {
                return Err(invalid("overlay model quad flags are invalid"));
            }
            model_quads.push(ModelQuad {
                material: local(
                    quad.material,
                    overlay.materials.len(),
                    material_base,
                    "quad",
                )?,
                ..*quad
            });
        }
        let mut model_random_offsets = self.model_random_offsets.to_vec();
        let mut seen = std::collections::HashSet::new();
        for &(template, component) in &overlay.model_random_offsets {
            if !component.is_valid() || !seen.insert(template) {
                return Err(invalid(
                    "overlay random-offset component is invalid or duplicated",
                ));
            }
            let template = local(
                template,
                overlay.model_templates.len(),
                template_base,
                "offset template",
            )?;
            model_random_offsets.push((template, component));
        }
        model_random_offsets.sort_unstable_by_key(|entry| entry.0);
        let mut model_templates = self.model_templates.to_vec();
        let compound_tails = crate::blob::compiled_compound_tails(&overlay.model_templates)?;
        let mut covered = 0usize;
        for (index, template) in overlay.model_templates.iter().enumerate() {
            if template.flags == crate::MODEL_TEMPLATE_FLAG_POTTED_PLANT
                && (template.quad_count == 0
                    || overlay
                        .model_templates
                        .get(index + 1)
                        .is_none_or(|next| next.flags != 0))
            {
                return Err(invalid(
                    "overlay pot-content template has no standalone world model",
                ));
            }
            if template.quad_start as usize != covered
                || template.quad_count as usize > crate::MAX_MODEL_TEMPLATE_QUADS
                || !matches!(
                    template.flags,
                    0 | crate::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
                        | crate::MODEL_TEMPLATE_FLAG_POTTED_PLANT
                )
            {
                return Err(invalid("overlay template spans are noncanonical"));
            }
            covered += template.quad_count as usize;
            model_templates.push(ModelTemplate {
                quad_start: quad_base + template.quad_start,
                ..*template
            });
        }
        if covered != overlay.model_quads.len() {
            return Err(invalid("overlay templates do not cover quads"));
        }
        let mut previous_carried = None;
        for &(index, faces) in &overlay.carried_cube_faces {
            let visual = overlay
                .visuals
                .get(index as usize)
                .ok_or_else(|| invalid("carried cube visual is out of bounds"))?;
            let template = overlay
                .model_templates
                .get(visual.model_template as usize)
                .ok_or_else(|| invalid("carried cube has no model template"))?;
            if previous_carried.is_some_and(|previous| previous >= index)
                || visual.kind != VisualKind::Model
                || template.flags != 0
                || template.quad_count != 6
                || faces.iter().any(|&material| {
                    material == crate::DIAGNOSTIC_MATERIAL
                        || material as usize >= overlay.materials.len()
                })
                || overlay.model_quads[template.quad_start as usize..][..6]
                    .iter()
                    .enumerate()
                    .any(|(face, quad)| {
                        !crate::model::unit_cube_quad_geometry_is_valid(
                            face,
                            quad.positions,
                            quad.flags,
                        )
                    })
            {
                return Err(invalid("carried cube geometry or faces are invalid"));
            }
            previous_carried = Some(index);
        }
        let mut visuals = self.visuals.to_vec();
        super::face_overrides::apply(
            &overlay.face_overrides,
            &mut visuals,
            &materials,
            material_base..material_base + overlay.materials.len() as u32,
        )?;
        for visual in &overlay.visuals {
            if overlay
                .model_templates
                .get(visual.model_template as usize)
                .is_some_and(|template| template.flags == crate::MODEL_TEMPLATE_FLAG_POTTED_PLANT)
            {
                return Err(invalid("overlay visual references pot-content geometry"));
            }
            if visual.model_template != NO_MODEL_TEMPLATE
                && compound_tails.get(visual.model_template as usize).copied() == Some(true)
            {
                return Err(invalid("overlay visual references a compound continuation"));
            }
            if !visual.flags.has_valid_semantics()
                || !visual_semantics_are_valid(
                    visual.kind,
                    visual.support,
                    visual.flags,
                    visual.contributor_role,
                )
                || (visual.model_template == NO_MODEL_TEMPLATE)
                    == matches!(visual.kind, VisualKind::Model | VisualKind::Cross)
            {
                return Err(invalid("overlay visual semantics are invalid"));
            }
            if visual.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
                && !visual.flags.contains(BlockFlags::CUBE_GEOMETRY)
                && (visual.kind != VisualKind::Model
                    || overlay
                        .model_templates
                        .get(visual.model_template as usize)
                        .is_none_or(|template| template.quad_count == 0))
            {
                return Err(invalid(
                    "overlay full-face occlusion requires a drawable model",
                ));
            }
            let mut faces = visual.faces;
            for face in &mut faces {
                *face = local(*face, overlay.materials.len(), material_base, "face")?;
            }
            visuals.push(BlockVisual {
                faces,
                model_template: optional(
                    visual.model_template,
                    overlay.model_templates.len(),
                    template_base,
                    "template",
                )?,
                animation: NO_ANIMATION,
                ..*visual
            });
        }
        let mut light_properties = self.light_properties.to_vec();
        light_properties.extend_from_slice(&overlay.light_properties);
        // A hash the base or an earlier overlay state already owns keeps its owner.
        let mut hashed = self.hashed.entries().to_vec();
        for (index, hash) in overlay.hashes.iter().copied().enumerate() {
            let Some(hash) = hash else {
                continue;
            };
            if self.sequential_id_for_hash(hash).is_none() {
                hashed.push((hash, first_id + index as u32));
            }
        }
        hashed.sort_by_key(|entry| entry.0);
        hashed.dedup_by_key(|entry| entry.0);
        let mut texture_pages = self.texture_pages.to_vec();
        let mut overlay_page_layer_counts = Vec::with_capacity(shards);
        for shard in &overlay.textures {
            overlay_page_layer_counts.push(shard.layers as usize);
            texture_pages.push(TexturePage::new(shard.clone()));
        }
        Ok(Self {
            visuals: visuals.into_boxed_slice(),
            light_properties: light_properties.into_boxed_slice(),
            hashed: super::hashed_ids::HashedIds::new(hashed.into_boxed_slice()),
            materials: materials.into_boxed_slice(),
            model_templates: model_templates.into_boxed_slice(),
            model_random_offsets: model_random_offsets.into_boxed_slice(),
            model_quads: model_quads.into_boxed_slice(),
            animations: animations.into_boxed_slice(),
            animation_frames: animation_frames.into_boxed_slice(),
            texture_pages: texture_pages.into_boxed_slice(),
            overlay_texture_source_sizes: overlay.texture_source_sizes.clone().into_boxed_slice(),
            overlay_texture_source_grids: overlay.texture_source_grids.clone().into_boxed_slice(),
            overlay_page_layer_counts: overlay_page_layer_counts.into_boxed_slice(),
            biomes: overlay
                .biomes
                .clone()
                .unwrap_or_else(|| self.biomes.clone()),
            provenance: self.provenance,
            carrier_identity: None,
            missing: AtomicU64::new(0),
        })
    }
}

/// Mips must halve from a square power-of-two base down to 1x1 with exact
/// layer-major byte lengths, as the chunk renderer uploads them unchecked.
fn validate_texture(texture: &TextureArray) -> Result<(), AssetError> {
    let base = texture.mips.first().map_or(0, |mip| mip.size);
    if texture.layers == 0 || !base.is_power_of_two() || base > MAX_TILE_SIZE {
        return Err(invalid("overlay texture page has an invalid base size"));
    }
    let mut size = base;
    for (level, mip) in texture.mips.iter().enumerate() {
        let expected = (size as usize)
            .checked_mul(size as usize * 4)
            .and_then(|bytes| bytes.checked_mul(texture.layers as usize));
        if size == 0 || mip.size != size || Some(mip.rgba8.len()) != expected {
            return Err(invalid(format!("overlay texture mip {level} is malformed")));
        }
        size /= 2;
    }
    if size != 0 {
        return Err(invalid("overlay texture page lacks a complete mip chain"));
    }
    Ok(())
}

fn offset(len: usize) -> Result<u32, AssetError> {
    u32::try_from(len).map_err(|_| invalid("overlay base table is too large"))
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fallback_tests;
