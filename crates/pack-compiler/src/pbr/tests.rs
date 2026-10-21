use std::{fs, path::Path};

use image::{ImageBuffer, Rgb, Rgba, RgbaImage};

use super::*;
use assets::texture::{
    PBR_REF_HEIGHT, PBR_REF_LABPBR, PBR_REF_MATERIAL, PBR_REF_NORMAL, PBR_REF_OCCLUSION,
    PBR_REF_SUBSURFACE,
};

/// Writes a test image and creates its parent directory.
fn save(root: &Path, name: &str, image: &RgbaImage) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image.save(path).unwrap();
}

/// Creates a small constant RGBA image for material tests.
fn rgba(pixel: [u8; 4]) -> RgbaImage {
    ImageBuffer::from_pixel(2, 2, Rgba(pixel))
}

#[test]
fn terrain_atlas_tint_reaches_authored_albedo_before_mip_filtering() {
    let root = tempfile::tempdir().unwrap();
    save(
        root.path(),
        "textures/blocks/pad.png",
        &rgba([200, 160, 120, 128]),
    );
    fs::write(
        root.path().join("textures/terrain_texture.json"),
        br##"{
        "texture_data":{"waterlily":{"textures":[{
            "path":"textures/blocks/pad", "tint_color":"#804020"
        }]}}
    }"##,
    )
    .unwrap();
    let pack = PbrPack::new(root.path(), None)
        .with_terrain_aliases([("waterlily", "textures/blocks/carrier_pad")]);
    let texture = load_pbr_texture(&[pack], "textures/blocks/carrier_pad")
        .unwrap()
        .unwrap();
    let frame = texture.frame(0, 1).unwrap();
    assert_eq!(frame.color.get_pixel(0, 0).0, [100, 40, 15, 128]);
    assert_eq!(frame.normal.get_pixel(0, 0).0, [128, 128, 255, 128]);
    let mips = build_pbr_mips(&frame, 2, false, false).unwrap();
    for mip in &mips.color {
        assert!(
            mip.rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [100, 40, 15, 128])
        );
    }
}

/// Combines test images and channel flags into an authored surface.
fn surface(color: RgbaImage, normal: RgbaImage, material: RgbaImage, flags: u32) -> PbrSurface {
    PbrSurface {
        color,
        normal,
        material,
        flags,
    }
}

#[test]
fn labpbr_preserves_dielectric_f0_metal_ids_height_ao_and_emission_sentinel() {
    let (normal, flags) = decode::normal(
        rgba([128, 192, 77, 19]),
        PbrFormat::LabPbr13,
        PbrNormalFormat::DirectX,
    );
    assert_eq!(normal.get_pixel(0, 0).0, [128, 192, 77, 19]);
    assert_ne!(flags & PBR_REF_OCCLUSION, 0);
    let (flipped, _) = decode::normal(
        rgba([128, 192, 77, 19]),
        PbrFormat::LabPbr13,
        PbrNormalFormat::OpenGl,
    );
    assert_eq!(flipped.get_pixel(0, 0)[1], 63);
    let (material, flags) = decode::specular(rgba([128, 10, 64, 255]), PbrFormat::LabPbr13);
    assert_eq!(material.get_pixel(0, 0).0, [10, 0, 127, 64]);
    assert_ne!(flags & PBR_REF_LABPBR, 0);
    assert_eq!(flags & PBR_REF_SUBSURFACE, 0);
    let (metal, _) = decode::specular(rgba([255, 231, 100, 254]), PbrFormat::LabPbr13);
    assert_eq!(metal.get_pixel(0, 0).0, [231, 255, 0, 100]);
    let (legacy, flags) = decode::specular(rgba([128, 10, 64, 255]), PbrFormat::Legacy);
    assert_eq!(legacy.get_pixel(0, 0).0, [10, 64, 127, 0]);
    assert_eq!(flags & PBR_REF_LABPBR, 0);
}

