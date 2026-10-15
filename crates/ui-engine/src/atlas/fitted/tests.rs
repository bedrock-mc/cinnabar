use super::super::jobs::ImageJob;
use super::*;

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
