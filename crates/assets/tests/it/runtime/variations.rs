use super::*;

#[test]
fn decode_rejects_resealed_invalid_positional_material_ranges_and_weights() {
    let mut compiled = compiled_assets();
    let mut materials = compiled.materials.into_vec();
    let leaf = materials[1];
    materials[1].variation_start = materials.len() as u32;
    materials[1].variation_count = 2;
    materials.extend(
        [Material {
            variation_weight: 0.5_f32.to_bits(),
            ..leaf
        }; 2],
    );
    compiled.materials = materials.into();
    let canonical = encode_blob(&compiled).unwrap();
    assert_eq!(
        RuntimeAssets::decode(&canonical).unwrap().materials(),
        compiled.materials.as_ref()
    );
    let table = read_u64(&canonical, MATERIALS_OFFSET_OFFSET) as usize;
    let selector = table + assets::MATERIAL_BYTES;
    let first_leaf = table + 2 * assets::MATERIAL_BYTES;
    for (offset, value) in [
        (selector + 12, u32::MAX),
        (selector + 16, u32::MAX),
        (selector + 20, 0.5_f32.to_bits()),
        (first_leaf + 4, assets::MATERIAL_FLAG_ALPHA_BLEND),
        (first_leaf + 16, 1),
        (first_leaf + 20, f32::NAN.to_bits()),
        (first_leaf + 20, (-0.5_f32).to_bits()),
        (first_leaf + 20, 0),
    ] {
        let mut corrupted = canonical.to_vec();
        write_u32(&mut corrupted, offset, value);
        reseal(&mut corrupted);
        assert_rejected(
            &corrupted,
            &format!("invalid positional material metadata at byte {offset}"),
        );
    }
}
