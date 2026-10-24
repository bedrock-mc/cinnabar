use super::*;

/// Encodes an original synthetic image through the same bounded format used by the service cache.
fn png(side: u32, color: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(side, side, image::Rgba([color, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Authenticated bytes require real decoding and content identity, never a service URL or account-based path.
#[test]
fn service_image_cache_validates_payload_and_bounds_decoded_allocation() {
    let directory = tempfile::tempdir().unwrap();
    let images = ImageDirectory::new(directory.path().to_owned(), super::super::LAUNCHER_ART);
    let first = png(1, 1);
    let original = images.store_service_image(&first).unwrap();
    assert_eq!(fs::read(&original).unwrap(), first);
    assert_eq!(images.store_service_image(&first).unwrap(), original);
    assert_ne!(images.store_service_image(&png(1, 2)).unwrap(), original);
    assert!(
        images
            .store_service_image(b"\x89PNG\r\n\x1a\ntruncated")
            .is_none()
    );
    let bounded = ImageDirectory::new(
        directory.path().join("bounded"),
        Surface {
            max_bytes: 64 * 1024,
            ..super::super::LAUNCHER_ART
        },
    );
    let compressed = png(1024, 1);
    assert!(compressed.len() < bounded.0.surface.max_bytes);
    assert!(bounded.store_service_image(&compressed).is_none());
}
