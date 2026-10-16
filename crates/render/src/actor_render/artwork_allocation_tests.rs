use super::*;
use std::{cell::Cell, rc::Rc};

/// Synthetic source pages use no installed assets or account data.
fn publication(marker: u8, layers: u32) -> ActorArtworkPages {
    let mut pages = ActorArtworkPages::default();
    pages.entity_identity = [marker; 32];
    pages.pages = Arc::from([render_model::ActorTexturePage {
        width: 16,
        height: 16,
        layers,
        rgba8: vec![marker; 16 * 16 * layers as usize * 4].into(),
        color_mask: false,
        multitexture: false,
    }]);
    pages
}

/// No-op resources expose stable identities without consuming physical GPU memory.
fn device() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    (device.into(), RenderQueue::new(queue))
}

#[test]
fn page_allocation_failure_retains_previous_texture_without_uploading() {
    let (device, queue) = device();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&publication(1, 1), &device, &queue));
    let previous = gpu.pages[0]._texture.id();
    let uploads = work_tests::upload_count();
    let _failure = allocation::inject(|label, _| label == "immutable actor page");
    assert!(gpu.prepare(&publication(2, 1), &device, &queue));
    assert_eq!(gpu.pages[0]._texture.id(), previous);
    assert_eq!(
        work_tests::upload_count(),
        uploads,
        "failed textures receive no writes"
    );
}

#[test]
fn first_page_allocation_failure_retries_through_one_texel() {
    let (device, queue) = device();
    let attempts = Rc::new(Cell::new(0));
    let observed = Rc::clone(&attempts);
    let _failure = allocation::inject(move |label, bytes| {
        if label != "immutable actor page" {
            return false;
        }
        observed.set(observed.get() + 1);
        bytes > 4
    });
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&publication(1, 1), &device, &queue));
    assert_eq!(gpu.pages[0]._texture.width(), 1);
    assert_eq!(attempts.get(), 5);
}

#[test]
fn superseded_pages_are_released_before_replacement_allocation() {
    let (device, queue) = device();
    let mut old = publication(1, 1);
    let removed = publication(2, 1).pages[0].clone();
    let weak = Arc::downgrade(&removed.rgba8);
    old.pages = Arc::from([old.pages[0].clone(), removed]);
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&old, &device, &queue));
    drop(old);
    let _observe = allocation::inject(move |label, _| {
        if label == "immutable actor page" {
            assert!(
                weak.upgrade().is_none(),
                "unused previous page still retained during allocation"
            );
        }
        false
    });
    assert!(gpu.prepare(&publication(3, 1), &device, &queue));
}

