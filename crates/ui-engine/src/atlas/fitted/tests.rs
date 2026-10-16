use super::super::jobs::ImageJob;
use super::*;
use crate::{atlas::ServerAtlas, sources::ImageWork};

/// An original two-by-two source whose decoded pixels are known.
fn source() -> Source {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([17, 29, 41, 255]))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    super::super::source(encoded.into_inner().into()).unwrap()
}

#[test]
fn reservation_covers_queued_work_and_unconsumed_pixels() {
    let mut jobs = FittedDecodes::default();
    let (sender, receiver) = crossbeam_channel::bounded(1);
    let source = source();
    jobs.request_to("first", &source, Some(&sender));
    receiver.try_recv().unwrap().run();
    jobs.request_to("second", &source, Some(&sender));
    assert!(receiver.is_empty());
    assert_eq!(jobs.pending(), Some("first"));
    let (key, pixels) = jobs.collect().unwrap();
    assert_eq!(key, "first");
    assert_eq!(pixels.unwrap(), [17, 29, 41, 255].repeat(4));
    assert!(jobs.pending().is_none());
    jobs.request_to("second", &source, Some(&sender));
    assert_eq!(receiver.try_recv().unwrap().key, "second");
}

#[test]
fn a_full_queue_retains_only_one_input_and_cancellation_skips_decode() {
    let (sender, receiver) = crossbeam_channel::bounded(1);
    let source = source();
    let mut occupying = FittedDecodes::default();
    occupying.request_to("occupied", &source, Some(&sender));
    let mut jobs = FittedDecodes::default();
    jobs.request_to("retry", &source, Some(&sender));
    assert!(jobs.retry.is_some());
    let references = Arc::strong_count(&source.bytes);
    for _ in 0..100 {
        jobs.request_to("deferred", &source, Some(&sender));
    }
    assert_eq!(Arc::strong_count(&source.bytes), references);
    assert_eq!(jobs.pending(), Some("retry"));
    let cancelled_retry = jobs.retry.take().unwrap();
    let completed = jobs.receiver.clone();
    drop(jobs);
    cancelled_retry.run_with(|_, _| panic!("cancelled job decoded"));
    assert!(completed.is_empty());
    let queued = receiver.try_recv().unwrap();
    let completed = occupying.receiver.clone();
    drop(occupying);
    queued.run_with(|_, _| panic!("retired atlas decoded queued input"));
    assert!(completed.is_empty());
}

#[test]
fn retirement_during_decode_discards_the_completion() {
    let mut jobs = FittedDecodes::default();
    let (sender, receiver) = crossbeam_channel::bounded(1);
    jobs.request_to("first", &source(), Some(&sender));
    let completed = jobs.receiver.clone();
    receiver.try_recv().unwrap().run_with(|_, _| {
        drop(jobs);
        Some(vec![255; 16])
    });
    assert!(completed.is_empty());
}

#[test]
fn unavailable_workers_report_failure_without_inline_decode() {
    let mut jobs = FittedDecodes::default();
    jobs.request_to("missing", &source(), None);
    assert_eq!(jobs.pending(), Some("missing"));
    assert_eq!(jobs.collect(), Some(("missing".into(), None)));
    assert!(jobs.pending().is_none());
}

#[test]
fn page_eviction_and_decode_completion_keep_source_snapshots() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(512, 512, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let source = super::super::source(encoded.into_inner().into()).unwrap();
    let mut atlas = ServerAtlas::new(&[], None, 1).with_image_work(ImageWork::inline());
    for key in ["a", "b"] {
        atlas.sources.insert(key.into(), source.clone());
    }
    atlas.oversized.insert("a", &source.bytes);
    let waiting = atlas.oversized();
    let (sender, receiver) = crossbeam_channel::bounded(1);
    atlas.decodes.jobs.request_to("a", &source, Some(&sender));
    receiver.try_recv().unwrap().run();
    assert_eq!(atlas.decodes.jobs.pending(), Some("a"));
    assert!(atlas.decodes.pixels.is_empty());
    atlas.require(["a"]);
    assert!(atlas.decodes.jobs.pending().is_none());
    assert_eq!(&atlas.decodes.pixels["a"][..4], [1, 2, 3, 255]);
    assert!(atlas.placement("a").is_some());
    assert!(atlas.oversized().shares_storage(&waiting));
    atlas.require(["b"]);
    assert!(atlas.placement("a").is_none());
    let evicted = atlas.oversized();
    assert_eq!(
        evicted.len(),
        2,
        "full-resolution sources outlive fitted-page residency"
    );
    atlas.require(["a"]);
    assert!(atlas.placement("b").is_none());
    assert!(atlas.oversized().shares_storage(&evicted));
}
