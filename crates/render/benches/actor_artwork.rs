use std::{hint::black_box, sync::Arc};

use bevy::render::renderer::{RenderDevice, RenderQueue};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use render::ActorArtworkBenchmark;
use render_model::{ActorArtworkPages, ActorTexturePage};

/// Submits pending uploads and reclaims staging memory outside publication timing.
fn drain_uploads(device: &RenderDevice, queue: &RenderQueue) {
    queue.submit([]);
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("benchmark uploads must complete");
}

/// Measures cold publication when a crowd samples a small part of a larger pack.
fn artwork_burst(c: &mut Criterion) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let mut pages = ActorArtworkPages::default();
    pages.entity_identity = [1; 32];
    pages.pages = (0..128)
        .map(|marker| ActorTexturePage {
            width: 128,
            height: 128,
            layers: 1,
            rgba8: Arc::from(vec![marker; 128 * 128 * 4]),
            color_mask: false,
            multitexture: false,
        })
        .collect();
    c.bench_function("actor_artwork/cold_pack_four_demanded_pages", |b| {
        b.iter_batched_ref(
            || {
                drain_uploads(&device, &queue);
                ActorArtworkBenchmark::default()
            },
            |gpu| black_box(gpu.prepare(&pages, &[1, 2, 3, 4], &device, &queue)),
            BatchSize::PerIteration,
        );
        drain_uploads(&device, &queue);
    });
}

criterion_group!(benches, artwork_burst);
criterion_main!(benches);
