use super::*;

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
