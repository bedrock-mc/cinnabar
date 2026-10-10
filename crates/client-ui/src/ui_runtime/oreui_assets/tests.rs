use super::*;

/// Makes authored GIF bytes without requiring any installed assets.
fn synthetic_animation(count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        for index in 0..count {
            let pixels =
                image::RgbaImage::from_pixel(2, 2, image::Rgba([index as u8, 120, 40, 255]));
            encoder
                .encode_frame(image::Frame::from_parts(
                    pixels,
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(100, 1),
                ))
                .unwrap();
        }
    }
    bytes
}

#[test]
fn animated_loading_keeps_each_frame_and_delay() {
    let frames = decode_animation(&synthetic_animation(3)).unwrap();
    assert_eq!(frames.len(), 3);
    for (index, (width, height, pixels, millis)) in frames.iter().enumerate() {
        assert_eq!((*width, *height, *millis), (2, 2, 100));
        assert_eq!(&pixels[..4], &[index as u8, 120, 40, 255]);
    }
    assert!(decode_animation(&synthetic_animation(MAX_ANIMATION_FRAMES + 1)).is_err());
    assert!(decode_animation(b"invalid GIF").is_err());
}

#[test]
fn profile_banners_fit_beside_existing_atlas_without_resizing() {
    let dir = std::env::temp_dir().join(format!("oreui-profile-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    image::RgbaImage::from_pixel(1024, 1024, image::Rgba([0, 0, 0, 255]))
        .save(dir.join("base.png"))
        .unwrap();
    std::fs::write(
        dir.join("atlas.json"),
        r#"[{"name":"base.png","width":1024,"height":1024,"coordinates":{}}]"#,
    )
    .unwrap();
    // The reference's eight banner images are 960 by 540 pixels.
    for name in PROFILE_BANNERS {
        image::RgbImage::from_pixel(960, 540, image::Rgb([80, 120, 160]))
            .save(dir.join(name))
            .unwrap();
    }
    let images = load(&dir).unwrap();
    for name in PROFILE_BANNERS {
        let [left, top, right, bottom] = images.sprites[name].bounds;
        assert_eq!((right - left, bottom - top), (960, 540));
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn atlas_crops_keep_their_original_pixels() {
    let dir = std::env::temp_dir().join(format!("oreui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_fn(4, 2, |x, y| {
        image::Rgba([x as u8 * 50, y as u8 * 70, 9, 255])
    })
    .save(dir.join("a.png"))
    .unwrap();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 128]))
        .save(dir.join("b.png"))
        .unwrap();
    std::fs::write(
        dir.join("atlas.json"),
        serde_json::to_vec(&serde_json::json!([
            {"name": "a.png", "width": 4, "height": 2, "coordinates": {
                (WORLD_CATEGORY_ICONS[0]): {"x": 1, "y": 0, "width": 2, "height": 2}
            }},
            {"name": "b.png", "width": 2, "height": 2, "coordinates": {
                (WORLD_CATEGORY_ICONS[1]): {"x": 0, "y": 0, "width": 2, "height": 2},
                (WORLD_CATEGORY_ICONS[2]): {"x": 1, "y": 1, "width": 5, "height": 5}
            }}
        ]))
        .unwrap(),
    )
    .unwrap();
    let images = load(&dir).unwrap();
    for key in &WORLD_CATEGORY_ICONS[..2] {
        let [left, top, right, bottom] = images.sprites[*key].bounds;
        assert_eq!((right - left, bottom - top), (2, 2));
    }
    assert!(!images.sprites.contains_key(WORLD_CATEGORY_ICONS[2]));
    let sprite = images.sprites[WORLD_CATEGORY_ICONS[0]];
    let page = &images.pages[usize::from(sprite.page)];
    for (dx, dy, expected) in [(0, 0, [50, 0, 9, 255]), (1, 1, [100, 70, 9, 255])] {
        let pixel = ((u32::from(sprite.bounds[1]) + dy) * page.dimensions[0]
            + u32::from(sprite.bounds[0])
            + dx) as usize
            * 4;
        assert_eq!(&page.pixels[pixel..pixel + 4], &expected);
    }
    let sprite = images.sprites[WORLD_CATEGORY_ICONS[1]];
    let page = &images.pages[usize::from(sprite.page)];
    let pixel = ((u32::from(sprite.bounds[1]) * page.dimensions[0] + u32::from(sprite.bounds[0]))
        * 4) as usize;
    assert_eq!(&page.pixels[pixel..pixel + 4], &[0, 255, 0, 128]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn standalone_art_masks_and_demand_pages_keep_source_pixels_and_reuse_cache() {
    let dir = std::env::temp_dir().join(format!("oreui-catalog-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("atlas.json"), "[]").unwrap();
    let icon = "assets/unlisted.icon-example.png";
    let background = "assets/unlisted-background.png";
    let animation = "assets/unlisted-animation.gif";
    image::RgbaImage::from_pixel(13, 7, image::Rgba([19, 22, 24, 128]))
        .save(dir.join(icon))
        .unwrap();
    image::RgbaImage::from_pixel(3840, 1, image::Rgba([40, 70, 90, 128]))
        .save(dir.join(background))
        .unwrap();
    std::fs::write(dir.join(animation), synthetic_animation(2)).unwrap();
    let core = load(&dir).unwrap();
    assert!(core.contains(background) && core.contains(animation));
    assert!(!core.sprites.contains_key(background));
    assert!(!core.sprites.contains_key(animation));
    let texel = |images: &OreUiImages, key: &str| {
        let sprite = images.sprites[key];
        let page = &images.pages[usize::from(sprite.page)];
        let start = (usize::from(sprite.bounds[1]) * page.dimensions[0] as usize
            + usize::from(sprite.bounds[0]))
            * 4;
        <[u8; 4]>::try_from(&page.pixels[start..start + 4]).unwrap()
    };
    assert!(!core.sprites.contains_key(icon));
    let icons = core.with_artwork(&[icon]).unwrap();
    assert_eq!(texel(&icons, icon), [19, 22, 24, 128]);
    assert_eq!(
        texel(&icons, &format!("@mask/{icon}")),
        [255, 255, 255, 128]
    );
    image::RgbaImage::from_pixel(3840, 1, image::Rgba([100, 70, 90, 128]))
        .save(dir.join(background))
        .unwrap();
    let first = core.with_artwork(&[background, animation]).unwrap();
    assert_eq!(texel(&first, background), [100, 70, 90, 128]);
    assert_eq!(
        first.pages[usize::from(first.sprites[background].page)].dimensions,
        [3840, 1]
    );
    assert_eq!(first.animations[animation].len(), 2);
    std::fs::remove_file(dir.join(background)).unwrap();
    std::fs::remove_file(dir.join(animation)).unwrap();
    let second = core
        .with_artwork(&[animation, background, background])
        .unwrap();
    assert!(
        first
            .pages
            .iter()
            .zip(&second.pages)
            .all(|(a, b)| Arc::ptr_eq(&a.pixels, &b.pixels))
    );
    assert!(core.with_artwork(&["../outside.png"]).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn installed_core_has_native_settings_art_within_texture_budget() {
    let Some(dir) = bundle_dir() else {
        eprintln!(
            "skipping installed_core_has_native_settings_art_within_texture_budget: installed OreUI bundle unavailable"
        );
        return;
    };
    let images = load(&dir).unwrap();
    for key in SETTINGS_ICONS.into_iter().chain([
        OVERWORLD_BLOCK_IMAGE,
        SETTINGS_ICON_HIGHLIGHT_IMAGE,
        SWITCH_ON_IMAGE,
        SWITCH_OFF_IMAGE,
        CHEVRON_LEFT_IMAGE,
    ]) {
        assert!(
            images.sprites.contains_key(key),
            "missing native settings art: {key}"
        );
    }
    for key in [SWITCH_ON_IMAGE, SWITCH_OFF_IMAGE, CHEVRON_LEFT_IMAGE] {
        assert!(images.sprites.contains_key(&format!("@mask/{key}")));
    }
    let mut files = Vec::new();
    collect_rasters(&dir, &dir.join("assets"), &mut files).unwrap();
    assert!(files.iter().all(|(key, _)| images.contains(key)));
    assert_eq!(images.source.as_ref().unwrap().len(), files.len());
    assert!(
        images
            .pages
            .iter()
            .map(|page| page.pixels.len())
            .sum::<usize>()
            < render_model::MAX_UI_FIXED_TEXTURE_BYTES / 2
    );
}

#[test]
fn startup_does_not_read_unreferenced_rasters() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("atlas.json"), "[]").unwrap();
    image::RgbImage::from_pixel(960, 540, image::Rgb([80, 120, 160]))
        .save(dir.path().join(WORLD_PREVIEW))
        .unwrap();
    std::fs::write(dir.path().join("assets/unreferenced.png"), b"invalid image").unwrap();
    let images = load(dir.path()).expect("unreferenced raster must not be read");
    assert!(images.sprites.contains_key(WORLD_PREVIEW));
    assert!(!images.sprites.contains_key("assets/unreferenced.png"));
}

#[test]
fn startup_does_not_decode_unreferenced_atlases() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("atlas.json"),
        r#"[{"name":"missing.png","width":2,"height":2,"coordinates":{"assets/unused.png":{"x":0,"y":0,"width":2,"height":2}}}]"#).unwrap();
    let images = load(dir.path()).expect("unreferenced atlas must not be read");
    assert!(images.pages.is_empty());
    assert!(images.sprites.is_empty());
}

#[test]
fn every_referenced_key_and_sleep_animation_loads_from_standalone_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("atlas.json"), "[]").unwrap();
    for key in referenced_keys() {
        if key.ends_with(".gif") {
            std::fs::write(dir.path().join(key), synthetic_animation(2)).unwrap();
        } else {
            image::RgbImage::from_pixel(13, 7, image::Rgb([17, 40, 90]))
                .save(dir.path().join(key))
                .unwrap();
        }
    }
    let sleeps = [
        format!("{SLEEP_ANIMATION_PREFIX}one.gif"),
        format!("{SLEEP_ANIMATION_PREFIX}two.gif"),
    ];
    for key in &sleeps {
        std::fs::write(dir.path().join(key), synthetic_animation(2)).unwrap();
    }
    let images = load(dir.path()).unwrap();
    for key in referenced_keys() {
        assert!(
            images.sprites.contains_key(key),
            "referenced art missing: {key}"
        );
        if is_mask(key) {
            assert!(
                images.sprites.contains_key(&format!("@mask/{key}")),
                "mask missing: {key}"
            );
        }
    }
    assert_eq!(images.loading_frames.len(), 2);
    for key in sleeps {
        assert_eq!(images.animations[&key].len(), 2);
        assert_eq!(images.animations[&key][1].1, 100);
    }
}

#[test]
fn only_referenced_atlas_rectangles_are_packed() {
    let dir = tempfile::tempdir().unwrap();
    image::RgbaImage::from_pixel(4, 4, image::Rgba([19, 22, 24, 128]))
        .save(dir.path().join("atlas.png"))
        .unwrap();
    let coordinates: HashMap<_, _> = referenced_keys()
        .map(|key| {
            (
                key,
                serde_json::json!({"x": 1, "y": 1, "width": 2, "height": 2}),
            )
        })
        .chain([(
            "assets/unused.png",
            serde_json::json!({"x": 0, "y": 0, "width": 4, "height": 4}),
        )])
        .collect();
    std::fs::write(
        dir.path().join("atlas.json"),
        serde_json::to_vec(&serde_json::json!([
            {"name": "atlas.png", "width": 4, "height": 4, "coordinates": coordinates}
        ]))
        .unwrap(),
    )
    .unwrap();
    let images = load(dir.path()).unwrap();
    for key in referenced_keys() {
        let sprite = images.sprites[key];
        assert_eq!(
            [
                sprite.bounds[2] - sprite.bounds[0],
                sprite.bounds[3] - sprite.bounds[1]
            ],
            [2, 2]
        );
    }
    assert!(!images.sprites.contains_key("assets/unused.png"));
}
