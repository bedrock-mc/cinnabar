use super::*;
use std::{cell::Cell, rc::Rc};

/// Makes small layered pages so a test can stop uploads at row and layer boundaries.
fn publication(marker: u8, count: usize) -> ActorArtworkPages {
    let mut pages = ActorArtworkPages::default();
    pages.entity_identity = [marker; 32];
    pages.pages = (0..count)
        .map(|_| render_model::ActorTexturePage {
            width: 3,
            height: 5,
            layers: 3,
            rgba8: vec![marker; 3 * 5 * 3 * 4].into(),
            color_mask: false,
            multitexture: false,
        })
        .collect();
    pages
}

/// Provides GPU handles without a physical adapter or installed assets.
fn device() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    (device.into(), RenderQueue::new(queue))
}

#[test]
fn demanded_pages_share_one_budget_even_with_duplicate_requests() {
    let (device, queue) = device();
    let pages = publication(1, 2);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&ActorArtworkPages::default(), &device, &queue));
    let written = Rc::new(Cell::new(0));
    let observed = written.clone();
    let _observe = gpu_allocation::inject(move |label, bytes| {
        if label == "actor page upload" {
            observed.set(observed.get() + bytes as usize);
        }
        false
    });
    let row = usize::from(pages.pages[0].width) * 4;
    let budget = row * 7 + 1;
    let mut total = 0;
    for frame in 1..=5 {
        written.set(0);
        assert!(gpu.prepare_pages(
            &pages,
            [0, 2, 2, 1, 99].into_iter(),
            &device,
            &queue,
            budget
        ));
        assert!(written.get() <= budget);
        assert!(written.get() > 0);
        assert_eq!(written.get() % row, 0);
        total += written.get();
        assert_eq!(gpu.page(1).is_some(), frame >= 3);
        assert_eq!(gpu.page(0).is_some(), frame == 5);
    }
    assert_eq!(
        total,
        pages
            .pages
            .iter()
            .map(|page| page.rgba8.len())
            .sum::<usize>()
    );
}

#[test]
fn demand_preempts_pending_prewarming_without_starting_more_background_pages() {
    let (device, queue) = device();
    let pages = publication(1, 3);
    let budget = pages.pages[0].rgba8.len() / 3;
    let allocations = Rc::new(Cell::new(0));
    let observed = allocations.clone();
    let _observe = gpu_allocation::inject(move |label, _| {
        if label == "immutable actor page" {
            observed.set(observed.get() + 1);
        }
        false
    });
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare_pages(&pages, [].into_iter(), &device, &queue, budget));
    for frame in 1..=3 {
        assert!(gpu.prepare_pages(&pages, [2].into_iter(), &device, &queue, budget));
        assert_eq!(gpu.page(1).is_some(), frame == 3);
        assert!(gpu.page(0).is_none());
        assert!(gpu.page(2).is_none());
        assert_eq!(allocations.get(), 2);
    }
    for _ in 0..2 {
        assert!(gpu.prepare_pages(&pages, [].into_iter(), &device, &queue, budget));
    }
    assert!(gpu.page(0).is_some());
    assert_eq!(allocations.get(), 2);
    assert!(gpu.prepare_pages(&pages, [].into_iter(), &device, &queue, budget));
    assert_eq!(allocations.get(), 3);
}

#[test]
fn pending_replacement_keeps_previous_texture_and_uses_latest_material_flags() {
    let (device, queue) = device();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&publication(1, 1), &device, &queue));
    let previous = gpu.page(0).unwrap()._texture.id();
    let mut pages = publication(2, 1);
    let budget = pages.pages[0].rgba8.len() / 3;
    assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    assert_eq!(gpu.page(0).unwrap()._texture.id(), previous);
    pages.entity_identity = [3; 32];
    let mut page = pages.pages[0].clone();
    page.rgba8 = page.rgba8.to_vec().into();
    page.color_mask = true;
    page.multitexture = true;
    pages.pages = Arc::from([page]);
    assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    assert_eq!(gpu.page(0).unwrap()._texture.id(), previous);
    assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    let ready = gpu.page(0).unwrap();
    assert_ne!(ready._texture.id(), previous);
    assert!(ready.source.shares_pixels_with(&pages.pages[0]));
    assert!(ready.color_mask && ready.multitexture);
}

