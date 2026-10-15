use super::*;
use preparation::{CONTENT_BYTES_ON_THREAD, cape_layer};
use render_model::{STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE};

/// Models a fresh wire allocation with an original solid cape image.
fn source(byte: u8) -> CapeImage {
    CapeImage {
        width: 64,
        height: 32,
        rgba8: vec![byte; 64 * 32 * 4].into(),
    }
}

#[test]
fn cold_crowd_does_no_content_work_on_the_caller() {
    let sources: Vec<_> = (0..100).map(source).collect();
    let before = CONTENT_BYTES_ON_THREAD.with(|bytes| bytes.get());
    let mut queue = CapePreparationQueue::default();
    for _ in 0..sources
        .len()
        .div_ceil(super::super::skin::queue::MAX_SKIN_PREPARATIONS_PER_PASS)
    {
        queue.begin_frame();
        for source in &sources {
            let _ = queue.request(source);
        }
        queue.submit();
        queue.finish_fixture_batch();
    }
    for source in &sources {
        let pixels = queue.request(source).unwrap();
        assert_eq!(pixels.len(), STANDARD_SKIN_BYTES);
        assert!(pixels.iter().all(|&byte| byte == source.rgba8[0]));
    }
    let after = CONTENT_BYTES_ON_THREAD.with(|bytes| bytes.get());
    assert_eq!(
        after - before,
        0,
        "no source hashing, resampling or output hashing on caller"
    );
    assert_eq!(
        queue.cache.as_ref().unwrap().normalized_bytes,
        sources.len() * STANDARD_SKIN_BYTES
    );
    eprintln!(
        "100 cold capes: caller normalization before={} bytes, after=0 bytes; worker={} bytes",
        sources.len() * STANDARD_SKIN_BYTES,
        queue.cache.as_ref().unwrap().normalized_bytes
    );
}

#[test]
fn recreated_wire_allocations_share_one_normalization() {
    let sources: Vec<_> = (0..100).map(|_| source(9)).collect();
    let mut queue = CapePreparationQueue::default();
    for _ in 0..sources
        .len()
        .div_ceil(super::super::skin::queue::MAX_SKIN_PREPARATIONS_PER_PASS)
    {
        queue.begin_frame();
        for source in &sources {
            let _ = queue.request(source);
        }
        queue.submit();
        queue.finish_fixture_batch();
    }
    let first = queue.request(&sources[0]).unwrap();
    for source in &sources {
        assert!(Arc::ptr_eq(
            first.pixels(),
            queue.request(source).unwrap().pixels()
        ));
    }
    let cache = queue.cache.as_ref().unwrap();
    assert_eq!(cache.normalized_bytes, STANDARD_SKIN_BYTES);
    assert_eq!(cache.hits, sources.len() - 1);
    assert!(queue.queued.is_empty());
    eprintln!(
        "100 equal cape allocations: normalization before={} bytes, after={} bytes; hits={}/{}",
        sources.len() * STANDARD_SKIN_BYTES,
        cache.normalized_bytes,
        cache.hits,
        sources.len()
    );
}

#[test]
fn admission_and_in_flight_work_are_bounded() {
    let mut queue = CapePreparationQueue::default();
    let sources: Vec<_> = (0..32).map(source).collect();
    for source in &sources {
        assert!(queue.request(source).is_none());
    }
    let batch = super::super::skin::queue::MAX_SKIN_PREPARATIONS_PER_PASS;
    assert_eq!(queue.queued.len(), batch);
    queue.submit();
    for source in &sources {
        let _ = queue.request(source);
    }
    assert_eq!(queue.queued.len(), batch);
    assert_eq!(queue.entries.len(), batch * 2);
    queue.submit();
    assert_eq!(
        queue.queued.len(),
        batch,
        "one worker owns the content cache"
    );
    queue.finish_fixture_batch();
    queue.submit();
    queue.finish_fixture_batch();
    assert_eq!(queue.source_bytes, batch * 2 * sources[0].rgba8.len());
}

