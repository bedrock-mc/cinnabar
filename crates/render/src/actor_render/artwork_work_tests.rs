use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Work {
    hashes: usize,
    hashed_bytes: usize,
    uploads: usize,
    uploaded_bytes: usize,
}

thread_local! {
    static WORK: Cell<Work> = Cell::new(Work::default());
}

/// Observes writes when a fault-injection test checks the allocation boundary.
pub(super) fn upload_count() -> usize {
    WORK.with(|work| work.get().uploads)
}

/// Counts the bytes fingerprinted for a newly encountered immutable source.
pub(super) fn record_hash(bytes: usize) {
    WORK.with(|work| {
        let mut value = work.get();
        value.hashes += 1;
        value.hashed_bytes += bytes;
        work.set(value);
    });
}

/// Counts the actual fitted bytes passed to the texture upload call.
pub(super) fn record_upload(bytes: usize) {
    WORK.with(|work| {
        let mut value = work.get();
        value.uploads += 1;
        value.uploaded_bytes += bytes;
        work.set(value);
    });
}

/// Starts a fresh work interval and returns the preceding interval's counters.
fn take_work() -> Work {
    WORK.with(|work| work.replace(Work::default()))
}

/// Distinct RGBA sources prevent content reuse from hiding an unnecessary upload.
fn raster(fill: u8) -> render_model::EquipmentRaster {
    render_model::EquipmentRaster {
        width: 8,
        height: 8,
        rgba8: vec![fill; 8 * 8 * 4].into(),
    }
}

#[test]
fn persona_arrival_hashes_and_uploads_only_the_new_layer() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let images = [raster(7), raster(19), raster(43)];
    let base = ActorArtworkPages::default()
        .with_actor_glint(raster(61))
        .with_item_glint(raster(79));
    let (first, _) = base
        .clone()
        .with_equipment_raster_page_updates(&[(0, images[0].clone()), (1, images[1].clone())]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&first, &device, &queue));
    let textures: Vec<_> = gpu.pages.iter().map(|page| page._texture.id()).collect();
    take_work();
    let (next, _) = first
        .clone()
        .with_equipment_raster_page_updates(&[(2, images[2].clone())]);
    assert!(gpu.prepare(&next, &device, &queue));
    let incremental = take_work();
    assert_eq!(
        incremental,
        Work {
            hashes: 1,
            hashed_bytes: images[2].rgba8.len(),
            uploads: 1,
            uploaded_bytes: images[2].rgba8.len()
        }
    );
    for (page, texture) in gpu.pages.iter().zip(&textures) {
        assert_eq!(page._texture.id(), *texture);
    }
    assert!(gpu.prepare(&next, &device, &queue));
    assert_eq!(
        take_work(),
        Work::default(),
        "an unchanged publication does no work"
    );
    let mut route_change = next;
    route_change.entity_identity = [3; 32];
    assert!(gpu.prepare(&route_change, &device, &queue));
    assert_eq!(
        take_work(),
        Work::default(),
        "route changes do not hash or upload retained pixels"
    );

    let (packed, _) = base.clone().with_equipment_rasters(&images[..2]);
    let mut legacy = GpuArtwork::default();
    assert!(legacy.prepare(&packed, &device, &queue));
    take_work();
    let (repacked, _) = base.with_equipment_rasters(&images);
    assert!(legacy.prepare(&repacked, &device, &queue));
    assert_eq!(
        take_work(),
        Work {
            hashes: 1,
            hashed_bytes: incremental.hashed_bytes * images.len(),
            uploads: 1,
            uploaded_bytes: incremental.uploaded_bytes * images.len()
        }
    );
}

#[test]
fn persona_slot_replacement_uploads_one_layer_and_keeps_other_textures() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let (first, _) = ActorArtworkPages::default()
        .with_actor_glint(raster(61))
        .with_item_glint(raster(79))
        .with_equipment_raster_page_updates(&[(0, raster(7)), (1, raster(19))]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&first, &device, &queue));
    let stable = gpu.pages[0]._texture.id();
    let replaced = gpu.pages[1]._texture.id();
    take_work();
    let newcomer = raster(43);
    let (next, _) = first.with_equipment_raster_page_updates(&[(1, newcomer.clone())]);
    assert!(gpu.prepare(&next, &device, &queue));
    assert_eq!(gpu.pages.len(), 2);
    assert_eq!(gpu.pages[0]._texture.id(), stable);
    assert_ne!(gpu.pages[1]._texture.id(), replaced);
    assert_eq!(
        take_work(),
        Work {
            hashes: 1,
            hashed_bytes: newcomer.rgba8.len(),
            uploads: 1,
            uploaded_bytes: newcomer.rgba8.len()
        }
    );
}

#[test]
fn equal_artwork_source_publications_do_not_rehash_or_upload() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let (mut pages, _) = ActorArtworkPages::default()
        .with_actor_glint(raster(61))
        .with_item_glint(raster(79))
        .with_equipment_raster_page_updates(&[(0, raster(7))]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages, &device, &queue));
    let texture = gpu.pages[0]._texture.id();
    let mut replacement = pages.pages[0].clone();
    replacement.rgba8 = replacement.rgba8.to_vec().into();
    pages.pages = Arc::from([replacement]);
    pages.entity_identity = [1; 32];
    take_work();
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_eq!(gpu.pages[0]._texture.id(), texture);
    assert_eq!(
        take_work(),
        Work {
            hashes: 1,
            hashed_bytes: pages.pages[0].rgba8.len(),
            ..Default::default()
        }
    );
    for marker in [2, 3] {
        pages.entity_identity = [marker; 32];
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(
            take_work(),
            Work::default(),
            "retained current allocations need no raster work"
        );
        assert_eq!(gpu.pages[0]._texture.id(), texture);
    }
}

#[test]
fn failed_artwork_replacement_keeps_its_source_until_upload_recovers() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let (first, _) = ActorArtworkPages::default()
        .with_actor_glint(raster(61))
        .with_item_glint(raster(79))
        .with_equipment_raster_page_updates(&[(0, raster(7))]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&first, &device, &queue));
    let previous = gpu.pages[0]._texture.id();
    let (mut replacement, _) = first.with_equipment_raster_page_updates(&[(0, raster(19))]);
    {
        let _failure = allocation::inject(|label, _| label == "immutable actor page");
        assert!(gpu.prepare(&replacement, &device, &queue));
    }
    assert_eq!(gpu.pages[0]._texture.id(), previous);
    replacement.entity_identity = [3; 32];
    take_work();
    assert!(gpu.prepare(&replacement, &device, &queue));
    assert_ne!(gpu.pages[0]._texture.id(), previous);
    assert_eq!(
        take_work(),
        Work {
            hashes: 1,
            hashed_bytes: replacement.pages[0].rgba8.len(),
            uploads: 1,
            uploaded_bytes: replacement.pages[0].rgba8.len(),
        }
    );
}
