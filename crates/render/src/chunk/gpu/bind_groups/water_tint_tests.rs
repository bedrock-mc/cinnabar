use super::*;

#[test]
fn native_water_palette_preserves_every_authored_rgb8_byte() {
    for value in 0_u8..=255 {
        let authored = [value, value.wrapping_mul(37), value.wrapping_mul(113)];
        let water = Color::srgb_u8(authored[0], authored[1], authored[2]).to_linear();
        let entry = BiomeTint {
            water: [water.red, water.green, water.blue],
            water_opacity: assets::DEFAULT_WATER_APPEARANCE_OPACITY,
            ..BiomeTint::default()
        };
        let uploaded = prepare_biome_tint_entries(&[entry]);
        assert_eq!(uploaded[0].water.to_le_bytes()[..3], authored);
        assert_eq!(uploaded[0].water_opacity, entry.water_opacity);
        assert_eq!(uploaded[0].grass, pack_linear_rgb10(entry.grass));
        assert_eq!(uploaded[0].foliage, pack_linear_rgb10(entry.foliage));
    }
}
