use super::*;
use std::{cell::Cell, rc::Rc};

/// Builds original solid-colour layers without installed game assets.
fn page(colors: &[[u8; 4]]) -> render_model::ActorTexturePage {
    render_model::ActorTexturePage {
        width: 8,
        height: 8,
        layers: colors.len() as u32,
        rgba8: colors
            .iter()
            .flat_map(|color| color.repeat(8 * 8))
            .collect(),
        color_mask: false,
        multitexture: false,
    }
}

/// Gives each immutable publication a distinct identity without installed game assets.
fn publication(marker: u8, pages: Vec<render_model::ActorTexturePage>) -> ActorArtworkPages {
    let mut artwork = ActorArtworkPages::default();
    artwork.entity_identity = [marker; 32];
    artwork.pages = pages.into();
    artwork
}

#[test]
fn identical_artwork_pages_own_distinct_slot_allocations() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let source = page(&[[255, 0, 0, 255]; 3]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(
        &publication(1, vec![source.clone(), source]),
        &device,
        &queue
    ));
    assert_ne!(
        gpu.page(0).unwrap()._texture.id(),
        gpu.page(1).unwrap()._texture.id()
    );
}

#[test]
fn reordered_artwork_pages_replace_only_their_own_slot_allocations() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let original = publication(
        1,
        vec![page(&[[255, 0, 0, 255]; 3]), page(&[[0, 255, 0, 255]; 5])],
    );
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&original, &device, &queue));
    let ids = [
        gpu.page(0).unwrap()._texture.id(),
        gpu.page(1).unwrap()._texture.id(),
    ];
    let uploads = work_tests::upload_count();
    let next = publication(
        2,
        vec![page(&[[0, 255, 0, 255]; 5]), page(&[[255, 0, 0, 255]; 3])],
    );
    assert!(gpu.prepare(&next, &device, &queue));
    assert_eq!(work_tests::upload_count() - uploads, 2);
    for (index, source) in next.pages.iter().enumerate() {
        let prepared = gpu.page(index).unwrap();
        assert!(!ids.contains(&prepared._texture.id()));
        assert!(prepared.source.shares_pixels_with(source));
    }
}

#[test]
fn unused_artwork_replacements_prewarm_while_late_demand_is_immediate() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let pages = |marker| {
        publication(
            marker,
            (0..12).map(|i| page(&[[marker, i, 0, 255]])).collect(),
        )
    };
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages(1), &device, &queue));
    let next = pages(2);
    let uploads = work_tests::upload_count();
    assert!(gpu.prepare_demanded(&next, &[], &device, &queue));
    assert_eq!(work_tests::upload_count() - uploads, PAGES_PER_FRAME);
    assert!(
        gpu.page(PAGES_PER_FRAME - 1)
            .unwrap()
            .source
            .shares_pixels_with(&next.pages[PAGES_PER_FRAME - 1])
    );
    assert!(
        !gpu.page(PAGES_PER_FRAME)
            .unwrap()
            .source
            .shares_pixels_with(&next.pages[PAGES_PER_FRAME])
    );
    let uploads = work_tests::upload_count();
    assert!(gpu.prepare_demanded(&next, &[10, 11, 12], &device, &queue));
    assert_eq!(work_tests::upload_count() - uploads, PAGES_PER_FRAME + 3);
    for index in [0, 7, 9, 10, 11] {
        assert!(
            gpu.page(index)
                .unwrap()
                .source
                .shares_pixels_with(&next.pages[index])
        );
    }
    assert!(
        !gpu.page(8)
            .unwrap()
            .source
            .shares_pixels_with(&next.pages[8])
    );
    let uploads = work_tests::upload_count();
    assert!(gpu.prepare_demanded(&next, &[], &device, &queue));
    assert_eq!(work_tests::upload_count() - uploads, 1);
    assert!(
        gpu.page(8)
            .unwrap()
            .source
            .shares_pixels_with(&next.pages[8])
    );
}

#[test]
fn replacing_multiple_artwork_pages_keeps_only_one_temporary_texture() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions::enabled(),
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let pages = |marker| {
        publication(
            marker,
            (0..4).map(|i| page(&[[marker, i, 0, 255]])).collect(),
        )
    };
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&pages(1), &device, &queue));
    queue.submit([]);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let resident = instance
        .generate_report()
        .unwrap()
        .hub
        .textures
        .num_allocated;
    assert_eq!(resident, 6, "four artwork pages and two glints");
    let peak = Rc::new(Cell::new(resident));
    let observed = peak.clone();
    let attempts = Rc::new(Cell::new(0));
    let counted = attempts.clone();
    let fault = gpu_allocation::inject(move |label, _| {
        if label != "immutable actor page" {
            return false;
        }
        counted.set(counted.get() + 1);
        let live = instance
            .generate_report()
            .unwrap()
            .hub
            .textures
            .num_allocated;
        observed.set(observed.get().max(live));
        live > resident + 1
    });
    let next = pages(2);
    assert!(gpu.prepare(&next, &device, &queue));
    drop(fault);
    assert_eq!(
        peak.get(),
        resident + 1,
        "each successful replacement retires its old allocation"
    );
    assert_eq!(attempts.get(), next.pages.len());
    for (index, source) in next.pages.iter().enumerate() {
        assert!(gpu.page(index).unwrap().source.shares_pixels_with(source));
    }
}

#[test]
fn failed_artwork_replacement_recovers_without_changing_publication() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let original = publication(1, vec![page(&[[255, 0, 0, 255]])]);
    let next = publication(2, vec![page(&[[0, 255, 0, 255]])]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&original, &device, &queue));
    let previous = gpu.page(0).unwrap()._texture.id();
    {
        let _fault = gpu_allocation::inject(|label, _| label == "immutable actor page");
        assert!(gpu.prepare(&next, &device, &queue));
    }
    assert_eq!(gpu.page(0).unwrap()._texture.id(), previous);
    assert!(gpu.prepare(&next, &device, &queue));
    assert!(
        gpu.page(0)
            .unwrap()
            .source
            .shares_pixels_with(&next.pages[0])
    );
    assert_ne!(gpu.page(0).unwrap()._texture.id(), previous);
}
