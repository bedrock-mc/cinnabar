//! Per-session server block visuals appended after the vanilla carrier.

use std::sync::atomic::AtomicU64;

use super::RuntimeAssets;
use crate::{
    Animation, AssetError, BlockFlags, BlockVisual, LightProperties, MAX_MATERIALS,
    MAX_TEXTURE_LAYERS, MAX_TILE_SIZE, Material, ModelQuad, ModelTemplate, NO_ANIMATION,
    NO_MODEL_TEMPLATE, TextureArray, TexturePage, TextureRef, VisualKind,
    compiled::material_flags_are_valid, compiled::visual_semantics_are_valid,
    model::model_quad_flags_are_valid,
};

/// Repoints a base material at an overlay texture, keeping its flags (tint, alpha).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterialOverride {
    /// Index into the base carrier's material table; never the diagnostic material.
    pub material: u32,
    pub texture: TextureRef,
    /// Overlay-local animation index, or `NO_ANIMATION`.
    pub animation: u32,
}

/// Server block visuals whose indices are local to the overlay: face and quad
/// material ids index `materials`, templates index `model_templates`, and
/// texture references use page 1 layers of `texture`.
#[derive(Clone, Debug, Default)]
pub struct BlockOverlay {
    pub visuals: Vec<BlockVisual>,
    pub light_properties: Vec<LightProperties>,
    pub materials: Vec<Material>,
    pub model_templates: Vec<ModelTemplate>,
    /// Sparse, overlay-local template components; no carrier-format change.
    pub model_random_offsets: Vec<(u32, block_transform::random_offset::RandomOffsetComponent)>,
    pub model_quads: Vec<ModelQuad>,
    pub animations: Vec<Animation>,
    pub animation_frames: Vec<TextureRef>,
    pub texture: Option<TextureArray>,
    /// Canonical network hashes parallel to `visuals`; incomplete state identities are absent.
    pub hashes: Vec<Option<u32>>,
    pub material_overrides: Vec<MaterialOverride>,
    /// Optional pack-defined tint maps and biome appearance rules.
    pub biomes: Option<crate::CompiledBiomeAssets>,
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
        let layers = overlay.texture.as_ref().map_or(0, |texture| texture.layers);
        if layers as usize > MAX_TEXTURE_LAYERS {
            return Err(invalid("overlay texture page exceeds the layer limit"));
        }
        if let Some(texture) = &overlay.texture {
            validate_texture(texture)?;
        }
        let material_base = offset(self.materials.len())?;
        let template_base = offset(self.model_templates.len())?;
        let quad_base = offset(self.model_quads.len())?;
        let animation_base = offset(self.animations.len())?;
        let frame_base = offset(self.animation_frames.len())?;
        if self.materials.len() + overlay.materials.len() > MAX_MATERIALS {
            return Err(invalid("overlay materials exceed the material limit"));
        }
        let page_ref = |reference: TextureRef| -> Result<TextureRef, AssetError> {
            if reference.page() != 1 || reference.layer() >= layers {
                return Err(invalid("overlay texture reference is outside its page"));
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

        let mut materials = self.materials.to_vec();
        crate::material_variations::validate(&overlay.materials)?;
        for material in &overlay.materials {
            if !material_flags_are_valid(material.flags) {
                return Err(invalid("overlay material flags are invalid"));
            }
            materials.push(Material {
                texture: page_ref(material.texture)?,
                flags: material.flags,
                animation: optional(
                    material.animation,
                    overlay.animations.len(),
                    animation_base,
                    "animation",
                )?,
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
            materials[replacement.material as usize] = Material {
                texture: page_ref(replacement.texture)?,
                variation_start: 0,
                variation_count: 0,
                animation: optional(
                    replacement.animation,
                    overlay.animations.len(),
                    animation_base,
                    "animation",
                )?,
                ..materials[replacement.material as usize]
            };
        }
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
        for template in &overlay.model_templates {
            if template.quad_start as usize != covered
                || template.quad_count as usize > crate::MAX_MODEL_TEMPLATE_QUADS
                || !matches!(template.flags, 0 | crate::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT)
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
        let mut visuals = self.visuals.to_vec();
        for visual in &overlay.visuals {
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
        let mut hashed = self.hashed.to_vec();
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
        if let Some(texture) = &overlay.texture {
            texture_pages.push(TexturePage::new(texture.clone()));
        }
        Ok(Self {
            visuals: visuals.into_boxed_slice(),
            light_properties: light_properties.into_boxed_slice(),
            hashed: hashed.into_boxed_slice(),
            materials: materials.into_boxed_slice(),
            model_templates: model_templates.into_boxed_slice(),
            model_random_offsets: model_random_offsets.into_boxed_slice(),
            model_quads: model_quads.into_boxed_slice(),
            animations: animations.into_boxed_slice(),
            animation_frames: animation_frames.into_boxed_slice(),
            texture_pages: texture_pages.into_boxed_slice(),
            biomes: overlay
                .biomes
                .clone()
                .unwrap_or_else(|| self.biomes.clone()),
            provenance: self.provenance,
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
mod tests {
    use super::*;
    use crate::{BlockFlags, ContributorRole, NetworkIdMode, TextureMip, VisualSupport};

    fn page(size: u32) -> TextureArray {
        let mut mips = Vec::new();
        let mut side = size;
        while side > 0 {
            mips.push(TextureMip {
                size: side,
                rgba8: vec![255; (side * side * 4) as usize].into(),
            });
            side /= 2;
        }
        TextureArray {
            layers: 1,
            mips: mips.into(),
        }
    }

    #[test]
    fn review_zero_sized_extra_mips_are_rejected() {
        let mut texture = page(16);
        let mut mips = texture.mips.to_vec();
        mips.push(TextureMip {
            size: 0,
            rgba8: Box::new([]),
        });
        texture.mips = mips.into();
        assert!(
            RuntimeAssets::diagnostic()
                .with_block_overlay(1, &cube_overlay(texture))
                .is_err()
        );
    }

    #[test]
    fn review_full_face_occlusion_requires_drawable_geometry() {
        let mut overlay = cube_overlay(page(16));
        overlay.visuals[0].kind = VisualKind::Model;
        overlay.visuals[0].flags = BlockFlags::OCCLUDES_FULL_FACE;
        overlay.visuals[0].model_template = 0;
        overlay.model_templates = vec![ModelTemplate {
            quad_start: 0,
            quad_count: 0,
            flags: 0,
        }];
        assert!(
            RuntimeAssets::diagnostic()
                .with_block_overlay(1, &overlay)
                .is_err()
        );
    }

    fn cube_overlay(texture: TextureArray) -> BlockOverlay {
        BlockOverlay {
            visuals: vec![BlockVisual {
                faces: [0; 6],
                flags: BlockFlags::CUBE_GEOMETRY,
                kind: VisualKind::Cube,
                support: VisualSupport::VanillaFallback,
                contributor_role: ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            }],
            light_properties: vec![LightProperties::OPAQUE_DARK],
            materials: vec![Material {
                texture: TextureRef::new(1, 0).unwrap(),
                flags: 0,
                animation: NO_ANIMATION,
                ..crate::Material::unvaried()
            }],
            texture: Some(texture),
            ..BlockOverlay::default()
        }
    }

    // Overlay hashes resolve to the overlay's ids and never shadow a base hash.
    #[test]
    fn overlay_hashes_extend_the_hash_table() {
        let base = RuntimeAssets::diagnostic();
        let mut overlay = cube_overlay(page(16));
        overlay.hashes = vec![Some(0xdead_beef)];
        let session = base.with_block_overlay(1, &overlay).unwrap();
        assert_eq!(session.sequential_id_for_hash(0xdead_beef), Some(1));
        overlay.hashes = vec![Some(1), Some(2)];
        assert!(base.with_block_overlay(1, &overlay).is_err());
    }

    // A base material can be repointed at the overlay page; the diagnostic material cannot.
    #[test]
    fn material_overrides_repoint_base_materials() {
        let base = RuntimeAssets::diagnostic();
        let mut overlay = cube_overlay(page(16));
        overlay.material_overrides = vec![MaterialOverride {
            material: 0,
            texture: TextureRef::new(1, 0).unwrap(),
            animation: NO_ANIMATION,
        }];
        assert!(base.with_block_overlay(1, &overlay).is_err());
    }

    #[test]
    fn replacing_a_selector_uses_the_server_texture_without_changing_leaf_weights() {
        let mut base = RuntimeAssets::diagnostic();
        base.materials = vec![
            Material::unvaried(),
            Material {
                variation_start: 2,
                variation_count: 1,
                ..Material::unvaried()
            },
            Material {
                variation_weight: 1.0_f32.to_bits(),
                ..Material::unvaried()
            },
        ]
        .into();
        let mut overlay = cube_overlay(page(16));
        overlay.material_overrides = vec![MaterialOverride {
            material: 1,
            texture: TextureRef::new(1, 0).unwrap(),
            animation: NO_ANIMATION,
        }];
        let session = base.with_block_overlay(1, &overlay).unwrap();
        assert_eq!(session.materials[1].variation_count, 0);
        assert_eq!(session.materials[1].texture.page(), 1);
        assert_eq!(session.materials[2].variation_weight, 1.0_f32.to_bits());
    }

    // A 32px page is accepted; malformed mips or dangling ids are refused whole.
    #[test]
    fn overlay_page_and_references_are_validated() {
        let base = RuntimeAssets::diagnostic();
        let session = base.with_block_overlay(1, &cube_overlay(page(32))).unwrap();
        let block = session.resolve(NetworkIdMode::Sequential, 1);
        assert_eq!(
            session
                .material(block.face(crate::BlockFace::Up).material_id())
                .texture
                .page(),
            1
        );

        let mut truncated = page(32);
        truncated.mips = truncated.mips[..3].into();
        assert!(
            base.with_block_overlay(1, &cube_overlay(truncated))
                .is_err()
        );
        let mut dangling = cube_overlay(page(16));
        dangling.visuals[0].faces[2] = 5;
        assert!(base.with_block_overlay(1, &dangling).is_err());
    }
}
