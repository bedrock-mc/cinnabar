use super::*;
use resource_pack::test_support::{layered_pack_view, pack_view};
use std::sync::Mutex;

/// An installed vanilla pack held in memory.
#[derive(Default)]
pub(super) struct MemoryPack(Mutex<BTreeMap<String, Vec<u8>>>);

impl MemoryPack {
    /// Adds or replaces the file at `relative`.
    pub(super) fn write(&self, relative: &str, bytes: Vec<u8>) {
        self.0.lock().unwrap().insert(relative.to_owned(), bytes);
    }
}

impl LocalPack for MemoryPack {
    fn file_size(&self, relative: &str) -> Option<u64> {
        Some(self.0.lock().unwrap().get(relative)?.len() as u64)
    }

    fn read(&self, relative: &str, limit: u64) -> Option<Vec<u8>> {
        let files = self.0.lock().unwrap();
        let bytes = files.get(relative)?;
        (bytes.len() as u64 <= limit).then(|| bytes.clone())
    }
}

#[test]
fn arbitrary_pack_texture_keeps_source_dimensions_sidecar_and_pixels() {
    let mut art = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(512, 512, image::Rgba([24, 48, 72, 255]))
        .write_to(&mut art, image::ImageFormat::Png)
        .unwrap();
    let files = vec![
        ("assets/gui/inventory.png".into(), art.into_inner()),
        (
            "assets/gui/inventory.json".into(),
            br#"{"base_size":[176,166],"nineslice_size":2}"#.to_vec(),
        ),
    ];
    let pack = pack_view(&files);
    for mut atlas in [
        ServerAtlas::new(&files, None, 1),
        ServerAtlas::new(&[], Some(pack), 1),
    ] {
        let key = "assets/gui/inventory";
        assert!(atlas.has_image(key));
        assert_eq!(atlas.image_size(key), Some([512.0, 512.0]));
        assert_eq!(atlas.sidecar(key).unwrap().base_size, [176.0, 166.0]);
        atlas.require([key]);
        let placement = atlas.placement(key).unwrap();
        assert_eq!(placement.rect[2..], [256, 256]);
        let [x, y, _, _] = placement.rect;
        let offset = ((u32::from(y) * PAGE_SIDE + u32::from(x)) * 4) as usize;
        assert_eq!(
            &atlas.pages[usize::from(placement.page)].pixels[offset..offset + 4],
            &[24, 48, 72, 255]
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
        assert!(atlas.decodes.get(key, &source, DecodeOn::Worker).is_none());
    }
    test_time::eventually("worker decodes become resident", || {
        atlas.require(keys.iter().map(String::as_str));
        resident(&atlas) == keys.len()
    });
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

#[test]
fn large_local_images_publish_metadata_without_reading_on_the_requesting_thread() {
    let root = Arc::new(MemoryPack::default());
    let key = "textures/ui/large_local";
    let mut encoded = png(2, 3);
    encoded.resize(MAX_PACK_TEXTURE_BYTES as usize + 1, 0);
    root.write(&format!("{key}.png"), encoded);
    let mut atlas = ServerAtlas::new(&[], None, 1).with_fallbacks(Some(root), None);
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
    let (_, allocations) = test_alloc::count(|| {
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
}

/// Incompressible pixels whose PNG exceeds the small-texture read limit.
fn large_noise() -> (image::RgbaImage, Vec<u8>) {
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
    (pixels, encoded)
}

#[test]
fn ui_images_larger_than_the_small_texture_limit_remain_readable() {
    let (mut pixels, encoded) = large_noise();
    let key = "textures/ui/original_noise";
    let expected = pixels.get_pixel(0, 0).0;
    let pack = pack_view(&[(format!("{key}.png"), encoded)]);
    let atlas = ServerAtlas::new(&[], Some(pack.clone()), 1).with_native_sheets(true);
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

    let departing = ServerAtlas::new(&[], Some(pack), 1).with_native_sheets(true);
    departing.image_size(key);
    drop(departing);
    let replacement = [190, 40, 90, 255];
    pixels.put_pixel(0, 0, image::Rgba(replacement));
    let mut encoded = Cursor::new(Vec::new());
    pixels
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let pack = pack_view(&[(format!("{key}.png"), encoded.into_inner())]);
    let mut reloaded = ServerAtlas::new(&[], Some(pack), 1).with_native_sheets(true);
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
    let pack = PackTextures::index(layered_pack_view(&[&lower, &higher]));
    pack.loaded
        .borrow_mut()
        .extend((0..23).map(|i| (format!("resident_{i}"), Some(source.clone()))));
    test_time::eventually("small winning pixels survive a large shadowed copy", || {
        pack.collect();
        pack.image(key).is_some_and(|source| source.size == [2, 3])
    });
}

/// Host image work whose clock reads `clock`, advancing it by `step` after every read.
fn clocked(clock: test_time::ManualClock, step: Duration, workers: bool) -> ImageWork {
    ImageWork {
        clock: Some(Arc::new(move || {
            let now = clock.elapsed();
            clock.advance(step);
            now
        })),
        workers,
    }
}

// The inline decode budget is measured on the host's clock, never the engine's own.
#[test]
fn inline_decode_budget_follows_the_host_clock() {
    let files: Vec<_> = (0..4)
        .map(|index| (format!("textures/ui/c{index}.png"), png(16, 16)))
        .collect();
    let keys: Vec<_> = (0..4)
        .map(|index| format!("textures/ui/c{index}"))
        .collect();
    let resident = |atlas: &ServerAtlas| keys.iter().all(|key| atlas.placement(key).is_some());
    let placed = |atlas: &ServerAtlas| keys.iter().any(|key| atlas.placement(key).is_some());

    let frozen = clocked(test_time::ManualClock::new(), Duration::ZERO, true);
    let mut atlas = ServerAtlas::new(&files, None, 1).with_image_work(frozen);
    atlas.require(keys.iter().map(String::as_str));
    assert!(
        resident(&atlas),
        "a clock inside the budget decodes every miss inline"
    );

    let spent = clocked(test_time::ManualClock::new(), INLINE_DECODE_BUDGET, true);
    let mut atlas = ServerAtlas::new(&files, None, 1).with_image_work(spent);
    atlas.require(keys.iter().map(String::as_str));
    assert!(
        !placed(&atlas),
        "a spent budget hands every miss to workers"
    );
    assert!(atlas.decodes.jobs.pending().is_some());
    test_time::eventually("worker decodes become resident", || {
        atlas.require(keys.iter().map(String::as_str));
        resident(&atlas)
    });

    let spent = clocked(test_time::ManualClock::new(), INLINE_DECODE_BUDGET, false);
    let mut atlas = ServerAtlas::new(&files, None, 1).with_image_work(spent);
    atlas.require(keys.iter().map(String::as_str));
    assert!(
        !placed(&atlas),
        "without workers a spent budget defers misses"
    );
    assert!(
        atlas.decodes.jobs.pending().is_none(),
        "and starts no thread"
    );
    let mut atlas = atlas.with_image_work(ImageWork::inline());
    atlas.require(keys.iter().map(String::as_str));
    assert!(
        resident(&atlas),
        "a later frame within its budget decodes them"
    );
}

// A host without threads gets large archive reads, original sheets, large local reads and
// large decodes within the frame that asks for them.
#[test]
fn a_host_without_workers_completes_large_images_inline() {
    let (pixels, encoded) = large_noise();
    let expected = pixels.get_pixel(0, 0).0;
    let key = "textures/ui/original_noise";
    let pack = pack_view(&[(format!("{key}.png"), encoded)]);
    let mut atlas = ServerAtlas::new(&[], Some(pack), 1)
        .with_native_sheets(true)
        .with_image_work(ImageWork::inline());
    PACK_READS.with(|reads| reads.borrow_mut().clear());
    atlas.require([key]);
    assert_eq!(PACK_READS.with(|reads| reads.borrow().clone()), [key]);
    assert_eq!(atlas.image_size(key), Some([1536.0, 1024.0]));
    assert!(atlas.native_sheet_ready(key));
    assert!(
        atlas
            .native_pages()
            .iter()
            .any(|page| page.dimensions() == [1536, 1024] && page.pixels()[..4] == expected)
    );

    let root = Arc::new(MemoryPack::default());
    let local = "textures/ui/large_local";
    let mut encoded = png(2, 3);
    encoded.resize(MAX_PACK_TEXTURE_BYTES as usize + 1, 0);
    root.write(&format!("{local}.png"), encoded);
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(Some(root), None)
        .with_image_work(ImageWork::inline());
    LOCAL_READS.with(|reads| reads.borrow_mut().clear());
    atlas.require([local]);
    assert_eq!(
        LOCAL_READS.with(|reads| reads.borrow().clone()),
        [format!("{local}.png")]
    );
    assert_eq!(atlas.fallback_size(local), Some([2.0, 3.0]));
    assert!(
        atlas.placement(local).is_some(),
        "a large encoding decodes inline"
    );
}
