use assets::*;

use super::terrain_material;

const GRASS_HASH: u32 = 0xdead_beef;
const DIRT_MATERIAL: u32 = 1;
const TOP_MATERIAL: u32 = 2;
const SIDE_MATERIAL: u32 = 3;

fn grass_fixture(side_flags: u32) -> RuntimeAssets {
    let mut grass = BlockVisual {
        faces: [SIDE_MATERIAL; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: VisualSupport::Exact,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    };
    grass.faces[BlockFace::Down as usize] = DIRT_MATERIAL;
    grass.faces[BlockFace::Up as usize] = TOP_MATERIAL;
    let materials: Vec<_> = [0, 0, MATERIAL_FLAG_GRASS_TINT, side_flags]
        .into_iter()
        .enumerate()
        .map(|(layer, flags)| Material {
            texture: TextureRef::new(0, layer as u32).unwrap(),
            flags,
            animation: NO_ANIMATION,
            ..Material::unvaried()
        })
        .collect();
    let compiled = CompiledAssets {
        visuals: vec![
            BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary),
            grass,
        ]
        .into(),
        light_properties: vec![LightProperties::default(); 2].into(),
        hashed: vec![(GRASS_HASH, 1)].into(),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: materials.len() as u32,
            mips: [16, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![255; size as usize * size as usize * 4 * materials.len()].into(),
                })
                .collect::<Vec<_>>()
                .into(),
        })]
        .into(),
        materials: materials.into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

#[test]
fn grass_particles_use_dirt_bottom_in_both_runtime_id_modes() {
    let assets = grass_fixture(0);
    for (mode, id) in [
        (NetworkIdMode::Sequential, 1),
        (NetworkIdMode::Hashed, GRASS_HASH),
    ] {
        let material = terrain_material(&assets, mode, id);
        assert_eq!(material.texture.layer(), DIRT_MATERIAL);
        assert_eq!(material.flags & MATERIAL_FLAG_TINT_MASK, 0);
    }
}

#[test]
fn terrain_texture_is_not_selected_by_top_or_side_tint() {
    for flags in [0, MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_FOLIAGE_TINT] {
        let assets = grass_fixture(flags);
        assert_eq!(
            terrain_material(&assets, NetworkIdMode::Sequential, 1)
                .texture
                .layer(),
            DIRT_MATERIAL
        );
    }
}
