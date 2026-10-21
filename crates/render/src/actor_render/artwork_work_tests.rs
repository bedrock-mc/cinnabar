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
fn unused_artwork_is_bounded_but_demanded_pages_are_immediate() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let mut pages = ActorArtworkPages::default();
    pages.entity_identity = [1; 32];
    pages.pages = (1..=12)
        .map(|marker| render_model::ActorTexturePage {
            width: 8,
            height: 8,
            layers: 1,
            rgba8: vec![marker; 8 * 8 * 4].into(),
            color_mask: false,
            multitexture: false,
        })
        .collect();
    let mut gpu = GpuArtwork::default();
    take_work();
    assert!(gpu.prepare_demanded(&pages, &[], &device, &queue));
    let background = take_work();
    assert_eq!(
        background.uploads,
        PAGES_PER_FRAME + 2,
        "includes two fallback glints"
    );
    assert!(gpu.page(PAGES_PER_FRAME).is_none());
    let demanded = [5, 6, 7, 8, 9, 10, 11, 12];
    assert!(gpu.prepare_demanded(&pages, &demanded, &device, &queue));
    assert_eq!(take_work().uploads, demanded.len());
    assert!(
        demanded
            .iter()
            .all(|&id| gpu.page(usize::from(id) - 1).is_some())
    );
    assert!(gpu.prepare_demanded(&pages, &demanded, &device, &queue));
    assert_eq!(take_work(), Work::default());
}

#[test]
fn failed_background_artwork_does_not_hide_demand_and_retries_when_demanded() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let (pages, _) = ActorArtworkPages::default()
        .with_equipment_raster_page_updates(&[(0, raster(1)), (1, raster(2))]);
    let attempts = std::rc::Rc::new(Cell::new(0));
    let observed = attempts.clone();
    let fault = gpu_allocation::inject(move |label, _| {
        if label != "immutable actor page" {
            return false;
        }
        observed.set(observed.get() + 1);
        observed.get() > 1
    });
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare_demanded(&pages, &[1], &device, &queue));
    assert!(
        attempts.get() > 1,
        "unused pages are prepared in the background"
    );
    assert!(gpu.page(0).is_some());
    assert!(gpu.page(1).is_none());
    drop(fault);
    take_work();
    assert!(gpu.prepare_demanded(&pages, &[], &device, &queue));
    assert_eq!(
        take_work(),
        Work::default(),
        "failed prewarming does not spin"
    );
    assert!(gpu.prepare_demanded(&pages, &[2], &device, &queue));
    assert_eq!(
        gpu.page(1).unwrap()._texture.width(),
        u32::from(pages.pages[1].width)
    );
    assert_eq!(
        take_work().uploads,
        1,
        "new demand retries the complete source"
    );
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
    let textures: Vec<_> = gpu
        .pages
        .iter()
        .flatten()
        .map(|page| page._texture.id())
        .collect();
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
    for (page, texture) in gpu.pages.iter().flatten().zip(&textures) {
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
    let stable = gpu.page(0).unwrap()._texture.id();
    let replaced = gpu.page(1).unwrap()._texture.id();
    take_work();
    let newcomer = raster(43);
    let (next, _) = first.with_equipment_raster_page_updates(&[(1, newcomer.clone())]);
    assert!(gpu.prepare(&next, &device, &queue));
    assert_eq!(gpu.pages.len(), 2);
    assert_eq!(gpu.page(0).unwrap()._texture.id(), stable);
    assert_ne!(gpu.page(1).unwrap()._texture.id(), replaced);
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
    let texture = gpu.page(0).unwrap()._texture.id();
    let mut replacement = pages.pages[0].clone();
    replacement.rgba8 = replacement.rgba8.to_vec().into();
    pages.pages = Arc::from([replacement]);
    pages.entity_identity = [1; 32];
    take_work();
    assert!(gpu.prepare(&pages, &device, &queue));
    assert_eq!(gpu.page(0).unwrap()._texture.id(), texture);
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
        assert_eq!(gpu.page(0).unwrap()._texture.id(), texture);
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
    let previous = gpu.page(0).unwrap()._texture.id();
    let (mut replacement, _) = first.with_equipment_raster_page_updates(&[(0, raster(19))]);
    {
        let _failure = gpu_allocation::inject(|label, _| label == "immutable actor page");
        assert!(gpu.prepare(&replacement, &device, &queue));
    }
    assert_eq!(gpu.page(0).unwrap()._texture.id(), previous);
    replacement.entity_identity = [3; 32];
    take_work();
    assert!(gpu.prepare(&replacement, &device, &queue));
    assert_ne!(gpu.page(0).unwrap()._texture.id(), previous);
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
