use super::*;
use crate::{atlas::jobs::ImageJob, sources::ImageWork};
use std::{
    io::Cursor,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

type PublishedImages = BTreeMap<String, (BodyGeneration, Arc<[u8]>)>;

#[derive(Default)]
struct Images(Mutex<PublishedImages>);

impl Images {
    /// Publishes an original solid-color fixture with a distinct body generation.
    fn publish(&self, key: &str, generation: u64, size: [u32; 2], color: u8) {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(size[0], size[1], image::Rgba([color; 4]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        self.0.lock().unwrap().insert(
            key.to_owned(),
            (BodyGeneration(generation), bytes.into_inner().into()),
        );
    }
}

impl crate::sources::RemoteSource for Images {
    /// Shares the currently published fixture without running a download.
    fn state(&self, key: &str) -> RemoteImage {
        self.0
            .lock()
            .unwrap()
            .get(key)
            .map_or(RemoteImage::Loading, |(generation, bytes)| {
                RemoteImage::Ready(*generation, Arc::clone(bytes))
            })
    }
}

/// Reads the texel addressed by already emitted sprite geometry.
fn texel(atlas: &ServerAtlas, slot: crate::atlas::ServerTexture) -> [u8; 4] {
    let [x, y, _, _] = slot.rect.map(usize::from);
    let offset = (y * crate::atlas::PAGE_SIDE as usize + x) * 4;
    atlas.images()[usize::from(slot.page)].pixels()[offset..offset + 4]
        .try_into()
        .unwrap()
}

#[test]
fn a_form_generation_change_preserves_already_painted_hud_geometry() {
    let hud = "textures/hud";
    let url = "https://images.test/form.png";
    let remote = Arc::new(Images::default());
    remote.publish(url, 1, [2; 2], 80);
    let files = vec![(format!("{hud}.png"), super::super::tests::png(2, 2))];
    let mut atlas = ServerAtlas::new(&files, None, 1)
        .with_fallbacks(None, Some(remote.clone()))
        .with_image_work(ImageWork::inline());
    atlas.require([hud, url]);
    atlas.require([hud]);
    let hud_geometry = atlas.placement(hud).unwrap();
    let form_geometry = atlas.placement(url).unwrap();
    let hud_pixel = texel(&atlas, hud_geometry);
    let revision = atlas.metadata_revision();

    remote.publish(url, 2, [4; 2], 120);
    atlas.require_current([url]);
    assert_eq!(atlas.metadata_revision(), revision);
    assert_eq!(atlas.fallback_size(url), Some([2.0; 2]));
    assert_eq!(atlas.placement(url).unwrap().rect, form_geometry.rect);
    assert_eq!(texel(&atlas, form_geometry), [80; 4]);
    assert_eq!(texel(&atlas, hud_geometry), hud_pixel);
    assert!(atlas.placement(hud).is_some());

    atlas.require([hud, url]);
    assert_eq!(atlas.fallback_size(url), Some([4.0; 2]));
    assert_eq!(texel(&atlas, atlas.placement(url).unwrap()), [120; 4]);
    assert_eq!(texel(&atlas, hud_geometry), hud_pixel);
}

#[test]
fn a_generation_change_retires_only_its_url_and_keeps_another_worker_result() {
    let changed = "https://images.test/changed.png";
    let stable = "https://images.test/stable.png";
    let pending = "https://images.test/pending.png";
    let remote = Arc::new(Images::default());
    for key in [changed, stable] {
        remote.publish(key, 1, [super::super::PAGE_SIDE + 1, 2], 80);
    }
    remote.publish(pending, 1, [2; 2], 80);
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(None, Some(remote.clone()))
        .with_image_work(ImageWork::inline());
    atlas.require([changed, stable]);
    let stable_geometry = atlas.placement(stable).unwrap();
    let stable_pixels = Arc::clone(&atlas.decodes.pixels[stable]);
    let oversized = atlas.oversized();
    let stable_body = &oversized.iter().find(|entry| entry.0 == stable).unwrap().1;
    atlas.prepare_outline(changed, [2; 2]);
    atlas.prepare_outline(stable, [2; 2]);
    let stable_outline = atlas.original_outline(stable, [2; 2]).unwrap();
    let body = atlas.remote_source(pending).unwrap();
    let (sender, receiver) = crossbeam_channel::bounded(1);
    atlas.decodes.jobs.request_to(pending, &body, Some(&sender));
    receiver.try_recv().unwrap().run();

    remote.publish(changed, 2, [4; 2], 120);
    atlas.require([changed, stable, pending]);
    assert_eq!(atlas.placement(stable).unwrap().rect, stable_geometry.rect);
    assert!(Arc::ptr_eq(&stable_pixels, &atlas.decodes.pixels[stable]));
    assert!(Arc::ptr_eq(
        &stable_outline,
        &atlas.original_outline(stable, [2; 2]).unwrap()
    ));
    assert!(atlas.original_outline(changed, [2; 2]).is_none());
    assert!(atlas.decodes.pixels.contains_key(pending));
    let oversized = atlas.oversized();
    assert!(oversized.iter().all(|entry| entry.0 != changed));
    assert!(Arc::ptr_eq(
        stable_body,
        &oversized.iter().find(|entry| entry.0 == stable).unwrap().1
    ));
    assert_eq!(texel(&atlas, stable_geometry), [80; 4]);
    assert_eq!(texel(&atlas, atlas.placement(changed).unwrap()), [120; 4]);
}

#[test]
fn replacing_a_native_url_preserves_another_sheets_pixels() {
    let changed = "https://images.test/changed.png";
    let stable = "https://images.test/stable.png";
    let remote = Arc::new(Images::default());
    let size = [crate::pages::WHOLE_ART_PAGE + 1, 2];
    for key in [changed, stable] {
        remote.publish(key, 1, size, 80);
    }
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(None, Some(remote.clone()))
        .with_native_sheets(true)
        .with_image_work(ImageWork::inline());
    atlas.require([changed, stable]);
    let page = atlas.native_sprite(stable).unwrap().0;
    let identity = atlas.native_pages()[page].identity();
    remote.publish(changed, 2, size, 120);
    atlas.require([stable]);
    assert!(!atlas.native_sheet_ready(changed));
    let page = atlas.native_sprite(stable).unwrap().0;
    assert_eq!(atlas.native_pages()[page].identity(), identity);
    assert_eq!(&atlas.native_pages()[page].pixels()[..4], &[80; 4]);
}

#[test]
fn an_inactive_url_defers_a_new_generation_until_the_next_frame() {
    let url = "https://images.test/form.png";
    let remote = Arc::new(Images::default());
    remote.publish(url, 1, [2; 2], 80);
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(None, Some(remote.clone()))
        .with_image_work(ImageWork::inline());
    atlas.require([url]);
    atlas.require([]);
    atlas.require([]);
    remote.publish(url, 2, [4; 2], 120);
    let geometry = atlas.placement(url).unwrap();
    atlas.require_current([url]);
    assert_eq!(atlas.fallback_size(url), Some([2.0; 2]));
    assert_eq!(texel(&atlas, geometry), [80; 4]);
    atlas.require([url]);
    assert_eq!(atlas.fallback_size(url), Some([4.0; 2]));
    assert_eq!(texel(&atlas, atlas.placement(url).unwrap()), [120; 4]);
}

#[test]
fn more_than_the_metadata_budget_of_ready_urls_keep_worker_results() {
    let remote = Arc::new(Images::default());
    let urls: Vec<_> = (0..=super::super::MAX_EXTRA)
        .map(|index| format!("https://images.test/{index:04}.png"))
        .collect();
    for url in &urls {
        remote.publish(url, 1, [2; 2], 80);
    }
    let ticks = AtomicU64::new(0);
    let work = ImageWork {
        clock: Some(Arc::new(move || {
            Duration::from_millis(ticks.fetch_add(3, Ordering::Relaxed))
        })),
        workers: true,
    };
    let mut atlas = ServerAtlas::new(&[], None, 1)
        .with_fallbacks(None, Some(remote))
        .with_image_work(work);
    for url in &urls {
        assert_eq!(atlas.fallback_size(url), Some([2.0; 2]));
    }
    let url = &urls[0];
    let body = atlas.remote_source(url).unwrap();
    let (sender, receiver) = crossbeam_channel::bounded(1);
    atlas.decodes.jobs.request_to(url, &body, Some(&sender));
    receiver.try_recv().unwrap().run();
    BODY_INSPECTIONS.with(|calls| calls.set(0));

    atlas.require(urls.iter().map(String::as_str));
    assert!(atlas.placement(url).is_some());
    assert_eq!(texel(&atlas, atlas.placement(url).unwrap()), [80; 4]);
    assert_eq!(BODY_INSPECTIONS.with(std::cell::Cell::get), 0);
}
