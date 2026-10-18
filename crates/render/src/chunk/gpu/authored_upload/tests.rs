use super::*;
use assets::texture::TextureMip;

#[test]
fn authored_streaming_profiles_each_frames_buffer_and_texture_bytes() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let source =
        source(vec![u32::MAX; EnhancedTextureAssets::REFERENCE_BASE_WORDS].into_boxed_slice());
    let reference_bytes = std::mem::size_of_val(source.texture_refs.as_ref());
    let mut upload = AuthoredUpload::new(
        ChunkTextureAssets::default().binding_identity(),
        source,
        &device,
    )
    .unwrap();
    let (written, counts) =
        render_scene::runtime_profile_uploads::measure(|| upload.step(&queue, reference_bytes));
    assert_eq!(written, reference_bytes);
    assert_eq!(counts, (1, written as u64));
    for _ in 0..3 {
        let (written, counts) =
            render_scene::runtime_profile_uploads::measure(|| upload.step(&queue, 8));
        assert_eq!(written, 8);
        assert_eq!(counts, (2, written as u64));
    }
    assert!(upload.complete());
    let (written, counts) =
        render_scene::runtime_profile_uploads::measure(|| upload.step(&queue, 8));
    assert_eq!((written, counts), (0, (0, 0)));
}

/// Builds a complete layer-major mip chain for upload scheduling tests.
fn array(side: u32, layers: u32) -> TextureArray {
    TextureArray {
        layers,
        mips: (0..=side.ilog2())
            .map(|level| {
                let size = side >> level;
                TextureMip {
                    size,
                    rgba8: vec![level as u8; (size * size * layers * 4) as usize]
                        .into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

/// Builds a valid material/frame table spanning exactly two normal upload budgets.
fn oversized_references() -> Arc<EnhancedTextureAssets> {
    let word_bytes = std::mem::size_of::<u32>();
    let words = 2 * FRAME_UPLOAD_BYTES / word_bytes;
    let base = EnhancedTextureAssets::REFERENCE_BASE_WORDS;
    let stride = EnhancedTextureAssets::REFERENCE_OVERRIDE_WORDS;
    let overrides = (words - base) / stride;
    assert_eq!(base + overrides * stride, words);
    let mut references = Vec::with_capacity(words);
    references.resize(base, u32::MAX);
    for index in 0..overrides {
        references.extend_from_slice(&[
            (index / assets::MAX_TEXTURE_LAYERS) as u32,
            (index % assets::MAX_TEXTURE_LAYERS) as u32,
            0,
        ]);
    }
    source(references.into_boxed_slice())
}

/// Pairs a checked reference table with minimal complete authored texture pages.
fn source(references: Box<[u32]>) -> Arc<EnhancedTextureAssets> {
    let [color0, color1, normal0, normal1, material0, material1] =
        std::array::from_fn(|_| array(1, 1));
    Arc::new(
        EnhancedTextureAssets::new(
            [color0, color1],
            [normal0, normal1],
            [material0, material1],
            references,
        )
        .unwrap(),
    )
}

/// A device-admitted table larger than one frame must finish before any bindings publish.
#[test]
fn reference_table_twice_the_frame_budget_completes_in_three_frames() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let base = ChunkTextureAssets::default();
    let (mut prepared, mut stats) =
        crate::chunk::gpu::texture_upload::build_chunk_texture_assets(&base, &device, &queue)
            .unwrap();
    let old_references = prepared.enhanced_texture_refs.id();
    let old_views = prepared.enhanced_views.each_ref().map(|view| view.id());
    let source = oversized_references();
    assert!(source.fits_device_limits(&device.limits()));
    assert_eq!(
        std::mem::size_of_val(source.texture_refs.as_ref()),
        2 * FRAME_UPLOAD_BYTES
    );
    let texture_bytes: usize = source
        .pages()
        .into_iter()
        .flat_map(|page| page.mips.iter())
        .map(|mip| mip.rgba8.len())
        .sum();
    let candidate = base.with_updated_enhanced(Some(source.clone()));
    let mut upload =
        AuthoredUpload::new(candidate.binding_identity(), source.clone(), &device).unwrap();
    for _ in 0..2 {
        assert_eq!(upload.step(&queue, FRAME_UPLOAD_BYTES), FRAME_UPLOAD_BYTES);
        assert!(!upload.complete());
        assert_eq!(prepared.enhanced_texture_refs.id(), old_references);
        assert_eq!(
            prepared.enhanced_views.each_ref().map(|view| view.id()),
            old_views
        );
        assert_eq!(prepared.identity, base.binding_identity());
    }
    assert_eq!(upload.step(&queue, FRAME_UPLOAD_BYTES), texture_bytes);
    assert!(upload.complete());
    assert_eq!(upload.step(&queue, FRAME_UPLOAD_BYTES), 0);
    upload.publish(&mut prepared, &mut stats);
    assert_eq!(prepared.identity, candidate.binding_identity());
    assert_ne!(prepared.enhanced_texture_refs.id(), old_references);
    assert!(Arc::ptr_eq(
        prepared.authored_source.as_ref().unwrap(),
        &source
    ));
    assert_eq!(
        prepared.authored_upload_bytes,
        (2 * FRAME_UPLOAD_BYTES + texture_bytes) as u64
    );
}

/// Sub-word allowances wait, while an unaligned byte budget still uploads whole reference words.
#[test]
fn reference_segments_obey_word_alignment_and_make_progress_with_small_budgets() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue::new(queue);
    let source =
        source(vec![u32::MAX; EnhancedTextureAssets::REFERENCE_BASE_WORDS].into_boxed_slice());
    let identity = ChunkTextureAssets::default()
        .with_updated_enhanced(Some(source.clone()))
        .binding_identity();
    let mut upload = AuthoredUpload::new(identity, source, &device).unwrap();
    let word = std::mem::size_of::<u32>();
    assert_eq!(upload.step(&queue, 0), 0);
    assert_eq!(upload.step(&queue, word - 1), 0);
    assert_eq!(upload.step(&queue, word + 1), word);
    assert_eq!(upload.step(&queue, word + 2), word);
    assert!(!upload.complete());
}

#[test]
fn bounded_stripes_visit_each_page_mip_layer_and_row_exactly_once() {
    let arrays = [
        array(8, 3),
        array(4, 2),
        array(8, 3),
        array(4, 2),
        array(8, 3),
        array(4, 2),
    ];
    let pages = arrays.each_ref();
    let mut schedule = Schedule::new(&pages).unwrap();
    let mut visits = arrays.map(|array| {
        array
            .mips
            .iter()
            .map(|mip| vec![false; mip.rgba8.len()])
            .collect::<Vec<_>>()
    });
    let budget = 68;
    let mut frames = 0;
    while !schedule.complete() {
        let mut remaining = budget;
        let mut frame_bytes = 0;
        while let Some(stripe) = schedule.next(remaining) {
            assert!(stripe.bytes <= remaining);
            assert!(stripe.offset + stripe.bytes <= visits[stripe.page][stripe.mip as usize].len());
            for written in &mut visits[stripe.page][stripe.mip as usize]
                [stripe.offset..stripe.offset + stripe.bytes]
            {
                assert!(!*written, "each authored source byte must be uploaded once");
                *written = true;
            }
            remaining -= stripe.bytes;
            frame_bytes += stripe.bytes;
        }
        assert!(frame_bytes > 0 && frame_bytes <= budget);
        frames += 1;
    }
    assert!(frames > 1);
    assert!(visits.iter().flatten().flatten().all(|written| *written));
    assert!(schedule.next(budget).is_none());
}

#[test]
fn scheduler_does_not_advance_when_a_row_exceeds_the_budget() {
    let arrays = std::array::from_fn(|_| array(8, 1));
    let mut schedule = Schedule::new(&arrays.each_ref()).unwrap();
    assert!(schedule.next(31).is_none());
    let first = schedule.next(32).unwrap();
    assert_eq!(
        (first.page, first.layer, first.row, first.rows),
        (0, 0, 0, 1)
    );
    assert_eq!(first.bytes, 32);
}

#[test]
fn whole_layers_are_coalesced_without_padding_the_remaining_mips() {
    let arrays = std::array::from_fn(|_| array(4, 4));
    let mut schedule = Schedule::new(&arrays.each_ref()).unwrap();
    let first = schedule.next(128).unwrap();
    assert_eq!((first.layer_count, first.rows, first.bytes), (2, 4, 128));
    let second = schedule.next(128).unwrap();
    assert_eq!(
        (second.layer, second.layer_count, second.bytes),
        (2, 2, 128)
    );
    let third = schedule.next(128).unwrap();
    assert_eq!((third.mip, third.layer_count, third.bytes), (1, 4, 64));
}

#[test]
fn obsolete_authored_identity_cancels_without_changing_carrier_identity() {
    let current = ChunkTextureBindingIdentity {
        carrier: render_scene::ChunkTextureAssetIdentity::new(1, 7),
        enhanced_pointer: 2,
    };
    let replacement = ChunkTextureBindingIdentity {
        enhanced_pointer: 3,
        ..current
    };
    assert_ne!(current, replacement);
    assert!(same_carrier(current, replacement));
    assert!(!same_carrier(
        current,
        ChunkTextureBindingIdentity {
            carrier: render_scene::ChunkTextureAssetIdentity::new(1, 8),
            ..replacement
        }
    ));
    assert!(!same_carrier(
        current,
        ChunkTextureBindingIdentity {
            carrier: render_scene::ChunkTextureAssetIdentity::new(4, 7),
            ..replacement
        }
    ));
    let arrays = std::array::from_fn(|_| array(8, 2));
    let mut obsolete = Schedule::new(&arrays.each_ref()).unwrap();
    obsolete.next(32).unwrap();
    let mut replacement = Schedule::new(&arrays.each_ref()).unwrap();
    let first = replacement.next(32).unwrap();
    assert_eq!(
        (first.page, first.mip, first.layer, first.row),
        (0, 0, 0, 0)
    );
}

#[test]
fn malformed_mip_shapes_are_rejected_before_any_gpu_upload() {
    let mut arrays = std::array::from_fn(|_| array(8, 1));
    arrays[2].mips[1].rgba8 = vec![0; 3].into_boxed_slice();
    assert!(Schedule::new(&arrays.each_ref()).is_none());
}

#[path = "native.rs"]
mod native;