#[test]
fn superseded_pending_pixels_are_released_without_being_published() {
    let (device, queue) = device();
    let pages = publication(1, 1);
    let weak = Arc::downgrade(&pages.pages[0].rgba8);
    let budget = pages.pages[0].rgba8.len() / 3;
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    drop(pages);
    let next = publication(2, 1);
    for frame in 1..=3 {
        assert!(gpu.prepare_pages(&next, [1].into_iter(), &device, &queue, budget));
        assert!(weak.upgrade().is_none());
        assert_eq!(gpu.page(0).is_some(), frame == 3);
    }
    assert!(
        gpu.page(0)
            .unwrap()
            .source
            .shares_pixels_with(&next.pages[0])
    );
}

#[cfg(feature = "shaderpacks")]
#[test]
fn pending_pages_restart_with_the_new_raw_view_formats() {
    use crate::actor::gpu::permits_raw_view;
    let (device, queue) = device();
    let pages = publication(1, 1);
    let budget = pages.pages[0].rgba8.len() / 3;
    for pack in [true, false] {
        let mut gpu = GpuArtwork::default();
        gpu.set_raw_views(ArtworkRawViews(!pack));
        assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
        assert!(gpu.page(0).is_none());
        gpu.set_raw_views(ArtworkRawViews(pack));
        for frame in 1..=3 {
            assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
            assert_eq!(gpu.page(0).is_some(), frame == 3);
        }
        let ready = gpu.page(0).unwrap();
        assert_eq!(ready.raw_texture().is_some(), pack);
        assert_eq!(
            permits_raw_view(device.wgpu_device(), &ready._texture),
            pack
        );
    }
}

#[test]
fn changing_view_formats_restarts_pending_uploads_and_keeps_complete_pages() {
    let (device, queue) = device();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&publication(1, 1), &device, &queue));
    for (marker, raw) in [(2, true), (3, false)] {
        let previous = gpu.page(0).unwrap()._texture.id();
        let pages = publication(marker, 1);
        let budget = pages.pages[0].rgba8.len() / 3;
        assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
        gpu.set_raw_views(ArtworkRawViews(raw));
        for frame in 1..=3 {
            assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
            assert_eq!(gpu.page(0).unwrap()._texture.id() == previous, frame < 3);
        }
        let ready = gpu.page(0).unwrap();
        assert!(ready.source.shares_pixels_with(&pages.pages[0]));
        assert_eq!(ready.raw_views, ArtworkRawViews(raw));
        #[cfg(feature = "shaderpacks")]
        assert_eq!(
            crate::actor::gpu::permits_raw_view(device.wgpu_device(), &ready._texture),
            raw
        );
    }
}

#[cfg(feature = "shaderpacks")]
#[test]
fn shaderpack_activation_keeps_pending_fallback_pages_native() {
    use crate::actor_render::tests::allocation_fault_tests::{draw_frame, fixture, publication};
    use crate::shaderpack::actors::RigSource;

    let mut app = fixture();
    let mut frame = publication(1, &[55, 77], 1);
    let large = &mut Arc::make_mut(&mut Arc::make_mut(&mut frame.artwork).pages)[1];
    large.width = 1024;
    large.height = 1024;
    large.layers = 3;
    large.rgba8 = [77, 64, 128, 255]
        .repeat(usize::from(large.width) * usize::from(large.height) * large.layers as usize)
        .into();
    let frames = large.rgba8.len().div_ceil(UPLOAD_BYTES_PER_FRAME);
    assert!(frames > 1);
    for _ in 0..frames {
        draw_frame(&mut app, &frame);
    }
    let previous = app
        .sub_app(RenderApp)
        .world()
        .resource::<ActorGpu>()
        .artwork
        .page(1)
        .unwrap()
        ._texture
        .id();
    app.sub_app_mut(RenderApp)
        .insert_resource(ArtworkRawViews(true));
    for step in 1..=frames {
        assert_eq!(draw_frame(&mut app, &frame).len(), 2);
        let world = app.sub_app(RenderApp).world();
        let gpu = world.resource::<ActorGpu>();
        let ready = step == frames;
        assert_eq!(
            gpu.artwork.page(1).unwrap()._texture.id() != previous,
            ready
        );
        assert!(gpu.pack_supports_span(&gpu.spans[0]));
        assert_eq!(gpu.pack_supports_span(&gpu.spans[1]), ready);
        let mut replaced = Vec::new();
        crate::actor_render::shaderpack_source::shaderpack_actors(world)
            .unwrap()
            .for_each_instance(&mut |instance| replaced.push(instance.index));
        assert_eq!(replaced, if ready { vec![0, 1] } else { vec![0] });
    }
}

