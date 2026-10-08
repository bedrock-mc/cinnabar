use assets::{
    BlobProvenance, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets, ContributorRole,
    LightProperties, MATERIAL_FLAG_OVERLAY_MASK, MIP_COUNT, Material, NO_ANIMATION,
    NO_MODEL_TEMPLATE, TILE_SIZE, TextureArray, TextureMip, TexturePage, TextureRef, VisualKind,
    VisualSupport, encode_blob,
};

use super::*;

/// Builds distinct stone, grass face and diagnostic layers without external carriers.
fn fixture() -> RuntimeAssets {
    let cube = BlockVisual {
        faces: [1; 6],
        flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
        kind: VisualKind::Cube,
        support: VisualSupport::Exact,
        contributor_role: ContributorRole::Primary,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    };
    let mut grass = cube;
    grass.faces = [3, 3, 2, 4, 3, 3];
    let diagnostic = BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary);
    let colors = [
        [255, 0, 255, 255],
        [120, 120, 120, 255],
        [95, 65, 40, 255],
        [0, 200, 0, 255],
        [180, 180, 180, 255],
    ];
    let mips = (0..MIP_COUNT)
        .map(|level| {
            let size = TILE_SIZE >> level;
            TextureMip {
                size,
                rgba8: colors
                    .iter()
                    .flat_map(|color| color.repeat((size * size) as usize))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let materials = (0..colors.len())
        .map(|layer| Material {
            texture: TextureRef::new(0, layer as u32).unwrap(),
            flags: match layer {
                3 => MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK,
                4 => MATERIAL_FLAG_GRASS_TINT,
                _ => 0,
            },
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let compiled = CompiledAssets {
        visuals: vec![diagnostic, cube, grass].into_boxed_slice(),
        light_properties: vec![LightProperties::default(); 3].into_boxed_slice(),
        hashed: vec![(11, 0), (22, 1), (33, 2)].into_boxed_slice(),
        materials,
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: colors.len() as u32,
            mips,
        })]
        .into_boxed_slice(),
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
fn stone_terrain_tile_uses_the_resolved_block_region() {
    let assets = fixture();
    let (tile, flags) = terrain_tile(&assets, NetworkIdMode::Sequential, 1).unwrap();
    assert_eq!(tile.key, 1);
    assert_eq!(tile.size, TILE_SIZE);
    assert_eq!(&tile.pixels[..4], &[120, 120, 120, 255]);
    assert_eq!(flags, 0);
}

#[test]
fn grass_terrain_uses_the_untinted_bottom_texture() {
    let assets = fixture();
    let (tile, flags) = terrain_tile(&assets, NetworkIdMode::Sequential, 2).unwrap();
    assert_eq!(tile.key, 2);
    assert_eq!(&tile.pixels[..4], &[95, 65, 40, 255]);
    assert_eq!(flags & MATERIAL_FLAG_TINT_MASK, 0);
    assert_eq!(flags & MATERIAL_FLAG_OVERLAY_MASK, 0);
}

#[test]
fn terrain_tile_hashes_resolve_the_same_state_and_skip_diagnostics() {
    let assets = fixture();
    for (id, hash) in [(1, 22), (2, 33)] {
        let sequential = terrain_tile(&assets, NetworkIdMode::Sequential, id).unwrap();
        let hashed = terrain_tile(&assets, NetworkIdMode::Hashed, hash).unwrap();
        assert_eq!(sequential.0.key, hashed.0.key);
        assert_eq!(sequential.0.pixels, hashed.0.pixels);
        assert_eq!(sequential.1, hashed.1);
    }
    assert!(terrain_tile(&assets, NetworkIdMode::Sequential, 0).is_none());
    assert!(terrain_tile(&assets, NetworkIdMode::Hashed, 11).is_none());
    assert!(terrain_tile(&assets, NetworkIdMode::Hashed, 99).is_none());
    assert!(terrain_tile(&RuntimeAssets::diagnostic(), NetworkIdMode::Sequential, 1).is_none());
}