#[test]
fn worker_cache_turnover_and_hidden_sources_stay_bounded() {
    let mut cache = WorkerCache::default();
    for byte in 0..=render_model::MAX_RENDERED_PLAYERS {
        cache.prepare(&source(byte as u8));
    }
    assert_eq!(cache.entries_len(), render_model::MAX_RENDERED_PLAYERS);
    let mut queue = CapePreparationQueue::default();
    let source = source(9);
    queue.request(&source);
    queue.submit();
    queue.finish_fixture_batch();
    for _ in 0..3 {
        queue.begin_frame();
    }
    assert!(queue.entries.is_empty());
    assert_eq!(queue.source_bytes, 0);
    queue.request(&source);
    queue.submit();
    queue.finish_fixture_batch();
    assert_eq!(
        queue.cache.as_ref().unwrap().normalized_bytes,
        STANDARD_SKIN_BYTES
    );
}

#[test]
fn dimensions_and_pixels_distinguish_content() {
    let mut cache = WorkerCache::default();
    let first = source(9);
    let mut changed = first.clone();
    let mut pixels = changed.rgba8.to_vec();
    pixels[4] = 8;
    changed.rgba8 = pixels.into();
    let mut reshaped = first.clone();
    reshaped.width = 32;
    reshaped.height = 64;
    let a = cache.prepare(&first).unwrap();
    let b = cache.prepare(&changed).unwrap();
    let c = cache.prepare(&reshaped).unwrap();
    assert!(!Arc::ptr_eq(a.pixels(), b.pixels()));
    assert!(!Arc::ptr_eq(a.pixels(), c.pixels()));
    assert_eq!(cache.entries_len(), 3);
}

#[test]
fn malformed_capes_are_cached_without_a_retry_loop() {
    let mut queue = CapePreparationQueue::default();
    let source = CapeImage {
        width: u32::MAX,
        height: u32::MAX,
        rgba8: Arc::from([]),
    };
    queue.request(&source);
    queue.submit();
    queue.finish_fixture_batch();
    assert!(queue.request(&source).is_none());
    assert!(queue.queued.is_empty());
    assert_eq!(queue.cache.as_ref().unwrap().normalized_bytes, 0);
    assert!(cape_layer(0, 32, &[]).is_none());
    assert!(cape_layer(1 << 31, 1 << 31, &[]).is_none());
    assert!(cape_layer(64, 32, &[0; 4]).is_none());
}

#[test]
fn normalization_preserves_every_sampled_texel_and_alpha() {
    for &(width, height) in &render_api::CAPE_DIMENSIONS {
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|index| [index as u8, (index / width) as u8, 7, (index % 256) as u8])
            .collect();
        let layer = cape_layer(width, height, &pixels).unwrap();
        for y in 0..STANDARD_SKIN_SIDE {
            for x in 0..STANDARD_SKIN_SIDE {
                let source = ((y * height as usize / STANDARD_SKIN_SIDE) * width as usize
                    + x * width as usize / STANDARD_SKIN_SIDE)
                    * 4;
                let target = (y * STANDARD_SKIN_SIDE + x) * 4;
                assert_eq!(&layer[target..target + 4], &pixels[source..source + 4]);
            }
        }
    }
}

#[test]
fn retired_owner_cannot_publish_into_a_new_queue() {
    let source = source(9);
    let mut old = CapePreparationQueue::default();
    old.request(&source);
    old.submit();
    let (_, idle) = mpsc::channel();
    let receiver = std::mem::replace(&mut old.receiver, Mutex::new(idle))
        .into_inner()
        .unwrap();
    drop(old);
    let mut next = CapePreparationQueue::default();
    next.request(&source);
    let _ = receiver.recv();
    next.begin_frame();
    assert!(next.request(&source).is_none());
    assert_eq!(next.queued.len(), 1);
    next.submit();
    next.finish_fixture_batch();
    assert!(next.request(&source).is_some());
}