#[test]
fn pending_retries_preserve_backoff_until_demanded_uploads_complete() {
    let (device, queue) = device();
    let pages = publication(1, 1);
    let budget = usize::from(pages.pages[0].width) * 4 * 3;
    for boundary in ["actor page upload", "actor page view"] {
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&ActorArtworkPages::default(), &device, &queue));
        {
            let _failure = gpu_allocation::inject(|label, _| label == "immutable actor page");
            assert!(!gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
        }
        let writes = Rc::new(Cell::new(0));
        let observed = writes.clone();
        let failure = gpu_allocation::inject(move |label, _| {
            if label == "actor page upload" {
                observed.set(observed.get() + 1);
            }
            label == boundary
        });
        let mut delay = gpu_allocation::MIN_RETRY_FRAMES;
        for _ in 0..3 {
            let before = writes.get();
            for _ in 1..delay {
                assert!(!gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
                assert_eq!(writes.get(), before, "{boundary}: retries must back off");
            }
            let exhausted = (0..pages.pages[0].rgba8.len()).any(|_| {
                let before = writes.get();
                let prepared = gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget);
                assert!(
                    writes.get() > before,
                    "{boundary}: an active retry must advance on consecutive frames"
                );
                assert!(gpu.page(0).is_none());
                !prepared
            });
            assert!(exhausted, "{boundary}: reduced sizes must exhaust");
            delay *= 2;
        }
        drop(failure);
        for _ in 1..delay {
            assert!(!gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
        }
        let frames = pages.pages[0].rgba8.len().div_ceil(budget);
        for step in 1..=frames {
            assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
            assert_eq!(gpu.page(0).is_some(), step == frames);
        }
        assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    }
}

#[cfg(feature = "shaderpacks")]
#[test]
fn reduced_pending_pages_keep_the_requested_view_format() {
    let (device, queue) = device();
    let pages = publication(1, 1);
    for boundary in ["actor page upload", "actor page view"] {
        let mut gpu = GpuArtwork::default();
        gpu.set_raw_views(ArtworkRawViews(true));
        assert!(gpu.prepare(&ActorArtworkPages::default(), &device, &queue));
        let mut failed = false;
        let _failure = gpu_allocation::inject(move |label, _| {
            if label != boundary || failed {
                return false;
            }
            failed = true;
            true
        });
        assert!(gpu.prepare_demanded(&pages, &[1], &device, &queue));
        assert!(gpu.page(0).is_none());
        assert!(gpu.prepare_demanded(&pages, &[1], &device, &queue));
        let ready = gpu.page(0).unwrap();
        assert!(ready._texture.width() < u32::from(pages.pages[0].width));
        assert!(crate::actor::gpu::permits_raw_view(
            device.wgpu_device(),
            ready.raw_texture().unwrap()
        ));
    }
}

#[test]
fn failed_row_writes_spend_the_budget_and_keep_a_compatible_previous_page() {
    let (device, queue) = device();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&publication(1, 2), &device, &queue));
    let previous = gpu.page(0).unwrap()._texture.id();
    let pages = publication(2, 2);
    let budget = pages.pages[0].rgba8.len() / 3;
    assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    let written = Rc::new(Cell::new(0));
    {
        let observed = written.clone();
        let _failure = gpu_allocation::inject(move |label, bytes| {
            if label == "actor page upload" {
                observed.set(observed.get() + bytes as usize);
                return true;
            }
            false
        });
        assert!(gpu.prepare_pages(&pages, [1, 2].into_iter(), &device, &queue, budget));
    }
    assert_eq!(written.get(), budget);
    assert_eq!(gpu.page(0).unwrap()._texture.id(), previous);
    for _ in 0..3 {
        assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    }
    assert_ne!(gpu.page(0).unwrap()._texture.id(), previous);
    assert!(
        gpu.page(0)
            .unwrap()
            .source
            .shares_pixels_with(&pages.pages[0])
    );
}

