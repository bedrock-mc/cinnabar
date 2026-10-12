use super::*;

// Prepared HUD textures read on the worker; an unprepared pack reads them on first draw.
#[test]
fn prepared_hud_textures_read_off_the_frame() {
    const HOTBAR: &str = "textures/ui/hotbar_0";
    let files = vec![(format!("{HOTBAR}.png"), png(182, 22))];
    for prepared in [true, false] {
        let Some(mut presentation) = crate::test_support::engine_presentation() else {
            eprintln!(
                "skipping prepared_hud_textures_read_off_the_frame: missing local UI carrier (make assets)"
            );
            return;
        };
        let base = presentation.pack_catalog_base().unwrap();
        let pack = ServerUiPack {
            view: Some(super::external_paths_tests::view(&files)),
            ..ServerUiPack::default()
        };
        let pack = if prepared {
            (*pack.prepare_catalog(&base)).clone()
        } else {
            ServerUiPack {
                catalog: Some(Arc::new(
                    super::super::screen_registry::registry()
                        .catalog()
                        .layer(&base, &[]),
                )),
                ..pack
            }
        };
        PACK_READS.with(|reads| reads.borrow_mut().clear());
        presentation.set_server_ui_pack(&pack);
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        presentation
            .build(
                &player,
                &crate::ui_runtime::UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let read = PACK_READS.with(|reads| reads.borrow().iter().any(|key| key == HOTBAR));
        assert_eq!(
            read, !prepared,
            "prepared={prepared}: the frame read the hotbar from the pack"
        );
    }
}

#[test]
fn cold_loading_backdrop_precedes_an_expensive_title_decode() {
    let files = vec![
        ("textures/ui/title.png".to_owned(), png(1992, 669)),
        ("textures/blocks/dirt.png".to_owned(), png(16, 16)),
    ];
    let mut atlas = ServerAtlas::new(&files, None, 2);
    atlas.require(["textures/ui/title", "textures/blocks/dirt"]);
    assert_eq!(
        atlas.placement("textures/blocks/dirt").unwrap().rect,
        [0, 0, 16, 16]
    );
}

/// Encodes a deterministic PNG fixture.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// Encodes a deterministic TGA fixture.
fn tga(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([4, 5, 6, 255]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Tga)
        .unwrap();
    bytes
}

// Only drawn textures pack; sidecars feed layout; TGA data in a png path reads.
#[test]
fn textures_pack_when_drawn_with_their_sidecars() {
    let files = vec![
        ("textures/ui/button.png".to_owned(), png(16, 8)),
        (
            "textures/ui/button.json".to_owned(),
            br#"{ "nineslice_size": 2, "base_size": [16, 8] }"#.to_vec(),
        ),
        ("textures/ui/other.png".to_owned(), tga(4, 4)),
        ("textures/ui/wide.png".to_owned(), png(512, 4)),
    ];
    let mut atlas = ServerAtlas::new(&files, None, 1);
    assert!(
        atlas
            .sidecar("textures/ui/button")
            .unwrap()
            .nineslice
            .is_some()
    );
    assert_eq!(atlas.image_size("textures/ui/wide"), Some([512.0, 4.0]));
    atlas.require(["textures/ui/button"]);
    assert_eq!(
        atlas.placement("textures/ui/button").unwrap().rect,
        [0, 0, 16, 8]
    );
    assert!(atlas.placement("textures/ui/other").is_none());
    assert_eq!(atlas.image_size("textures/ui/other"), Some([4.0, 4.0]));
    assert_eq!(atlas.images().len(), 1);
    assert!(atlas.take_dirty());
    atlas.require(["textures/ui/button"]);
    assert!(!atlas.take_dirty(), "a resident texture changes no page");
    atlas.require(["textures/ui/wide"]);
    assert_eq!(
        atlas.placement("textures/ui/wide").unwrap().rect[2..],
        [256, 2],
        "an oversized texture packs downscaled"
    );
    let oversized = atlas.oversized();
    assert_eq!(oversized.len(), 1, "and is offered to the art pages");
    assert_eq!(oversized[0].0, "textures/ui/wide");
}

// A sidecar with no pack image still overrides; an image alone leaves no sidecar.
#[test]
fn images_and_sidecars_inherit_independently() {
    let files = vec![
        (
            "textures/ui/panel.json".to_owned(),
            br#"{ "nineslice_size": 3, "base_size": [9, 9] }"#.to_vec(),
        ),
        ("textures/ui/frame.png".to_owned(), png(8, 8)),
    ];
    let atlas = ServerAtlas::new(&files, None, 1);
    assert!(!atlas.has_image("textures/ui/panel"));
    assert_eq!(
        atlas.sidecar("textures/ui/panel").unwrap().base_size,
        [9.0, 9.0]
    );
    assert!(atlas.has_image("textures/ui/frame"));
    assert!(atlas.sidecar("textures/ui/frame").is_none());
}

// Worker decodes become resident when collected by later frames and retain
// their pixels for reuse, regardless of how fast the local CPU decodes.
#[test]
fn worker_decodes_become_resident_and_keep_their_pixels() {
    // Distinct pixels exercise independent decoded sources.
    let noise = |seed: u32| {
        let mut bytes = Vec::new();
        image::RgbaImage::from_fn(256, 256, |x, y| {
            let v = (x * 7919 + y * 104_729 + seed * 31).wrapping_mul(2_654_435_761);
            image::Rgba(v.to_le_bytes())
        })
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
        bytes
    };
    let files: Vec<_> = (0..40)
        .map(|index| (format!("textures/ui/b{index}.png"), noise(index)))
        .collect();
    let keys: Vec<_> = (0..40)
        .map(|index| format!("textures/ui/b{index}"))
        .collect();
    let mut atlas = ServerAtlas::new(&files, None, 64);
    let resident = |atlas: &ServerAtlas| {
        keys.iter()
            .filter(|key| atlas.placement(key).is_some())
            .count()
    };
    for key in &keys {
        let source = atlas.image(key).unwrap();
        assert!(atlas.decodes.get(key, &source, false).is_none());
    }
    atlas.require(keys.iter().map(String::as_str));
    let started = Instant::now();
    while resident(&atlas) < keys.len() {
        assert!(started.elapsed() < Duration::from_secs(10));
        test_time::idle();
        atlas.require(keys.iter().map(String::as_str));
    }
    assert_eq!(atlas.decodes.pixels.len(), keys.len());
}

// A full atlas evicts the page drawn least recently, never one drawn this frame.
#[test]
fn a_full_atlas_evicts_the_least_recently_drawn_page() {
    let files: Vec<_> = (0..3)
        .map(|index| (format!("textures/ui/t{index}.png"), png(200, 200)))
        .collect();
    let mut atlas = ServerAtlas::new(&files, None, 2);
    atlas.require(["textures/ui/t0"]);
    atlas.require(["textures/ui/t1"]);
    atlas.require(["textures/ui/t1", "textures/ui/t2"]);
    assert!(atlas.placement("textures/ui/t0").is_none());
    assert!(atlas.placement("textures/ui/t1").is_some());
    assert!(atlas.placement("textures/ui/t2").is_some());
    atlas.require(["textures/ui/t0", "textures/ui/t1", "textures/ui/t2"]);
    let resident = (0..3)
        .filter(|index| atlas.placement(&format!("textures/ui/t{index}")).is_some())
        .count();
    assert_eq!(
        resident, 2,
        "two pages hold two of three; the rest is left out"
    );
}

// Vanilla's `textures/ui/White` must not open `white.png`, even on a
// case-insensitive file system.
#[test]
fn vanilla_images_match_their_exact_spelling() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-exact-case-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let ui = root.join("textures/ui");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("white.png"), png(2, 2)).unwrap();
    let atlas = ServerAtlas::new(&[], None, 1).with_fallbacks(Some(root.clone()), None);
    assert_eq!(atlas.fallback_size("textures/ui/white"), Some([2.0, 2.0]));
    assert_eq!(atlas.fallback_size("textures/ui/White"), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn vanilla_sidecars_inherit_without_an_image_and_keep_exact_case() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-sidecar-case-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let ui = root.join("textures/ui");
    std::fs::create_dir_all(&ui).unwrap();
    let path = ui.join("border.json");
    std::fs::write(&path, br#"{"base_size":[16,16],"nineslice_size":4}"#).unwrap();
    let atlas = ServerAtlas::new(&[], None, 1).with_fallbacks(Some(root.clone()), None);
    assert!(atlas.fallback_size("textures/ui/border").is_none());
    assert!(atlas.sidecar("textures/ui/border").is_none());
    let meta = atlas.fallback_sidecar("textures/ui/border").unwrap();
    assert_eq!(meta.base_size, [16.0, 16.0]);
    assert_eq!(atlas.fallback_sidecar("textures/ui/Border"), None);
    assert_eq!(
        atlas.fallback_sidecar(&format!("{VANILLA_IN_PACKAGE}textures/ui/border")),
        Some(meta)
    );
    std::fs::remove_file(path).unwrap();
    assert_eq!(atlas.fallback_sidecar("textures/ui/border"), Some(meta));
    let _ = std::fs::remove_dir_all(root);
}
#[test]
fn vanilla_fallback_rejects_parent_paths_and_symlink_escapes() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-fallback-boundary-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let vanilla = root.join("vanilla");
    std::fs::create_dir_all(vanilla.join("textures")).unwrap();
    std::fs::write(root.join("private.png"), png(2, 2)).unwrap();
    std::fs::write(root.join("private.json"), br#"{"nineslice_size":4}"#).unwrap();
    let atlas = ServerAtlas::new(&[], None, 1).with_fallbacks(Some(vanilla.clone()), None);
    for key in [
        format!("{VANILLA_IN_PACKAGE}../private"),
        "textures/../../private".into(),
    ] {
        assert!(atlas.fallback_size(&key).is_none(), "{key}");
        assert!(atlas.fallback_sidecar(&key).is_none(), "{key}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("private.png"), vanilla.join("textures/link.png"))
            .unwrap();
        std::os::unix::fs::symlink(
            root.join("private.json"),
            vanilla.join("textures/link.json"),
        )
        .unwrap();
        assert!(atlas.fallback_size("textures/link").is_none());
        assert!(atlas.fallback_sidecar("textures/link").is_none());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn large_local_images_publish_metadata_without_reading_on_the_requesting_thread() {
    let root = std::env::temp_dir().join(format!("cinnabar-local-source-{}", std::process::id()));
    std::fs::create_dir_all(root.join("textures/ui")).unwrap();
    let key = "textures/ui/large_local";
    let mut encoded = png(2, 3);
    encoded.resize(MAX_PACK_TEXTURE_BYTES as usize + 1, 0);
    std::fs::write(root.join(format!("{key}.png")), &encoded).unwrap();
    let mut atlas = ServerAtlas::new(&[], None, 1).with_fallbacks(Some(root.clone()), None);
    LOCAL_READS.with(|reads| reads.borrow_mut().clear());
    assert_eq!(
        atlas.fallback_size(key),
        None,
        "a cold large source waits for worker metadata"
    );
    assert!(LOCAL_READS.with(|reads| reads.borrow().is_empty()));
    test_time::eventually("local metadata and pixels are published", || {
        atlas.require([key]);
        atlas.fallback_size(key) == Some([2.0, 3.0]) && atlas.placement(key).is_some()
    });
    assert!(atlas.metadata_revision() > 0);
    assert!(atlas.take_metadata_changed());
    let (_, allocations) = crate::allocation_count::count(|| {
        for _ in 0..3 {
            atlas.require([key]);
            assert_eq!(atlas.fallback_size(key), Some([2.0, 3.0]));
        }
    });
    assert_eq!(
        allocations, 0,
        "resident local sources reuse metadata and pixels"
    );
    assert!(LOCAL_READS.with(|reads| reads.borrow().is_empty()));
    assert!(!atlas.take_metadata_changed());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ui_images_larger_than_the_small_texture_limit_remain_readable() {
    let mut pixels = image::RgbaImage::new(1536, 1024);
    let mut noise = 0x8a35_7d21u32;
    for pixel in pixels.pixels_mut() {
        let mut bytes = [0; 4];
        for byte in &mut bytes {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            *byte = noise as u8;
        }
        *pixel = image::Rgba(bytes);
    }
    let mut encoded = Cursor::new(Vec::new());
    pixels
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let encoded = encoded.into_inner();
    assert!(encoded.len() as u64 > resource_pack::MAX_PACK_TEXTURE_BYTES);
    let key = "textures/ui/original_noise";
    let expected = pixels.get_pixel(0, 0).0;
    let view = super::external_paths_tests::view(&[(format!("{key}.png"), encoded)]);
    let atlas = ServerAtlas::new(&[], Some(view.clone()), 1).with_native_sheets(true);
    PACK_READS.with(|reads| reads.borrow_mut().clear());
    atlas.image_size(key);
    assert!(
        PACK_READS.with(|reads| reads.borrow().is_empty()),
        "large archive entries must not inflate on the requesting presentation thread",
    );
    let mut atlas = atlas;
    test_time::eventually(
        "large original image pixels arrive from their workers",
        || {
            atlas.require([key]);
            atlas.poll_native_images();
            atlas
                .native_pages()
                .iter()
                .any(|page| page.dimensions() == [1536, 1024] && page.pixels()[..4] == expected)
        },
    );
    assert_eq!(atlas.image_size(key), Some([1536.0, 1024.0]));
    assert!(PACK_READS.with(|reads| reads.borrow().is_empty()));

    let departing = ServerAtlas::new(&[], Some(view), 1).with_native_sheets(true);
    departing.image_size(key);
    drop(departing);
    let replacement = [190, 40, 90, 255];
    pixels.put_pixel(0, 0, image::Rgba(replacement));
    let mut encoded = Cursor::new(Vec::new());
    pixels
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let view = super::external_paths_tests::view(&[(format!("{key}.png"), encoded.into_inner())]);
    let mut reloaded = ServerAtlas::new(&[], Some(view), 1).with_native_sheets(true);
    test_time::eventually(
        "replacement image never publishes the departing pack's source",
        || {
            reloaded.require([key]);
            reloaded.poll_native_images();
            reloaded
                .native_pages()
                .iter()
                .any(|page| page.pixels()[..4] == replacement)
        },
    );
    assert!(PACK_READS.with(|reads| reads.borrow().is_empty()));
}

#[test]
fn small_winning_source_remains_readable_above_large_lower_layers_at_the_budget_boundary() {
    let mut large = png(1, 1);
    large.resize(MAX_PACK_TEXTURE_BYTES as usize + 1, 0);
    let source = source(Arc::from(large.clone())).unwrap();
    let key = "textures/ui/winning";
    let lower = [(format!("{key}.png"), large)];
    let higher = [(format!("{key}.png"), png(2, 3))];
    let pack = PackTextures::index(super::external_paths_tests::layered_view(&[
        &lower, &higher,
    ]));
    pack.loaded
        .borrow_mut()
        .extend((0..23).map(|i| (format!("resident_{i}"), Some(source.clone()))));
    test_time::eventually("small winning pixels survive a large shadowed copy", || {
        pack.collect();
        pack.image(key).is_some_and(|source| source.size == [2, 3])
    });
}
