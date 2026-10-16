use super::super::tests::MemoryPack;
use super::*;
use resource_pack::test_support::pack_view;
use std::{io::Cursor, sync::Arc};

/// Shipped images resolve with no carrier or local pack, and a server pack's copy shadows them.
#[test]
fn builtin_images_load_without_carriers_and_yield_to_server_images() {
    let key = "textures/ui/hotbar_offhand_left";
    let mut encoded = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(29, 24, image::Rgba([12, 12, 12, 245]))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let frame: &'static [u8] = Vec::leak(encoded.into_inner());
    let images: &'static [(&str, &[u8])] = Vec::leak(vec![(key, frame)]);
    let mut atlas = ServerAtlas::new(&[], None, 1).with_builtin_images(images);
    assert_eq!(atlas.fallback_size(key), Some([29.0, 24.0]));
    test_time::eventually("built-in image becomes resident", || {
        atlas.require([key]);
        atlas.placement(key).is_some()
    });
    assert_eq!(atlas.placement(key).unwrap().rect[2..], [29, 24]);

    let files = vec![(format!("{key}.png"), png(0))];
    let mut replacement = ServerAtlas::new(&files, None, 1).with_builtin_images(images);
    test_time::eventually("server replacement becomes resident", || {
        replacement.require([key]);
        replacement.placement(key).is_some()
    });
    assert_eq!(replacement.placement(key).unwrap().rect[2..], [2, 3]);
}

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
    let root = Arc::new(MemoryPack::default());
    let key = "textures/reloaded";
    root.write(
        "textures/reloaded.png",
        png(MAX_PACK_TEXTURE_BYTES as usize + 1),
    );
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
    root.write("textures/reloaded.png", encoded);
    let mut replacement = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(Some(root), None)
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
}

#[test]
fn unreadable_large_local_extension_reveals_small_fallback_and_settles_missing_keys() {
    let root = Arc::new(MemoryPack::default());
    let key = "textures/fallback";
    root.write(
        "textures/fallback.png",
        vec![0; MAX_PACK_TEXTURE_BYTES as usize + 1],
    );
    root.write("textures/fallback.tga", png(0));
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
    root.write(
        "textures/missing.png",
        vec![0; MAX_PACK_TEXTURE_BYTES as usize + 1],
    );
    assert_eq!(atlas.fallback_size(missing), None);
    test_time::eventually("invalid local source becomes a terminal miss", || {
        atlas.poll_native_images();
        atlas.extra.borrow().contains_key(missing)
    });
    assert!(atlas.extra.borrow()[missing].is_none());
}

#[test]
fn local_and_archive_sources_share_the_large_encoded_budget() {
    let mut encoded = png(5 * 1024 * 1024);
    let key = "textures/large";
    let pack = pack_view(&[(format!("{key}.png"), encoded.clone())]);
    let root = Arc::new(MemoryPack::default());
    let mut atlas = ServerAtlas::new(&[], Some(pack), 1).with_fallbacks(Some(root.clone()), None);
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
    root.write("textures/budget.png", encoded);
    root.write("textures/budget.tga", png(0));
    atlas = atlas.with_fallbacks(Some(root), None);
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
}
