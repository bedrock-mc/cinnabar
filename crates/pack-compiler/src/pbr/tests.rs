use std::{fs, path::Path};

use image::{ImageBuffer, Rgb, Rgba, RgbaImage};

use super::*;

fn save(root: &Path, name: &str, image: &RgbaImage) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image.save(path).unwrap();
}

fn rgba(pixel: [u8; 4]) -> RgbaImage {
    ImageBuffer::from_pixel(2, 2, Rgba(pixel))
}

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
    )
    .unwrap();
    let middle = &mip.color[1].rgba8;
    assert_eq!(
        middle
            .chunks_exact(4)
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
    )
    .unwrap();
    let normal = &mip.normal[1].rgba8;
    assert!((normal[0] as i32 - 128).abs() <= 1);
    assert!((normal[1] as i32 - 128).abs() <= 1);
    assert_eq!(normal[2..], [255, 128]);
    assert!(mip.material[1].rgba8[2] > mip.material[0].rgba8[2] + 64);
    let material = RgbaImage::from_fn(2, 2, |x, _| {
        Rgba([if x == 0 { 230 } else { 237 }, 0, 64, 12])
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
    )
    .unwrap();
    assert!([230, 237].contains(&mip.material[1].rgba8[0]));
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
    )
    .unwrap();
    assert!(
        mip.material[0]
            .rgba8
            .chunks_exact(4)
            .all(|pixel| [229, 237].contains(&pixel[0])),
        "initial normalization must preserve encoded F0/metal identity"
    );
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
    let mip = build_pbr_mips(&frame, assets::PBR_TILE_SIZE, false).unwrap();
    assert_eq!(mip.color[0].size, assets::PBR_TILE_SIZE);
    assert_eq!(mip.normal[0].size, assets::PBR_TILE_SIZE);
    assert_eq!(mip.material[0].size, assets::PBR_TILE_SIZE);
    assert_eq!(mip.color.last().unwrap().size, 1);
}