#[test]
fn texture_set_constants_follow_argb_mer_and_subsurface_dominance_rules() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join("textures/blocks/stone.texture_set.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, br##"{"minecraft:texture_set":{"color":"#80112233","metalness_emissive_roughness_subsurface":[64.0,128.0,192.0,128.0],"heightmap":64.0}}"##).unwrap();
    let pack = PbrPack::new(directory.path(), None);
    let texture = load_pbr_texture(&[pack], "textures/blocks/stone")
        .unwrap()
        .unwrap();
    let frame = texture.frame(0, 1).unwrap();
    assert_eq!(frame.color.get_pixel(0, 0).0, [17, 34, 51, 128]);
    assert_eq!(frame.material.get_pixel(0, 0).0, [0, 128, 192, 128]);
    assert_eq!(frame.normal.get_pixel(0, 0).0, [128, 128, 255, 64]);
    assert_eq!(frame.flags & (PBR_REF_HEIGHT | PBR_REF_NORMAL), 0);
    assert_ne!(frame.flags & PBR_REF_SUBSURFACE, 0);
    let (tie, _) = decode::mer(rgba([64, 0, 192, 64]), true);
    assert_eq!(tie.get_pixel(0, 0).0, [0, 0, 192, 64]);
    let (metal, _) = decode::mer(rgba([255, 0, 192, 64]), true);
    assert_eq!(metal.get_pixel(0, 0).0, [255, 0, 192, 0]);
    fs::write(&path,br#"{"minecraft:texture_set":{"color":[255,255,255,255],"metalness_emissive_roughness":[64.5,0,255]}}"#).unwrap();
    assert!(
        load_pbr_texture(
            &[PbrPack::new(directory.path(), None)],
            "textures/blocks/stone"
        )
        .is_err()
    );
}

#[test]
fn texture_sets_reject_conflicting_layers_and_cross_pack_references() {
    let owner = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let path = owner.path().join("textures/blocks/stone.texture_set.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, br#"{"minecraft:texture_set":{"color":[255,255,255,255],"normal":"normal","heightmap":64}}"#).unwrap();
    let packs = [
        PbrPack::new(owner.path(), None),
        PbrPack::new(other.path(), None),
    ];
    assert!(load_pbr_texture(&packs, "textures/blocks/stone").is_err());
    fs::write(&path, br#"{"minecraft:texture_set":{"color":"missing"}}"#).unwrap();
    save(
        other.path(),
        "textures/blocks/missing.png",
        &rgba([1, 2, 3, 255]),
    );
    assert!(load_pbr_texture(&packs, "textures/blocks/stone").is_err());
    fs::write(
        &path,
        br#"{"minecraft:texture_set":{"color":"../../outside"}}"#,
    )
    .unwrap();
    assert!(load_pbr_texture(&packs, "textures/blocks/stone").is_err());
}

#[test]
fn suffixes_do_not_guess_specular_format_or_mix_unrelated_material_companions() {
    let colors = tempfile::tempdir().unwrap();
    let maps = tempfile::tempdir().unwrap();
    save(
        colors.path(),
        "assets/minecraft/textures/block/stone.png",
        &rgba([1, 2, 3, 255]),
    );
    save(
        maps.path(),
        "assets/minecraft/textures/block/stone_s.png",
        &rgba([255, 231, 77, 128]),
    );
    let unknown = [
        PbrPack::new(colors.path(), None).with_group("paired"),
        PbrPack::new(maps.path(), None).with_group("paired"),
    ];
    let frame = load_pbr_texture(&unknown, "textures/blocks/stone")
        .unwrap()
        .unwrap()
        .frame(0, 1)
        .unwrap();
    assert_eq!(frame.flags & PBR_REF_MATERIAL, 0);
    let unpaired = [
        PbrPack::new(colors.path(), None),
        PbrPack::new(maps.path(), Some(PbrFormat::LabPbr13)),
    ];
    let frame = load_pbr_texture(&unpaired, "textures/blocks/stone")
        .unwrap()
        .unwrap()
        .frame(0, 1)
        .unwrap();
    assert_eq!(frame.flags & PBR_REF_MATERIAL, 0);
    let paired = [
        PbrPack::new(colors.path(), None).with_group("paired"),
        PbrPack::new(maps.path(), Some(PbrFormat::LabPbr13)).with_group("paired"),
    ];
    let frame = load_pbr_texture(&paired, "textures/blocks/stone")
        .unwrap()
        .unwrap()
        .frame(0, 1)
        .unwrap();
    assert_ne!(frame.flags & PBR_REF_MATERIAL, 0);
    assert_eq!(frame.material.get_pixel(0, 0)[0], 231);

    // A split pack may put the verified LabPBR declaration on its color owner.
    // The grouped map companion inherits that declaration; an unrelated pack
    // still cannot make `_s` meaningful.
    let owner_declares_format = [
        PbrPack::new(colors.path(), Some(PbrFormat::LabPbr13)).with_group("owner-declared"),
        PbrPack::new(maps.path(), None).with_group("owner-declared"),
    ];
    let frame = load_pbr_texture(&owner_declares_format, "textures/blocks/stone")
        .unwrap()
        .unwrap()
        .frame(0, 1)
        .unwrap();
    assert_ne!(frame.flags & PBR_REF_MATERIAL, 0);
}

#[test]
fn translated_block_names_and_declared_format_match_bedrock_catalog_paths() {
    let directory = tempfile::tempdir().unwrap();
    save(
        directory.path(),
        "assets/minecraft/textures/block/oak_planks.png",
        &rgba([10, 20, 30, 255]),
    );
    save(
        directory.path(),
        "assets/minecraft/textures/block/oak_planks_s.png",
        &rgba([128, 10, 0, 255]),
    );
    let path = directory
        .path()
        .join("assets/minecraft/optifine/texture.properties");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "format=lab-pbr/1.3\n").unwrap();
    let pack = PbrPack::new(directory.path(), None);
    assert_eq!(pack.format(), PbrFormat::LabPbr13);
    let frame = load_pbr_texture(&[pack], "textures/blocks/planks_oak")
        .unwrap()
        .unwrap()
        .frame(0, 1)
        .unwrap();
    assert_eq!(frame.color.get_pixel(0, 0).0, [10, 20, 30, 255]);
    assert_eq!(frame.material.get_pixel(0, 0).0, [10, 0, 127, 0]);
}

#[test]
fn animation_strips_keep_each_color_frame_and_static_material_maps() {
    let directory = tempfile::tempdir().unwrap();
    let color = RgbaImage::from_fn(2, 6, |_, y| {
        Rgba(match y / 2 {
            0 => [255, 0, 0, 255],
            1 => [0, 255, 0, 255],
            _ => [0, 0, 255, 255],
        })
    });
    save(
        directory.path(),
        "assets/minecraft/textures/block/water_still.png",
        &color,
    );
    save(
        directory.path(),
        "assets/minecraft/textures/block/water_still_mer.png",
        &rgba([0, 0, 12, 255]),
    );
    let texture = load_pbr_texture(
        &[PbrPack::new(directory.path(), None)],
        "textures/blocks/water_still",
    )
    .unwrap()
    .unwrap();
    for (index, expected) in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
        .into_iter()
        .enumerate()
    {
        let frame = texture.frame(index, 3).unwrap();
        assert_eq!(frame.color.dimensions(), (2, 2));
        assert_eq!(frame.color.get_pixel(0, 0).0, expected);
        assert_eq!(frame.material.get_pixel(0, 0).0, [0, 0, 12, 0]);
    }
    fs::write(
        directory
            .path()
            .join("assets/minecraft/textures/block/water_still.png.mcmeta"),
        br#"{"animation":{"frames":[2,0]}}"#,
    )
    .unwrap();
    let texture = load_pbr_texture(
        &[PbrPack::new(directory.path(), None)],
        "textures/blocks/water_still",
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        texture.frame(0, 2).unwrap().color.get_pixel(0, 0).0,
        [0, 0, 255, 255]
    );
    assert_eq!(
        texture.frame(1, 2).unwrap().color.get_pixel(0, 0).0,
        [255, 0, 0, 255]
    );
    let normals = RgbaImage::from_fn(2, 6, |_, y| {
        Rgba([128, 128, (111 + y / 2 * 11) as u8, ((y / 2 + 1) * 32) as u8])
    });
    let materials = RgbaImage::from_fn(2, 6, |_, y| Rgba([(y / 2 * 21) as u8, 0, 12, 255]));
    save(
        directory.path(),
        "assets/minecraft/textures/block/water_still_n.png",
        &normals,
    );
    save(
        directory.path(),
        "assets/minecraft/textures/block/water_still_mer.png",
        &materials,
    );
    let texture = load_pbr_texture(
        &[PbrPack::new(directory.path(), Some(PbrFormat::LabPbr13))],
        "textures/blocks/water_still",
    )
    .unwrap()
    .unwrap();
    let blue = texture.frame(0, 2).unwrap();
    assert_eq!(blue.normal.get_pixel(0, 0).0[2..], [133, 96]);
    assert_eq!(blue.material.get_pixel(0, 0)[0], 42);
    let red = texture.frame(1, 2).unwrap();
    assert_eq!(red.normal.get_pixel(0, 0).0[2..], [111, 32]);
    assert_eq!(red.material.get_pixel(0, 0)[0], 0);
}

#[test]
fn standalone_height_generates_normal_gradients_in_increasing_v_basis() {
    let directory = tempfile::tempdir().unwrap();
    save(
        directory.path(),
        "assets/minecraft/textures/block/stone.png",
        &rgba([128, 128, 128, 255]),
    );
    let heights = RgbaImage::from_fn(4, 4, |_, y| {
        let value = (y * 64) as u8;
        Rgba([value, value, value, 255])
    });
    save(
        directory.path(),
        "assets/minecraft/textures/block/stone_h.png",
        &heights,
    );
    let frame = load_pbr_texture(
        &[PbrPack::new(directory.path(), None)],
        "textures/blocks/stone",
    )
    .unwrap()
    .unwrap()
    .frame(0, 1)
    .unwrap();
    assert_ne!(frame.flags & PBR_REF_HEIGHT, 0);
    assert_ne!(frame.flags & PBR_REF_NORMAL, 0);
    assert!(frame.normal.get_pixel(1, 1)[1] < 128);
    assert_eq!(frame.normal.get_pixel(1, 1)[3], 64);
}

#[test]
fn smaller_authored_normals_preserve_separate_height_detail() {
    let directory = tempfile::tempdir().unwrap();
    save(
        directory.path(),
        "textures/blocks/fixture.png",
        &rgba([255; 4]),
    );
    save(
        directory.path(),
        "textures/blocks/fixture_n.png",
        &ImageBuffer::from_pixel(1, 1, Rgba([192, 128, 255, 128])),
    );
    let heights = RgbaImage::from_fn(4, 4, |x, y| Rgba([(x + y * 4) as u8 * 16; 4]));
    save(directory.path(), "textures/blocks/fixture_h.png", &heights);
    let frame = load_pbr_texture(
        &[PbrPack::new(directory.path(), Some(PbrFormat::LabPbr13))],
        "textures/blocks/fixture",
    )
    .unwrap()
    .unwrap()
    .frame(0, 1)
    .unwrap();
    assert_ne!(frame.flags & PBR_REF_HEIGHT, 0);
    assert_ne!(frame.flags & PBR_REF_NORMAL, 0);
    assert_eq!(frame.normal.dimensions(), heights.dimensions());
    for (normal, height) in frame.normal.pixels().zip(heights.pixels()) {
        assert_eq!(normal.0[..3], [192, 128, 255]);
        assert_eq!(normal[3], height[0]);
    }
}

#[test]
fn initial_resizing_weights_surface_channels_by_opacity() {
    let color = RgbaImage::from_fn(4, 4, |x, _| {
        Rgba([255, 255, 255, if x % 2 == 0 { 255 } else { 0 }])
    });
    let normal = RgbaImage::from_fn(4, 4, |x, _| {
        Rgba(if x % 2 == 0 {
            [64, 96, 180, 40]
        } else {
            [240, 128, 0, 220]
        })
    });
    let material = RgbaImage::from_fn(4, 4, |x, _| {
        Rgba(if x % 2 == 0 {
            [20, 30, 64, 40]
        } else {
            [200, 220, 255, 240]
        })
    });
    for format in [0, PBR_REF_LABPBR] {
        for side in [1, 2, 8] {
            let mips = build_pbr_mips(
                &surface(
                    color.clone(),
                    normal.clone(),
                    material.clone(),
                    PBR_REF_NORMAL | PBR_REF_MATERIAL | format,
                ),
                side,
                false,
                false,
            )
            .unwrap();
            for ((color, normal), material) in mips.color[0]
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .zip(mips.normal[0].rgba8.as_chunks::<4>().0)
                .zip(mips.material[0].rgba8.as_chunks::<4>().0)
                .filter(|((color, _), _)| color[3] > 0)
            {
                assert!(color[3] > 0);
                assert_eq!(*normal, [64, 96, 180, 40]);
                for (&actual, expected) in material.iter().zip([20, 30, 64, 40]) {
                    assert!(
                        (actual as i32 - expected).abs() <= 1,
                        "material: {material:?}"
                    );
                }
            }
        }
    }
    let overlay = build_pbr_mips(
        &surface(color, normal, material, PBR_REF_NORMAL | PBR_REF_MATERIAL),
        1,
        false,
        true,
    )
    .unwrap();
    assert!(overlay.normal[0].rgba8[0] > 128);
    assert_eq!(overlay.normal[0].rgba8[2..], [90, 130]);
    assert!(overlay.material[0].rgba8[2] > 200);
}

#[test]
fn initial_resizing_preserves_signed_normals_and_constant_roughness() {
    for normal in [[64, 96, 180, 40], [64, 160, 180, 40], [192, 96, 180, 40]] {
        for side in [1, 2, 8] {
            let mips = build_pbr_mips(
                &surface(
                    rgba([255, 255, 255, 128]),
                    RgbaImage::from_pixel(4, 4, Rgba(normal)),
                    rgba([20, 30, 96, 40]),
                    PBR_REF_NORMAL | PBR_REF_MATERIAL,
                ),
                side,
                false,
                false,
            )
            .unwrap();
            for mip in &mips.normal {
                for actual in mip.rgba8.as_chunks::<4>().0 {
                    assert_eq!(*actual, normal, "normal {normal:?} resized to {side}");
                }
            }
            for mip in &mips.material {
                for actual in mip.rgba8.as_chunks::<4>().0 {
                    assert!(
                        (actual[2] as i32 - 96).abs() <= 1,
                        "constant normal {normal:?} resized to {side} changed roughness: {actual:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn initial_resizing_filters_opposing_normals_before_roughness_correction() {
    for horizontal in [false, true] {
        let normals = RgbaImage::from_fn(4, 4, |x, y| {
            let coordinate = if horizontal { x } else { y };
            Rgba([if coordinate % 2 == 0 { 25 } else { 230 }, 128, 255, 128])
        });
        let mips = build_pbr_mips(
            &surface(
                rgba([255, 255, 255, 255]),
                normals,
                rgba([0, 0, 0, 0]),
                PBR_REF_NORMAL | PBR_REF_MATERIAL,
            ),
            2,
            false,
            false,
        )
        .unwrap();
        assert!((mips.normal.last().unwrap().rgba8[0] as i32 - 128).abs() <= 1);
        assert!(mips.material.last().unwrap().rgba8[2] > 128);
    }
}

#[test]
fn color_filtering_is_linear_premultiplied_and_cutout_coverage_survives_mips() {
    let color = RgbaImage::from_fn(2, 2, |x, _| {
        Rgba(if x == 0 {
            [0, 0, 0, 255]
        } else {
            [255, 255, 255, 255]
        })
    });
    let mip = build_pbr_mips(
        &surface(color, rgba([128, 128, 255, 128]), rgba([0, 0, 255, 0]), 0),
        2,
        false,
        false,
    )
    .unwrap();
    assert!((mip.color[1].rgba8[0] as i32 - 188).abs() <= 1);
    let color = RgbaImage::from_fn(4, 4, |x, _| {
        Rgba(if x < 2 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 0]
        })
    });
    let mip = build_pbr_mips(
        &surface(color, rgba([128, 128, 255, 128]), rgba([0, 0, 255, 0]), 0),
        4,
        true,
        false,
    )
    .unwrap();
    let middle = &mip.color[1].rgba8;
    assert_eq!(
        middle
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] >= 128)
            .count(),
        2
    );
    assert_eq!(mip.color[2].rgba8[..3], [255, 0, 0]);
}

#[test]
fn normal_mips_are_unit_vectors_and_unresolved_detail_raises_roughness() {
    let normals = RgbaImage::from_fn(2, 2, |x, _| {
        Rgba([if x == 0 { 230 } else { 25 }, 128, 255, 128])
    });
    let mip = build_pbr_mips(
        &surface(
            rgba([255, 255, 255, 255]),
            normals,
            rgba([0, 0, 0, 0]),
            PBR_REF_NORMAL | PBR_REF_MATERIAL,
        ),
        2,
        false,
        false,
    )
    .unwrap();
    let normal = &mip.normal[1].rgba8;
    assert!((normal[0] as i32 - 128).abs() <= 1);
    assert!((normal[1] as i32 - 128).abs() <= 1);
    assert_eq!(normal[2..], [255, 128]);
    assert!(mip.material[1].rgba8[2] > mip.material[0].rgba8[2] + 64);
    let material = RgbaImage::from_fn(2, 2, |x, _| {
        Rgba([if x == 0 { 237 } else { 230 }, 0, 64, 12])
    });
    let mip = build_pbr_mips(
        &surface(
            rgba([255, 255, 255, 255]),
            rgba([128, 128, 255, 128]),
            material,
            PBR_REF_MATERIAL | PBR_REF_LABPBR,
        ),
        2,
        false,
        false,
    )
    .unwrap();
    assert_eq!(mip.material[1].rgba8[0], 230);
    let material = RgbaImage::from_fn(4, 4, |x, _| {
        Rgba([if x % 2 == 0 { 237 } else { 229 }, 0, 64, 12])
    });
    let mip = build_pbr_mips(
        &surface(
            rgba([255, 255, 255, 255]),
            rgba([128, 128, 255, 128]),
            material,
            PBR_REF_MATERIAL | PBR_REF_LABPBR,
        ),
        2,
        false,
        false,
    )
    .unwrap();
    assert!(
        mip.material[0]
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| [229, 237].contains(&pixel[0])),
        "initial normalization must preserve encoded F0/metal identity"
    );
}

#[test]
fn labpbr_categories_ignore_transparent_texels_but_keep_overlay_mask_texels() {
    for (visible, hidden) in [
        ([20, 0, 64, 30], [237, 0, 64, 200]),
        ([237, 0, 64, 200], [20, 0, 64, 30]),
    ] {
        let color = RgbaImage::from_fn(2, 2, |x, _| {
            Rgba([255, 255, 255, if x == 0 { 255 } else { 0 }])
        });
        let material = RgbaImage::from_fn(2, 2, |x, _| Rgba(if x == 0 { visible } else { hidden }));
        for overlay_mask in [false, true] {
            let mip = build_pbr_mips(
                &surface(
                    color.clone(),
                    rgba([128, 128, 255, 128]),
                    material.clone(),
                    PBR_REF_MATERIAL | PBR_REF_LABPBR,
                ),
                2,
                false,
                overlay_mask,
            )
            .unwrap();
            let expected = if overlay_mask {
                [237, 0, 64, 200]
            } else {
                visible
            };
            assert_eq!(mip.material[1].rgba8[0], expected[0]);
            assert_eq!(mip.material[1].rgba8[3], expected[3]);
        }
    }
}

#[test]
fn labpbr_category_averages_and_metal_modes_use_opacity_weights() {
    for (values, expected) in [([20, 100, 20, 100], 40), ([230, 237, 230, 237], 230)] {
        let color = RgbaImage::from_fn(2, 2, |x, _| {
            Rgba([255, 255, 255, if x == 0 { 255 } else { 85 }])
        });
        let material = RgbaImage::from_fn(2, 2, |x, y| {
            Rgba([
                values[(y * 2 + x) as usize],
                0,
                64,
                if x == 0 { 100 } else { 200 },
            ])
        });
        let mip = build_pbr_mips(
            &surface(
                color,
                rgba([128, 128, 255, 128]),
                material,
                PBR_REF_MATERIAL | PBR_REF_LABPBR,
            ),
            2,
            false,
            false,
        )
        .unwrap();
        assert_eq!(mip.material[1].rgba8[0], expected);
        assert_eq!(mip.material[1].rgba8[3], 125);
    }
}

#[test]
fn image_bytes_identify_jpeg_payloads_even_when_the_file_suffix_is_png() {
    let directory = tempfile::tempdir().unwrap();
    save(
        directory.path(),
        "assets/minecraft/textures/block/stone.png",
        &rgba([128, 128, 128, 255]),
    );
    let path = directory
        .path()
        .join("assets/minecraft/textures/block/stone_normal.png");
    let image = ImageBuffer::from_pixel(2, 2, Rgb([128_u8, 128, 255]));
    image
        .save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
    let frame = load_pbr_texture(
        &[PbrPack::new(directory.path(), None)],
        "textures/blocks/stone",
    )
    .unwrap()
    .unwrap()
    .frame(0, 1)
    .unwrap();
    assert_ne!(frame.flags & PBR_REF_NORMAL, 0);
}

#[test]
fn available_faithful_materials_keep_the_512_target_and_declared_labpbr_data() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/FaithfulPBR_512");
    if !root.is_dir() {
        eprintln!("missing fixture: local FaithfulPBR_512 authored pack");
        return;
    }
    let texture = load_pbr_texture(&[PbrPack::new(root, None)], "textures/blocks/stone")
        .unwrap()
        .unwrap();
    let frame = texture.frame(0, 1).unwrap();
    assert_ne!(frame.flags & PBR_REF_LABPBR, 0);
    let mip = build_pbr_mips(&frame, assets::texture::PBR_TILE_SIZE, false, false).unwrap();
    assert_eq!(mip.color[0].size, assets::texture::PBR_TILE_SIZE);
    assert_eq!(mip.normal[0].size, assets::texture::PBR_TILE_SIZE);
    assert_eq!(mip.material[0].size, assets::texture::PBR_TILE_SIZE);
    assert_eq!(mip.color.last().unwrap().size, 1);
}

#[test]
fn overlay_mask_mips_preserve_dirt_rgb_and_filter_tint_weights_independently() {
    let color = RgbaImage::from_fn(2, 2, |x, _| {
        Rgba(if x == 0 {
            [255, 0, 0, 0]
        } else {
            [0, 255, 0, 255]
        })
    });
    let mips = build_pbr_mips(
        &surface(
            color,
            RgbaImage::from_fn(2, 2, |x, _| {
                Rgba([128, 128, if x == 0 { 128 } else { 255 }, 128])
            }),
            RgbaImage::from_fn(2, 2, |x, _| Rgba([if x == 0 { 0 } else { 255 }, 0, 255, 0])),
            0,
        ),
        2,
        false,
        true,
    )
    .unwrap();
    assert_eq!(&mips.color[0].rgba8[..4], &[255, 0, 0, 0]);
    assert_eq!(&mips.color[1].rgba8[..4], &[188, 188, 0, 128]);
    assert_eq!(mips.normal[1].rgba8[2], 192);
    assert_eq!(mips.material[1].rgba8[0], 128);
}

#[test]
fn explicit_color_timeline_resamples_unequal_companions_and_keeps_matching_order() {
    let directory = tempfile::tempdir().unwrap();
    let stem = "assets/minecraft/textures/block/water_still";
    save(
        directory.path(),
        &format!("{stem}.png"),
        &RgbaImage::from_fn(2, 8, |_, y| Rgba([(100 + y / 2) as u8, 0, 0, 255])),
    );
    fs::write(
        directory.path().join(format!("{stem}.png.mcmeta")),
        br#"{"animation":{"frames":[3,1,0,2]}}"#,
    )
    .unwrap();
    for companion_count in [1, 2, 4, 6] {
        save(
            directory.path(),
            &format!("{stem}_n.png"),
            &RgbaImage::from_fn(2 * companion_count, 2, |x, _| {
                Rgba([128, 128, (100 + x / 2) as u8, 128])
            }),
        );
        save(
            directory.path(),
            &format!("{stem}_mer.png"),
            &RgbaImage::from_fn(2, 2 * companion_count, |_, y| {
                Rgba([(20 * (y / 2)) as u8, 0, 200, 255])
            }),
        );
        save(
            directory.path(),
            &format!("{stem}_h.png"),
            &RgbaImage::from_fn(2, 2 * companion_count, |_, y| {
                Rgba([(10 + 20 * (y / 2)) as u8, 0, 0, 255])
            }),
        );
        let texture = load_pbr_texture(
            &[PbrPack::new(directory.path(), Some(PbrFormat::LabPbr13))],
            "textures/blocks/water_still",
        )
        .unwrap()
        .unwrap();
        for index in 0..8 {
            let color_frame = [3, 1, 0, 2][index / 2];
            let companion_frame = if companion_count == 4 {
                color_frame
            } else {
                index as u32 * companion_count / 8
            };
            let frame = texture.frame(index, 8).unwrap();
            assert_eq!(frame.color.get_pixel(0, 0)[0], (100 + color_frame) as u8);
            assert_eq!(
                frame.normal.get_pixel(0, 0)[2],
                (100 + companion_frame) as u8
            );
            assert_eq!(
                frame.normal.get_pixel(0, 0)[3],
                (10 + 20 * companion_frame) as u8
            );
            assert_eq!(
                frame.material.get_pixel(0, 0)[0],
                (20 * companion_frame) as u8
            );
        }
    }
}

#[test]
fn animation_frames_inherit_default_duration_but_keep_explicit_overrides() {
    let directory = tempfile::tempdir().unwrap();
    let stem = "assets/minecraft/textures/block/water_still";
    save(
        directory.path(),
        &format!("{stem}.png"),
        &RgbaImage::from_fn(2, 4, |_, y| Rgba([if y < 2 { 20 } else { 80 }, 0, 0, 255])),
    );
    for frame_list in [
        r#"[0,{"index":1,"time":1}]"#,
        r#"[{"index":0},{"index":1,"time":1}]"#,
    ] {
        fs::write(
            directory.path().join(format!("{stem}.png.mcmeta")),
            format!(r#"{{"animation":{{"frametime":4,"frames":{frame_list}}}}}"#),
        )
        .unwrap();
        let texture = load_pbr_texture(
            &[PbrPack::new(directory.path(), None)],
            "textures/blocks/water_still",
        )
        .unwrap()
        .unwrap();
        for index in 0..5 {
            assert_eq!(
                texture.frame(index, 5).unwrap().color.get_pixel(0, 0)[0],
                if index < 4 { 20 } else { 80 }
            );
        }
    }
}
