/// Horizontal random-offset range and lattice shared by every stalk in a column.
pub const OFFSET_MIN: f32 = -0.25;
pub const OFFSET_SPAN: f32 = 0.5;
pub const OFFSET_STEPS: u32 = 16;
pub const STEM_UV_STRIDE: f32 = 3.0 / 16.0;
pub const LEAF_PLANE_INSET: f32 = 0.01;

const POSITION_X_MULTIPLIER: u32 = 0x002f_c20f;
const POSITION_Z_MULTIPLIER: u32 = 0x06eb_fff5;

/// Packs X/Z offset indices and the stem's four-way UV selector.
pub(crate) fn positional_transform(x: i32, z: i32) -> u32 {
    let hash = (i64::from(z).wrapping_mul(i64::from(POSITION_Z_MULTIPLIER))
        ^ i64::from(x).wrapping_mul(i64::from(POSITION_X_MULTIPLIER))) as u64;
    let seed = ((hash
        .wrapping_mul(0x0285_b825)
        .wrapping_add(11)
        .wrapping_mul(hash)
        >> 16) as i32 as i64 as u64)
        ^ 0x6a09_e667_f3bc_c909;
    let mut a = mix(seed);
    let mut b = mix(seed.wrapping_add(0x9e37_79b9_7f4a_7c15));
    if a == 0 && b == 0 {
        a = 0x9e37_79b9_7f4a_7c15;
        b = 0x6a09_e667_f3bc_c909;
    }
    let offset_x = next(&mut a, &mut b) >> 60;
    let _ = next(&mut a, &mut b);
    let offset_z = next(&mut a, &mut b) >> 60;
    let hash = (z as u32).wrapping_mul(POSITION_Z_MULTIPLIER)
        ^ (x as u32).wrapping_mul(POSITION_X_MULTIPLIER);
    let uv = (hash.wrapping_mul(37).wrapping_add(11).wrapping_mul(hash) >> 4) & 3;
    offset_x as u32 | ((offset_z as u32) << 4) | (uv << 8)
}

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn next(a: &mut u64, b: &mut u64) -> u64 {
    let value = a.wrapping_add(*b).rotate_left(17).wrapping_add(*a);
    *b ^= *a;
    *a = a.rotate_left(49) ^ *b ^ b.wrapping_shl(21);
    *b = b.rotate_left(28);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_offsets_preserve_signed_coordinates_and_wrapping_hashes() {
        for (x, z, expected_x, expected_z) in [
            (0, 0, 2, 4),
            (1, 0, 15, 7),
            (0, 1, 14, 9),
            (-1, -1, 5, 2),
            (16, -32, 2, 2),
            (i32::MIN, i32::MAX, 8, 14),
        ] {
            let transform = positional_transform(x, z);
            assert_eq!(transform & 15, expected_x, "x={x}, z={z}");
            assert_eq!((transform >> 4) & 15, expected_z, "x={x}, z={z}");
        }
    }

    #[test]
    fn origin_matches_pinned_bamboo_collision_offset() {
        let transform = positional_transform(0, 0);
        assert_eq!(transform, 2 | (4 << 4));
        assert!(
            (OFFSET_MIN + (transform & 15) as f32 * OFFSET_SPAN / (OFFSET_STEPS - 1) as f32 + 0.5
                - 0.31666666)
                .abs()
                < 0.0000001
        );
        assert!(
            (OFFSET_MIN
                + ((transform >> 4) & 15) as f32 * OFFSET_SPAN / (OFFSET_STEPS - 1) as f32
                + 0.5
                - 0.38333333)
                .abs()
                < 0.0000001
        );
    }
}