#[test]
fn fresh_pages_reduce_after_staging_failure_without_exceeding_the_budget() {
    let (device, queue) = device();
    let pages = publication(1, 1);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&ActorArtworkPages::default(), &device, &queue));
    let budget = pages.pages[0].rgba8.len() / 3;
    let written = Rc::new(Cell::new(0));
    let observed = written.clone();
    let _failure = gpu_allocation::inject(move |label, bytes| {
        if label == "actor page upload" {
            observed.set(observed.get() + bytes as usize);
            return bytes > 24;
        }
        false
    });
    assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    assert!(gpu.page(0).is_none());
    assert_eq!(written.replace(0), budget);
    assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
    let ready = gpu.page(0).unwrap();
    assert_eq!(ready._texture.width(), 1);
    assert_eq!(
        ready._texture.depth_or_array_layers(),
        pages.pages[0].layers
    );
    assert!(ready.source.shares_pixels_with(&pages.pages[0]));
    assert_eq!(written.get(), 24);
}

#[test]
fn row_bands_preserve_pixels_across_layer_boundaries() {
    let Some(native) =
        crate::gpu_snapshot::Gpu::for_fixture("row_bands_preserve_pixels_across_layer_boundaries")
    else {
        return;
    };
    let device = RenderDevice::from(native.device.clone());
    let queue = RenderQueue::new(native.queue.clone());
    let mut pages = publication(1, 1);
    let mut page = pages.pages[0].clone();
    page.rgba8 = (0..page.layers)
        .flat_map(|layer| {
            (0..page.height).flat_map(move |y| {
                (0..page.width).flat_map(move |x| {
                    [
                        32 + x as u8 * 64,
                        24 + y as u8 * 48,
                        32 + layer as u8 * 96,
                        255,
                    ]
                })
            })
        })
        .collect();
    pages.pages = Arc::from([page.clone()]);
    let reference = upload_page(&device, &queue, &page, "complete fixture", default()).unwrap();
    let view = reference.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    let draw = |view: &TextureView| {
        native.render(
            r#"
struct Vertex { @builtin(position) position: vec4f, @location(0) uv: vec2f }
@group(0) @binding(0) var artwork: texture_2d_array<f32>;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let positions = array(vec2f(-1., -1.), vec2f(3., -1.), vec2f(-1., 3.));
    let p = positions[index];
    return Vertex(vec4f(p, 0.5, 1.), p * 0.5 + 0.5);
}
@fragment fn fragment(in: Vertex) -> @location(0) vec4f {
    let size = textureDimensions(artwork);
    let texel = vec2u(in.uv * vec2f(f32(size.x * textureNumLayers(artwork)), f32(size.y)));
    return textureLoad(artwork, vec2i(vec2u(texel.x % size.x, texel.y)), i32(texel.x / size.x), 0);
}
"#,
            "vertex",
            &[crate::gpu_snapshot::Draw {
                fragment: "fragment",
                vertices: 0..3,
                bindings: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                }],
                blend: None,
                write_depth: false,
            }],
        )
    };
    let before = draw(&view);
    let mut gpu = GpuArtwork::default();
    let row = usize::from(page.width) * 4;
    let budget = row * 2 + 1;
    let frames = page.rgba8.len().div_ceil(row * 2);
    for frame in 1..=frames {
        assert!(gpu.prepare_pages(&pages, [1].into_iter(), &device, &queue, budget));
        assert_eq!(gpu.page(0).is_some(), frame == frames);
        queue.submit([]);
    }
    let after = draw(&gpu.page(0).unwrap().view);
    assert!(
        before
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel != &before[..4])
    );
    assert_eq!(after, before, "every layer and row retains the same pixels");
    crate::gpu_snapshot::save("actor-row-bands-before", &before);
    crate::gpu_snapshot::save("actor-row-bands-after", &after);
}
