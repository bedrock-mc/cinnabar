use super::*;
use crate::runtime::network::reload_environment::tests::{stack, view};
use render_scene::screen_overlay::{PUMPKIN_MASK_PATH, SPYGLASS_MASK_PATH};

/// Encodes an original constant-color image with the requested dimensions.
fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba(pixel));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// Creates immutable startup masks that are distinguishable from every optional override.
fn masks() -> Arc<ScreenOverlayTextures> {
    let bytes = (render_scene::SCREEN_OVERLAY_TEXTURE_SIDE.pow(2) * 4) as usize;
    Arc::new(ScreenOverlayTextures::new(vec![21; bytes], vec![42; bytes]).unwrap())
}

/// Creates a valid startup weather carrier with its fixed schema dimensions.
fn weather() -> WeatherTextureAssets {
    use assets::weather_textures::{END_SKY_SIDE, WEATHER_SHEET_SIDE, WeatherTextures};
    let image = |side: u32, pixel: u8| WeatherImage {
        width: side,
        height: side,
        rgba8: vec![pixel; (side * side * 4) as usize].into(),
    };
    WeatherTextureAssets::new(
        Arc::new(WeatherTextures {
            weather: image(WEATHER_SHEET_SIDE, 5),
            end_sky: image(END_SKY_SIDE, 7),
        }),
        [7; 32],
    )
}

#[test]
fn pumpkin_priority_preserves_alpha_and_removal_restores_startup_identity() {
    let base = masks();
    let path = PUMPKIN_MASK_PATH.to_owned();
    let lower = png(32, 32, [255, 0, 0, 255]);
    let upper = png(16, 16, [0, 255, 0, 0]);
    let layers = stack(&[&[(&path, &lower)], &[(&path, &upper)]]);
    let prepared = prepare_screen_overlays(&layers, Some(&base)).unwrap();
    assert_eq!(&prepared.pumpkin().unwrap()[..4], &[0, 255, 0, 0]);
    assert_eq!(prepared.spyglass(), base.spyglass());
    assert_eq!(
        &prepare_screen_overlays(&view(&[(&path, &lower)]), Some(&base))
            .unwrap()
            .pumpkin()
            .unwrap()[..4],
        &[255, 0, 0, 255]
    );
    assert!(Arc::ptr_eq(
        &base,
        &prepare_screen_overlays(&view(&[]), Some(&base)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        &base,
        &prepare_screen_overlays(&view(&[(&path, b"invalid image")]), Some(&base)).unwrap()
    ));
}

#[test]
fn masks_work_independently_without_a_startup_carrier() {
    for (path, pumpkin) in [(PUMPKIN_MASK_PATH, true), (SPYGLASS_MASK_PATH, false)] {
        let path = path.to_owned();
        let png = png(8, 8, [1, 2, 3, 128]);
        let prepared = prepare_screen_overlays(&view(&[(&path, &png)]), None).unwrap();
        assert_eq!(prepared.pumpkin().is_some(), pumpkin);
        assert_eq!(prepared.spyglass().is_some(), !pumpkin);
        let bytes = prepared.layer_major();
        let start = if pumpkin { 0 } else { bytes.len() / 2 };
        assert_eq!(&bytes[start..start + 4], &[1, 2, 3, 128]);
    }
    assert!(prepare_screen_overlays(&view(&[]), None).is_none());
}

#[test]
fn end_sky_keeps_512_pixels_and_restores_base_on_removal() {
    let base = weather();
    let path = format!("{END_SKY_TEXTURE_PATH}.png");
    let lower = png(128, 128, [255, 0, 0, 255]);
    let upper = png(512, 512, [0, 0, 255, 255]);
    let prepared = prepare_weather(&stack(&[&[(&path, &lower)], &[(&path, &upper)]]), &base);
    let image = prepared.end_sky().unwrap();
    assert_eq!((image.width, image.height), (512, 512));
    assert_eq!(&image.rgba8[..4], &[0, 0, 255, 255]);
    assert_ne!(prepared.identity(), base.identity());
    assert_eq!(
        prepared.identity(),
        prepare_weather(&view(&[(&path, &upper)]), &base).identity()
    );
    assert!(Arc::ptr_eq(
        prepared.textures().unwrap(),
        base.textures().unwrap()
    ));
    let restored = prepare_weather(&view(&[]), &base);
    assert_eq!(restored.identity(), base.identity());
    assert_eq!(restored.end_sky(), base.end_sky());
    assert_eq!(
        prepare_weather(&view(&[(&path, b"invalid image")]), &base).identity(),
        base.identity()
    );
    let optional = prepare_weather(&view(&[(&path, &upper)]), &WeatherTextureAssets::default());
    assert_eq!(optional.end_sky().unwrap().width, 512);
}

#[test]
fn environment_dependencies_notice_masks_and_end_sky_added_after_an_empty_stack() {
    use pack_runtime::reload_diff::{Changes, Subscriber, compile};
    let old = view(&[]);
    let mut dependencies = Default::default();
    compile(
        Subscriber::Atmosphere,
        &old.shared_stack(),
        &mut dependencies,
        |view| {
            prepare_screen_overlays(view, None);
            prepare_weather(view, &WeatherTextureAssets::default());
        },
    );
    for path in [PUMPKIN_MASK_PATH, SPYGLASS_MASK_PATH, END_SKY_TEXTURE_PATH] {
        let path = if path == END_SKY_TEXTURE_PATH {
            format!("{path}.png")
        } else {
            path.to_owned()
        };
        let image = png(16, 16, [3, 4, 5, 255]);
        let next = view(&[(&path, &image)]);
        let old_admission = resource_pack::PackAdmission::Validated(old.shared_stack());
        assert!(Changes::between(next.stack(), Some((&old_admission, &dependencies))).atmosphere);
    }
}
