use std::mem::size_of_val;

use super::*;
use assets::{BLOCK_VISUAL_VARIANT_COVERED_GRASS, BLOCK_VISUAL_VARIANT_TOP_SNOW};

#[test]
fn missing_values_and_materials_use_one_bounded_diagnostic_counter() {
    let runtime = RuntimeAssets::decode(&valid_blob()).expect("decode valid blob");
    let runtime_size = size_of_val(&runtime);

    for value in 0..10_000 {
        let missing = runtime.resolve(NetworkIdMode::Sequential, value + 100);
        assert!(!missing.is_known());
        assert_eq!(missing.support(), VisualSupport::Diagnostic);
        assert_eq!(
            missing.face(BlockFace::Up).material_id(),
            DIAGNOSTIC_MATERIAL
        );
    }

    assert_eq!(runtime.missing_count(), 10_000);
    assert_eq!(size_of_val(&runtime), runtime_size);
    assert_eq!(
        runtime.material(u32::MAX),
        Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        }
    );
    assert_eq!(runtime.missing_count(), 10_001);
    assert_eq!(
        runtime.material(1),
        Material {
            texture: TextureRef::new(0, 1).unwrap(),
            flags: MATERIAL_FLAG_FOLIAGE_TINT,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        }
    );
    assert_eq!(runtime.missing_count(), 10_001);
}

#[test]
fn covered_grass_variant_is_bounded_and_round_trips() {
    let mut compiled = compiled_assets();
    compiled.visuals[1].variant = BLOCK_VISUAL_VARIANT_COVERED_GRASS | 1;
    let decoded = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    assert_eq!(
        decoded.resolve(NetworkIdMode::Sequential, 1).variant(),
        compiled.visuals[1].variant
    );
    for invalid in [
        BLOCK_VISUAL_VARIANT_COVERED_GRASS,
        BLOCK_VISUAL_VARIANT_COVERED_GRASS | compiled.materials.len() as u32,
        BLOCK_VISUAL_VARIANT_COVERED_GRASS | BLOCK_VISUAL_VARIANT_TOP_SNOW | 1,
    ] {
        compiled.visuals[1].variant = invalid;
        assert!(encode_blob(&compiled).is_err(), "accepted {invalid:#x}");
    }
    compiled.visuals[1].variant = BLOCK_VISUAL_VARIANT_COVERED_GRASS | 1;
    compiled.visuals[1].kind = VisualKind::Diagnostic;
    assert!(encode_blob(&compiled).is_err());
}

#[test]
fn decode_rejects_resealed_out_of_range_covered_grass_material() {
    let mut compiled = compiled_assets();
    compiled.visuals[1].variant = BLOCK_VISUAL_VARIANT_COVERED_GRASS | 1;
    let mut blob = encode_blob(&compiled).unwrap();
    let visuals_offset = read_u64(&blob, VISUALS_OFFSET_OFFSET) as usize;
    // Six face words, flags, kind/support/role, model, animation, then variant.
    let variant_offset = visuals_offset + 44 + 40;
    write_u32(
        &mut blob,
        variant_offset,
        BLOCK_VISUAL_VARIANT_COVERED_GRASS | compiled.materials.len() as u32,
    );
    reseal(&mut blob);
    assert_rejected(&blob, "covered grass material outside the carrier table");
}

#[test]
fn leaf_selector_requires_the_complete_cutout_and_deep_material_group() {
    let mut compiled = compiled_assets();
    let mut materials = compiled.materials.to_vec();
    let base = materials.len() as u32;
    materials.extend(std::iter::repeat_n(
        Material::unvaried(),
        assets::SEASONAL_LEAF_MATERIAL_COUNT as usize,
    ));
    compiled.materials = materials.into_boxed_slice();
    compiled.visuals[1].variant = assets::BLOCK_VISUAL_VARIANT_SEASONAL_LEAF | base;
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    let mut truncated = compiled.materials.to_vec();
    truncated.pop();
    compiled.materials = truncated.into_boxed_slice();
    assert!(encode_blob(&compiled).is_err());
}

#[test]
fn two_sided_world_materials_round_trip_only_with_cutout_admission() {
    let mut compiled = compiled_assets();
    compiled.materials[1].flags =
        assets::MATERIAL_FLAG_TWO_SIDED | assets::MATERIAL_FLAG_ALPHA_CUTOUT;
    let runtime = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    assert_eq!(runtime.materials()[1].flags, compiled.materials[1].flags);
    compiled.materials[1].flags = assets::MATERIAL_FLAG_TWO_SIDED;
    assert!(encode_blob(&compiled).is_err());
    compiled.materials[1].flags |= assets::MATERIAL_FLAG_ALPHA_BLEND;
    assert!(encode_blob(&compiled).is_err());
}

#[test]
fn native_leaf_colour_round_trips_for_cutout_and_deep_but_not_blended_materials() {
    for alpha in [0, assets::MATERIAL_FLAG_ALPHA_CUTOUT] {
        let mut compiled = compiled_assets();
        compiled.materials[1].flags = assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR | alpha;
        let runtime = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
        assert_eq!(runtime.materials()[1].flags, compiled.materials[1].flags);
        compiled.materials[1].flags |= assets::MATERIAL_FLAG_ALPHA_BLEND;
        assert!(encode_blob(&compiled).is_err());
    }
}
