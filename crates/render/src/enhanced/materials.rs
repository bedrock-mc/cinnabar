//! Enhanced material classes: one GPU word per material id, derived from the
//! compiled block registry (the analogue of a shader pack's block table).

use assets::{
    MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_WATER_TINT,
    NetworkIdMode, RuntimeAssets, VisualKind,
};

/// Light-emission nibble shared by every block state that uses the material.
pub(crate) const CLASS_EMISSION_MASK: u32 = 0xf;
pub(crate) const CLASS_LEAVES: u32 = 1 << 4;
pub(crate) const CLASS_PLANT: u32 = 1 << 5;
pub(crate) const CLASS_WATER: u32 = 1 << 6;
pub(crate) const CLASS_LAVA: u32 = 1 << 7;

const BLOCK_FACES: [assets::BlockFace; 6] = assets::BlockFace::ALL;

/// Class word per material. A material is emissive only when every block
/// state referencing it emits, so shared side textures of lit blocks stay dark.
#[must_use]
pub(crate) fn material_classes(assets: &RuntimeAssets) -> Vec<u32> {
    let materials = assets.materials();
    let mut emission = vec![u8::MAX; materials.len()];
    let mut classes = vec![0_u32; materials.len()];
    let templates = assets.model_templates();
    let quads = assets.model_quads();
    let mut referenced = Vec::new();
    for sequential in 0..assets.visual_count() {
        let Ok(sequential) = u32::try_from(sequential) else {
            break;
        };
        let block = assets.resolve(NetworkIdMode::Sequential, sequential);
        if !block.is_known() {
            continue;
        }
        referenced.clear();
        referenced.extend(BLOCK_FACES.map(|face| block.face(face).material_id()));
        if let Some(template) = block
            .model_template()
            .and_then(|index| templates.get(index as usize))
        {
            let start = template.quad_start as usize;
            let end = start.saturating_add(template.quad_count as usize);
            referenced.extend(
                quads
                    .get(start..end.min(quads.len()))
                    .unwrap_or_default()
                    .iter()
                    .map(|quad| quad.material),
            );
        }
        let emits = block.light_properties().emission();
        for &material_id in &referenced {
            let Some(material) = materials.get(material_id as usize) else {
                continue;
            };
            let index = material_id as usize;
            emission[index] = emission[index].min(emits);
            classes[index] |= kind_class(block.kind(), material.flags);
        }
    }
    classes
        .iter()
        .zip(&emission)
        .zip(materials)
        .map(|((&class, &emits), material)| {
            let emits = if emits == u8::MAX { 0 } else { emits };
            let liquid = if material.flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE != 0 {
                CLASS_LAVA
            } else if material.flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_WATER_TINT {
                CLASS_WATER
            } else {
                0
            };
            class | liquid | u32::from(emits) & CLASS_EMISSION_MASK
        })
        .collect()
}

/// Classifies vegetation from geometry and material flags.
fn kind_class(kind: VisualKind, flags: u32) -> u32 {
    let cutout = flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0;
    let tint = flags & MATERIAL_FLAG_TINT_MASK;
    let vegetation_tint = tint == MATERIAL_FLAG_FOLIAGE_TINT || tint == MATERIAL_FLAG_GRASS_TINT;
    match kind {
        VisualKind::Cube if cutout && tint == MATERIAL_FLAG_FOLIAGE_TINT => CLASS_LEAVES,
        VisualKind::Cross => CLASS_PLANT,
        VisualKind::Model if cutout && vegetation_tint => CLASS_PLANT,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_flags_select_waving_classes() {
        let leaves = MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT;
        assert_eq!(kind_class(VisualKind::Cube, leaves), CLASS_LEAVES);
        assert_eq!(kind_class(VisualKind::Cube, MATERIAL_FLAG_ALPHA_CUTOUT), 0);
        assert_eq!(kind_class(VisualKind::Cross, 0), CLASS_PLANT);
        assert_eq!(
            kind_class(
                VisualKind::Model,
                MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_GRASS_TINT
            ),
            CLASS_PLANT
        );
        assert_eq!(kind_class(VisualKind::Model, MATERIAL_FLAG_ALPHA_CUTOUT), 0);
    }

    /// Builds a cube visual for the shared-material regression.
    fn cube(faces: [u32; 6]) -> assets::BlockVisual {
        assets::BlockVisual {
            faces,
            flags: assets::BlockFlags::CUBE_GEOMETRY | assets::BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            support: assets::VisualSupport::Exact,
            contributor_role: assets::ContributorRole::Primary,
            model_template: assets::NO_MODEL_TEMPLATE,
            animation: assets::NO_ANIMATION,
            variant: 0,
        }
    }

    // Glowstone-like, lit/unlit furnace sharing side textures, and leaves.
    #[test]
    fn shared_materials_glow_only_when_every_user_emits() {
        let air = assets::BlockVisual {
            faces: [0; 6],
            flags: assets::BlockFlags::AIR,
            kind: VisualKind::Invisible,
            support: assets::VisualSupport::Exact,
            contributor_role: assets::ContributorRole::Air,
            model_template: assets::NO_MODEL_TEMPLATE,
            animation: assets::NO_ANIMATION,
            variant: 0,
        };
        let light = |emission| assets::LightProperties::new(emission, 0).expect("light");
        let material = |flags| assets::Material {
            texture: assets::TextureRef::DIAGNOSTIC,
            flags,
            animation: assets::NO_ANIMATION,
            variation_start: 0,
            variation_count: 0,
            variation_weight: 0,
        };
        let compiled = assets::CompiledAssets {
            visuals: vec![
                air,
                cube([1; 6]),
                cube([2, 2, 2, 2, 3, 2]),
                cube([2, 2, 2, 2, 4, 2]),
                cube([5; 6]),
            ]
            .into_boxed_slice(),
            light_properties: vec![light(0), light(15), light(13), light(0), light(0)]
                .into_boxed_slice(),
            hashed: Box::new([]),
            materials: vec![
                material(0),
                material(0),
                material(0),
                material(0),
                material(0),
                material(MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT),
            ]
            .into_boxed_slice(),
            model_templates: Box::new([]),
            model_quads: Box::new([]),
            animations: Box::new([]),
            animation_frames: Box::new([]),
            texture_pages: vec![assets::TexturePage::new(assets::TextureArray {
                layers: 1,
                mips: [16_u32, 8, 4, 2, 1]
                    .into_iter()
                    .map(|size| assets::TextureMip {
                        size,
                        rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            })]
            .into_boxed_slice(),
            biomes: assets::CompiledBiomeAssets::diagnostic(),
            provenance: assets::BlobProvenance {
                source_manifest_sha256: [0xA5; 32],
                block_registry_sha256: [0x5A; 32],
                light_registry_sha256: [0x33; 32],
                biome_registry_sha256: [0x3C; 32],
            },
        };
        let blob = assets::encode_blob(&compiled).expect("encode");
        let classes = material_classes(&RuntimeAssets::decode(&blob).expect("decode"));
        assert_eq!(classes[1], 15);
        assert_eq!(classes[2], 0);
        assert_eq!(classes[3], 13);
        assert_eq!(classes[4], 0);
        assert_eq!(classes[5], CLASS_LEAVES);
    }

    #[test]
    fn diagnostic_assets_have_one_quiet_class_per_material() {
        let assets = RuntimeAssets::diagnostic();
        let classes = material_classes(&assets);
        assert_eq!(classes.len(), assets.materials().len());
        assert!(classes.iter().all(|class| class & CLASS_EMISSION_MASK == 0));
    }
}
