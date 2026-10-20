use super::*;

#[test]
fn pack_json_matches_previous_normalized_bytes() {
    for bytes in [
        br#"{"texture_data":{"a":{"textures":"textures/first"},"a":{"textures":"textures/last"}},"number":1.0000000000000002}"#.as_slice(),
        b"\xef\xbb\xbf{/* catalog */\"texture_data\":{\"caf\xe9\":{\"textures\":\"textures/a\",},},}",
        br#"{"number":18446744073709551616}"#,
        b"{\"texture_data\":{,}}",
        b"{} {}",
        b"1e309",
    ] {
        let before = bedrock_packs::jsonc::normalize_jsonc(bytes)
            .and_then(|normalized| serde_json::from_slice::<Value>(&normalized).ok());
        assert_eq!(
            parse_pack_json(bytes).map(|value| serde_json::to_vec(&value).unwrap()),
            before.map(|value| serde_json::to_vec(&value).unwrap()),
        );
    }
}

#[test]
fn full_catalogs_keep_later_valid_overrides_after_skipping_new_keys() {
    let mut paths = (0..MAX_CATALOG_ENTRIES)
        .map(|index| (format!("z_{index:05}"), "textures/old".to_owned()))
        .collect::<HashMap<_, _>>();
    let mut grids = paths
        .keys()
        .map(|key| (key.clone(), 1))
        .collect::<HashMap<_, _>>();
    let layer = br#"{"texture_data":{
        "a_new":{"textures":"textures/unadmitted"},
        "z_00000":{"textures":"textures/replacement"},
        "z_00001":{"textures":false},
        "z_00002":{"textures":"textures/gridded","quad":1}
    }}"#;
    merge_texture_catalog(&mut paths, layer);
    merge_terrain_grids(&mut grids, layer);
    assert_eq!(paths.len(), MAX_CATALOG_ENTRIES);
    assert_eq!(grids.len(), MAX_CATALOG_ENTRIES);
    assert!(!paths.contains_key("a_new"));
    assert!(!grids.contains_key("a_new"));
    assert_eq!(paths["z_00000"], "textures/replacement");
    assert_eq!(
        grids["z_00000"], 0,
        "omitted grids reset on a valid override"
    );
    assert_eq!(paths["z_00001"], "textures/old");
    assert_eq!(
        grids["z_00001"], 1,
        "malformed overrides retain the lower grid"
    );
    assert_eq!(paths["z_00002"], "textures/gridded");
    assert_eq!(grids["z_00002"], 1 & assets::TERRAIN_QUAD_SHIFT_MASK as u8);
}

/// Encodes original row and column detail into a synthetic PNG.
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([x as u8, y as u8, (y / width) as u8, 255])
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[test]
fn large_strips_and_particle_atlases_retain_every_source_pixel() {
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    for (width, height, strip) in [
        (64, 2048, true),
        (128, 4096, true),
        (64, 8384, true),
        (2048, 2048, false),
    ] {
        let bytes = png(width, height);
        let view =
            LayeredPackView::new(crate::test_support::stack(&[("textures/test.png", &bytes)]));
        let texture =
            decode_pack_texture_with_policy(&view, "textures/test", policy, strip).unwrap();
        assert_eq!([texture.width, texture.height], [width, height]);
        assert_eq!(texture.rgba8.len(), (width * height * 4) as usize);
        let x = width - 1;
        let y = height - 1;
        assert_eq!(
            &texture.rgba8[((y * width + x) * 4) as usize..][..4],
            &[x as u8, y as u8, (y / width) as u8, 255]
        );
        assert!(decode_pack_texture(&view, "textures/test").is_none());
    }
}

#[test]
fn horizontal_strip_decoding_checks_both_axes_before_pixel_allocation() {
    let policy = TexturePolicy::for_device(64);
    for (width, height) in [(8192, 64), (64, 8192)] {
        let bytes = png(width, height);
        let view =
            LayeredPackView::new(crate::test_support::stack(&[("textures/test.png", &bytes)]));
        let texture =
            decode_pack_texture_with_policy(&view, "textures/test", policy, true).unwrap();
        assert_eq!([texture.width, texture.height], [width, height]);
        assert_eq!(texture.rgba8.len(), (width * height * 4) as usize);
        assert_eq!(
            &texture.rgba8[texture.rgba8.len() - 4..],
            &[
                (width - 1) as u8,
                (height - 1) as u8,
                ((height - 1) / width) as u8,
                255
            ]
        );
        assert!(decode_pack_texture_with_policy(&view, "textures/test", policy, false).is_none());
    }
    for (width, height) in [(8193, 64), (8192, 65)] {
        assert!(
            decode_image_with_policy(&png(width, height), ImageFormat::Png, policy, true).is_none()
        );
    }
}