#[test]
fn reduced_actor_uploads_sample_every_layer_with_normalized_uvs() {
    let Some(native) = crate::gpu_snapshot::Gpu::for_fixture(
        "reduced_actor_uploads_sample_every_layer_with_normalized_uvs",
    ) else {
        return;
    };
    let (device, queue) = (
        RenderDevice::from(native.device.clone()),
        RenderQueue::new(native.queue.clone()),
    );
    let mut pixels = Vec::new();
    for layer in 0..2 {
        for y in 0..64 {
            for x in 0..64 {
                let color = if x < 8 || y < 8 || x >= 56 || y >= 56 {
                    [32, 96, 192, 255]
                } else if y >= 40 {
                    if (x + y) % 2 == 0 {
                        [240, 160, 48, 255]
                    } else {
                        [48, 48, 48, 255]
                    }
                } else if layer == 0 {
                    [240, 160, 48, 255]
                } else {
                    [64, 192, 112, 255]
                };
                pixels.extend_from_slice(&color);
            }
        }
    }
    let page = render_model::ActorTexturePage {
        width: 64,
        height: 64,
        layers: 2,
        rgba8: pixels.into(),
        color_mask: false,
        multitexture: false,
    };
    let before_texture =
        upload_or_reduce(&device, &queue, &page, "original fixture", true).unwrap();
    let before_view = before_texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    let mut failed = false;
    let _failure = allocation::inject(move |label, _| {
        if label != "reduced fixture" || failed {
            return false;
        }
        failed = true;
        true
    });
    let after_texture = upload_or_reduce(&device, &queue, &page, "reduced fixture", true).unwrap();
    let after_view = after_texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    assert_eq!(after_texture.width(), before_texture.width() / 2);
    assert_eq!(after_texture.depth_or_array_layers(), page.layers);
    let sampler = native
        .device
        .create_sampler(&wgpu::SamplerDescriptor::default());
    let draw = |view: &TextureView| {
        let entries = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ];
        native.render(
            r#"
struct Vertex { @builtin(position) position: vec4f, @location(0) uv: vec2f }
@group(0) @binding(0) var artwork: texture_2d_array<f32>;
@group(0) @binding(1) var artwork_sampler: sampler;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let positions = array(vec2f(-1., -1.), vec2f(3., -1.), vec2f(-1., 3.));
    let p = positions[index];
    return Vertex(vec4f(p, 0.5, 1.), p * 0.5 + 0.5);
}
@fragment fn fragment(in: Vertex) -> @location(0) vec4f {
    let layer = i32(in.uv.x >= 0.5);
    let uv = vec2f(fract(in.uv.x * 2.), in.uv.y);
    return textureSample(artwork, artwork_sampler, uv, layer);
}
"#,
            "vertex",
            &[crate::gpu_snapshot::Draw {
                fragment: "fragment",
                vertices: 0..3,
                bindings: &entries,
                blend: None,
                write_depth: false,
            }],
        )
    };
    let before = draw(&before_view);
    let after = draw(&after_view);
    for x in [64, 192] {
        let at = (128 * crate::gpu_snapshot::SNAPSHOT_SIDE as usize + x) * 4;
        assert_eq!(&before[at..at + 4], &after[at..at + 4]);
        assert_eq!(after[at + 3], 255);
    }
    assert_ne!(
        before, after,
        "fine detail changes while both layers remain visible"
    );
    if let Some(directory) = std::env::var_os("CINNABAR_REVIEW_SNAPSHOT_DIR") {
        std::fs::create_dir_all(directory).unwrap();
        crate::gpu_snapshot::save("actor-artwork-before", &before);
        crate::gpu_snapshot::save("actor-artwork-after", &after);
    }
}

#[test]
fn exhausting_all_page_sizes_skips_generic_artwork_without_repeating_allocations() {
    let (device, queue) = device();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&ActorArtworkPages::default(), &device, &queue));
    let uploads = work_tests::upload_count();
    let attempts = Rc::new(Cell::new(0));
    let observed = Rc::clone(&attempts);
    let _failure = allocation::inject(move |label, _| {
        if label != "immutable actor page" {
            return false;
        }
        observed.set(observed.get() + 1);
        true
    });
    let pages = publication(1, 2);
    assert!(!gpu.prepare(&pages, &device, &queue));
    assert!(gpu.pages.is_empty());
    assert!(gpu.glint.texture.is_some());
    assert_eq!(attempts.get(), 5);
    assert!(!gpu.prepare(&pages, &device, &queue));
    assert_eq!(attempts.get(), 5);
    assert_eq!(work_tests::upload_count(), uploads);
}

#[test]
fn incompatible_old_layer_count_uses_coarse_new_data_instead() {
    let (device, queue) = device();
    let mut gpu = GpuArtwork::default();
    assert!(gpu.prepare(&publication(1, 1), &device, &queue));
    let previous = gpu.pages[0]._texture.id();
    let _failure = allocation::inject(|label, bytes| label == "immutable actor page" && bytes > 8);
    assert!(gpu.prepare(&publication(2, 2), &device, &queue));
    assert_ne!(gpu.pages[0]._texture.id(), previous);
    assert_eq!(gpu.pages[0]._texture.depth_or_array_layers(), 2);
    assert_eq!(gpu.pages[0]._texture.width(), 1);
}
