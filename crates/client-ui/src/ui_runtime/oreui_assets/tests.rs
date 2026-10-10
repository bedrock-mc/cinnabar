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

/// Extracts a sprite's straight RGBA8 pixels without including atlas padding.
fn pixels(images: &OreUiImages, key: &str) -> Vec<u8> {
    let sprite = images.sprites[key];
    let page = &images.pages[usize::from(sprite.page)];
    let [left, top, right, bottom] = sprite.bounds.map(usize::from);
    (top..bottom)
        .flat_map(|y| {
            let start = (y * page.dimensions[0] as usize + left) * 4;
            page.pixels[start..start + (right - left) * 4]
                .iter()
                .copied()
        })
        .collect()
}

#[test]
fn shipped_art_resolves_without_game_install() {
    let directory = tempfile::tempdir().unwrap();
    let images = load_oreui_images(directory.path());
    for key in referenced_keys() {
        assert!(images.contains(key), "missing shipped artwork: {key}");
    }
    for source in embedded::ALL {
        assert!(
            images.contains(source.key),
            "missing manifest art: {}",
            source.key
        );
    }
    assert_eq!(referenced_keys().count(), embedded::ALL.len());
    assert!(
        images
            .pages
            .iter()
            .map(|page| page.pixels.len())
            .sum::<usize>()
            < render_model::MAX_UI_FIXED_TEXTURE_BYTES
    );
    assert_eq!(
        images.loading_frames.len(),
        usize::from(embedded::LOADING.frames)
    );
}

#[test]
fn missing_or_corrupt_panorama_carrier_uses_original_banner_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let originals = shipped_oreui_images();
    let missing = load_oreui_images(directory.path());
    std::fs::write(
        directory
            .path()
            .join(assets::carriers::OREUI_PANORAMAS.output),
        b"corrupt",
    )
    .unwrap();
    let corrupt = load_oreui_images(directory.path());
    for key in PROFILE_BANNERS.into_iter().chain([WORLD_PREVIEW]) {
        assert_eq!(pixels(&missing, key), pixels(&originals, key));
        assert_eq!(pixels(&corrupt, key), pixels(&originals, key));
    }
}

#[test]
fn panorama_carrier_overrides_only_the_banners_and_world_preview() {
    use assets::oreui_panorama::{self, BANNER_COUNT, IMAGE_BYTES, OreUiPanoramas};
    let directory = tempfile::tempdir().unwrap();
    let panoramas = OreUiPanoramas {
        images: std::array::from_fn(|index| {
            [index as u8, 90, 140, 255].repeat(IMAGE_BYTES / 4).into()
        }),
    };
    std::fs::write(
        directory
            .path()
            .join(assets::carriers::OREUI_PANORAMAS.output),
        oreui_panorama::encode(&panoramas).unwrap(),
    )
    .unwrap();
    let images = load_oreui_images(directory.path());
    for (index, key) in PROFILE_BANNERS
        .into_iter()
        .chain([WORLD_PREVIEW])
        .enumerate()
    {
        assert_eq!(pixels(&images, key), panoramas.images[index].as_ref());
    }
    assert_eq!(PROFILE_BANNERS.len(), BANNER_COUNT);
    assert_eq!(
        pixels(&images, PLAY_TAB_ICONS[0]),
        pixels(&shipped_oreui_images(), PLAY_TAB_ICONS[0])
    );
}

#[test]
fn embedded_masks_keep_original_alpha() {
    let images = shipped_oreui_images();
    for source in embedded::ALL.iter().filter(|source| is_mask(source.key)) {
        let original = pixels(&images, source.key);
        assert_eq!(
            pixels(&images, &format!("@mask/{}", source.key)),
            alpha_mask(&original)
        );
    }
}