#[test]
fn malformed_over_budget_and_device_oversized_images_are_rejected() {
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    assert!(decode_image_with_policy(b"broken", ImageFormat::Png, policy, true).is_none());
    assert!(decode_image_with_policy(&png(2048, 4096), ImageFormat::Png, policy, true).is_none());
    assert!(decode_image_with_policy(&png(64, 8385), ImageFormat::Png, policy, true).is_none());
    assert!(
        decode_image_with_policy(
            &png(2048, 2048),
            ImageFormat::Png,
            TexturePolicy::for_device(1024),
            false
        )
        .is_none()
    );
}

#[test]
fn resolved_images_keep_the_filename_selected_after_decode_and_texture_set_fallbacks() {
    let mut tga = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 30, 200, 255]))
        .write_to(&mut tga, ImageFormat::Tga)
        .unwrap();
    let png = png(16, 16);
    let view = LayeredPackView::new(crate::test_support::stack(&[
        ("textures/fallback.png", b"invalid image"),
        ("textures/fallback.tga", tga.get_ref()),
        ("textures/shared.png", &png),
        (
            "textures/alias.texture_set.json",
            br#"{"minecraft:texture_set":{"color":"shared"}}"#,
        ),
        (
            "textures/solid.texture_set.json",
            br#"{"minecraft:texture_set":{"color":[1,2,3,255]}}"#,
        ),
    ]));
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    let fallback =
        resolve_pack_texture_with_policy(&view, "textures/fallback", policy, false).unwrap();
    assert_eq!(fallback.image_path, "textures/fallback.tga");
    assert_eq!(&fallback.texture.rgba8[..4], &[20, 30, 200, 255]);
    for path in [
        "textures/shared",
        "./textures/shared.png",
        "textures/shared.PNG",
        "textures/alias",
    ] {
        let image = resolve_pack_texture_with_policy(&view, path, policy, false).unwrap();
        assert_eq!(image.image_path, "textures/shared.png");
    }
    let solid = resolve_pack_texture_with_policy(&view, "textures/solid", policy, false).unwrap();
    assert_eq!(solid.image_path, "textures/solid.texture_set.json");
    assert_eq!(&solid.texture.rgba8[..], &[1, 2, 3, 255]);
}

#[test]
fn indexed_alias_candidates_preserve_precedence_without_reading_image_payloads() {
    let stack = crate::test_support::stack(&[
        ("textures/shared.png", b"metadata only"),
        ("textures/shared.tga", b"metadata only"),
        (
            "textures/alias.texture_set.json",
            br#"{"minecraft:texture_set":{"color":"shared"}}"#,
        ),
    ]);
    let view = LayeredPackView::tracked(stack);
    for path in ["textures/shared", "./textures/shared.PNG", "textures/alias"] {
        assert_eq!(
            indexed_image_candidate(&view, path).as_deref(),
            Some(if path.ends_with(".PNG") {
                "textures/shared.png"
            } else {
                "textures/shared.tga"
            })
        );
    }
    assert_eq!(indexed_image_candidate(&view, "textures/uncached"), None);
    let dependencies = view.dependencies().unwrap().snapshot();
    assert!(!dependencies.iter().any(|dependency| matches!(dependency,
        resource_pack::PackDependency::File { path, limit }
            if (path.ends_with(".png") || path.ends_with(".tga"))
                && *limit == resource_pack::MAX_PACK_TEXTURE_BYTES
    )));
}

#[test]
fn extensionless_texture_decodes_tga_ahead_of_darker_png() {
    let mut png = Cursor::new(Vec::new());
    let mut tga = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([110, 110, 110, 255]))
        .write_to(&mut png, ImageFormat::Png)
        .unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([145, 145, 145, 255]))
        .write_to(&mut tga, ImageFormat::Tga)
        .unwrap();
    let view = LayeredPackView::new(crate::test_support::stack(&[
        ("textures/grass.png", png.get_ref()),
        ("textures/grass.tga", tga.get_ref()),
    ]));
    let policy = TexturePolicy::for_device(assets::texture::policy::MAX_TEXTURE_SIDE);
    let resolved =
        resolve_pack_texture_with_policy(&view, "textures/grass", policy, false).unwrap();
    assert_eq!(resolved.image_path, "textures/grass.tga");
    assert_eq!(&resolved.texture.rgba8[..4], &[145, 145, 145, 255]);
    let explicit =
        resolve_pack_texture_with_policy(&view, "textures/grass.png", policy, false).unwrap();
    assert_eq!(&explicit.texture.rgba8[..4], &[110, 110, 110, 255]);
}
