use assets::{build_legacy_terrain_mip_chain, build_texture_mip_chain};

#[test]
fn terrain_array_rebuild_preserves_layers_and_does_not_mutate_shared_art() {
    let base = [
        [0; 4], [255; 4], [0; 4], [0; 4], [120; 4], [120; 4], [120; 4], [120; 4],
    ]
    .concat();
    let texture = assets::TextureArray {
        layers: 2,
        mips: vec![assets::TextureMip {
            size: 2,
            rgba8: base.clone().into_boxed_slice(),
        }]
        .into_boxed_slice(),
    };
    let native = assets::rebuild_legacy_terrain_mips(&texture).unwrap();
    assert_eq!(native.layers, texture.layers);
    assert_eq!(native.mips[0], texture.mips[0]);
    assert_eq!(
        native.mips[1].rgba8.as_ref(),
        &[63, 63, 63, 63, 120, 120, 120, 120]
    );
    assert_eq!(texture.mips.len(), 1);
    assert_eq!(texture.mips[0].rgba8.as_ref(), base);
}

#[test]
fn terrain_array_rebuild_rejects_malformed_layers() {
    for layers in [0, 2, u32::MAX] {
        let texture = assets::TextureArray {
            layers,
            mips: vec![assets::TextureMip {
                size: 1,
                rgba8: vec![0; 4].into_boxed_slice(),
            }]
            .into_boxed_slice(),
        };
        assert!(assets::rebuild_legacy_terrain_mips(&texture).is_err());
    }
}

#[test]
fn native_terrain_mips_average_unassociated_byte_rgba_and_truncate() {
    let base = [
        [0, 0, 0, 0],
        [255, 128, 64, 255],
        [0, 0, 0, 0],
        [0, 0, 0, 0],
    ]
    .concat()
    .into_boxed_slice();
    let native = build_legacy_terrain_mip_chain(&base, 2).unwrap();
    assert_eq!(native[0].rgba8, base);
    assert_eq!(native[1].rgba8.as_ref(), &[63, 32, 16, 63]);
    // The existing shared/carried producer is intentionally left untouched.
    let carried = build_texture_mip_chain(base, 2).unwrap();
    assert_ne!(native[1].rgba8, carried[1].rgba8);
}

#[test]
fn native_terrain_mips_use_original_pixels_at_every_level() {
    // The 2x2 regions sum to 3,7,7,7: native averages all original source
    // pixels to 1, rather than averaging the truncated mip bytes 0,1,1,1 to 0.
    let mut base = vec![0; 4 * 4 * 4];
    for (index, value) in [(0, 3), (2, 7), (8, 7), (10, 7)] {
        base[index * 4..index * 4 + 4].copy_from_slice(&[value; 4]);
    }
    let mips = build_legacy_terrain_mip_chain(&base, 4).unwrap();
    assert_eq!(
        mips[1].rgba8.as_ref(),
        &[0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]
    );
    assert_eq!(mips[2].rgba8.as_ref(), &[1; 4]);
}

#[test]
fn native_terrain_mips_do_not_weaken_fully_covered_cutout_regions() {
    let mut base = vec![0; 4 * 4 * 4];
    for y in 0..2 {
        for x in 0..2 {
            base[(y * 4 + x) * 4..(y * 4 + x + 1) * 4].copy_from_slice(&[100, 150, 200, 255]);
        }
    }
    let native = build_legacy_terrain_mip_chain(&base, 4).unwrap();
    let carried = build_texture_mip_chain(base.into_boxed_slice(), 4).unwrap();
    assert_eq!(native[1].rgba8[3], 255);
    assert_eq!(carried[1].rgba8[3], 128);
    assert_eq!(native[2].rgba8[3], 63);
}

#[test]
fn native_terrain_mips_reject_unbounded_sizes_and_invalid_base_lengths() {
    for size in [0, 3, assets::MAX_TILE_SIZE * 2] {
        assert!(build_legacy_terrain_mip_chain(&[], size).is_err());
    }
    assert!(build_legacy_terrain_mip_chain(&[0; 3], 1).is_err());
}
