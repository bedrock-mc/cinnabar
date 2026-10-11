use super::*;
use std::{io::Cursor, sync::Arc};

/// Makes an encoded image with valid dimensions and a caller-selected encoded length.
fn png(bytes: usize) -> Vec<u8> {
    let mut encoded = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 3, image::Rgba([20, 40, 60, 255]))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let mut encoded = encoded.into_inner();
    encoded.resize(bytes.max(encoded.len()), 0);
    encoded
}

#[test]
fn local_source_replacement_publishes_only_the_new_original_pixels() {
    let root = std::env::temp_dir().join(format!("cinnabar-local-reload-{}", std::process::id()));
    std::fs::create_dir_all(root.join("textures")).unwrap();
    let key = "textures/reloaded";
    std::fs::write(
        root.join("textures/reloaded.png"),
        png(MAX_PACK_TEXTURE_BYTES as usize + 1),
    )
    .unwrap();
    let departing = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(Some(root.clone()), None)
        .with_native_sheets(true);
    assert_eq!(departing.fallback_size(key), None);
    drop(departing);
    let color = [170, 60, 30, 255];
    let mut encoded = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2048, 1, image::Rgba(color))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let mut encoded = encoded.into_inner();
    encoded.resize(MAX_PACK_TEXTURE_BYTES as usize + 1, 0);
    std::fs::write(root.join("textures/reloaded.png"), encoded).unwrap();
    let mut replacement = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(Some(root.clone()), None)
        .with_native_sheets(true);
    test_time::eventually(
        "replacement local source publishes fresh native pixels",
        || {
            replacement.require([key]);
            replacement
                .native_pages()
                .iter()
                .any(|page| page.dimensions() == [2048, 1] && page.pixels()[..4] == color)
        },
    );
    assert_eq!(replacement.fallback_size(key), Some([2048.0, 1.0]));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unreadable_large_local_extension_reveals_small_fallback_and_settles_missing_keys() {
    let root = std::env::temp_dir().join(format!("cinnabar-local-failure-{}", std::process::id()));
    std::fs::create_dir_all(root.join("textures")).unwrap();
    let key = "textures/fallback";
    std::fs::write(
        root.join("textures/fallback.png"),
        vec![0; MAX_PACK_TEXTURE_BYTES as usize + 1],
    )
    .unwrap();
    std::fs::write(root.join("textures/fallback.tga"), png(0)).unwrap();
    let mut atlas = ServerAtlas::new(&[], None, 1).with_fallbacks(Some(root.clone()), None);
    assert_eq!(atlas.fallback_size(key), None);
    assert!(
        !atlas.extra.borrow().contains_key(key),
        "queued images are not terminal misses"
    );
    test_time::eventually(
        "failed large extension publishes the next valid local source",
        || {
            atlas.require([key]);
            atlas.fallback_size(key) == Some([2.0, 3.0]) && atlas.placement(key).is_some()
        },
    );
    let missing = "textures/missing";
    std::fs::write(
        root.join("textures/missing.png"),
        vec![0; MAX_PACK_TEXTURE_BYTES as usize + 1],
    )
    .unwrap();
    assert_eq!(atlas.fallback_size(missing), None);
    test_time::eventually("invalid local source becomes a terminal miss", || {
        atlas.poll_native_images();
        atlas.extra.borrow().contains_key(missing)
    });
    assert!(atlas.extra.borrow()[missing].is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_and_archive_sources_share_the_large_encoded_budget() {
    let mut encoded = png(5 * 1024 * 1024);
    let key = "textures/large";
    let view = super::super::external_paths_tests::view(&[(format!("{key}.png"), encoded.clone())]);
    let mut atlas = ServerAtlas::new(&[], Some(view), 1);
    let large = source(Arc::from(encoded.clone())).unwrap();
    let pack = atlas.pack.as_ref().unwrap();
    pack.loaded
        .borrow_mut()
        .extend((0..10).map(|i| (format!("pack_{i}"), Some(large.clone()))));
    atlas
        .extra
        .borrow_mut()
        .extend((0..8).map(|i| (format!("local_{i}"), Some(large.clone()))));
    assert_eq!(atlas.source_read_limit(), 6 * 1024 * 1024);
    assert_eq!(atlas.image_size(key), None);
    test_time::eventually(
        "remaining aggregate budget admits one large archive",
        || {
            atlas.poll_native_images();
            atlas.image_size(key).is_some()
        },
    );
    assert_eq!(atlas.source_read_limit(), MAX_PACK_TEXTURE_BYTES);
    encoded.resize(MAX_PACK_TEXTURE_BYTES as usize + 1, 0);
    let root = std::env::temp_dir().join(format!("cinnabar-local-budget-{}", std::process::id()));
    std::fs::create_dir_all(root.join("textures")).unwrap();
    std::fs::write(root.join("textures/budget.png"), &encoded).unwrap();
    std::fs::write(root.join("textures/budget.tga"), png(0)).unwrap();
    atlas = atlas.with_fallbacks(Some(root.clone()), None);
    assert_eq!(atlas.fallback_size("textures/budget"), None);
    test_time::eventually(
        "budget-rejected large source still reveals a small local extension",
        || {
            atlas.poll_native_images();
            atlas.fallback_size("textures/budget") == Some([2.0, 3.0])
        },
    );
    assert!(
        atlas.extra.borrow()["textures/budget"]
            .as_ref()
            .unwrap()
            .bytes
            .len() as u64
            <= MAX_PACK_TEXTURE_BYTES
    );
    std::fs::remove_dir_all(root).unwrap();
}
