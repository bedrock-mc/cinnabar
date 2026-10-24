use super::*;
use assets::{
    BlockOverlay, Material, TextureRef,
    material_keys::{MaterialKeys, MaterialTextureSource},
};

/// A base carrier whose material 1 is a flat colour layer appended after the diagnostic page.
fn base(color: [u8; 4]) -> RuntimeAssets {
    let mut texture = RuntimeAssets::diagnostic().texture_array().clone();
    for mip in &mut texture.mips {
        mip.rgba8 = color.repeat((mip.size * mip.size) as usize).into();
    }
    let overlay = BlockOverlay {
        materials: vec![Material {
            texture: TextureRef::new(1, 0).unwrap(),
            ..Material::unvaried()
        }],
        textures: vec![texture],
        ..Default::default()
    };
    RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .unwrap()
}

fn source(path: &str, baked_tint: Option<[u8; 3]>) -> MaterialTextureSource {
    MaterialTextureSource {
        key: "fixture".into(),
        variant: 0,
        path: path.into(),
        raster: format!("{path}.png").into(),
        baked_tint,
    }
}

#[test]
fn item_texture_paths_resolve_base_block_rasters() {
    let keys = MaterialKeys::default().with_sources(
        std::iter::empty(),
        [(1, source("textures/blocks/fixture_beacon", None))],
        [],
    );
    let index = terrain_raster_index(&keys);
    let texture = terrain_raster(
        &index,
        &base([9, 80, 160, 255]),
        "textures/blocks/Fixture_Beacon",
    )
    .expect("vanilla block raster by its item_texture path");
    assert_eq!((texture.width, texture.height), (16, 16));
    assert_eq!(&texture.rgba8[..4], &[9, 80, 160, 255]);
    assert!(terrain_raster(&index, &base([0; 4]), "textures/blocks/absent").is_none());
}

#[test]
fn rasters_with_baked_atlas_tints_are_not_their_source_image() {
    let keys = MaterialKeys::default().with_sources(
        std::iter::empty(),
        [(
            1,
            source("textures/blocks/fixture_grass", Some([120, 200, 80])),
        )],
        [],
    );
    let index = terrain_raster_index(&keys);
    assert!(
        terrain_raster(
            &index,
            &base([1, 2, 3, 255]),
            "textures/blocks/fixture_grass"
        )
        .is_none()
    );
}
