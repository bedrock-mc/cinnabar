use super::*;

#[test]
fn adapter_limits_reject_oversized_texture_arrays() {
    assert_eq!(
        TextureArrayLimits {
            max_layers: 4,
            max_dimension_2d: 16,
        }
        .validate(5, 16),
        Err(TextureLimitError::Layers {
            requested: 5,
            supported: 4,
        })
    );
    assert_eq!(
        TextureArrayLimits {
            max_layers: 4,
            max_dimension_2d: 8,
        }
        .validate(4, 16),
        Err(TextureLimitError::Dimension {
            requested: 16,
            supported: 8,
        })
    );
}

#[test]
fn mip_upload_plan_preserves_exact_layer_offsets_and_row_padding() {
    let plans = plan_texture_mip_uploads(runtime_assets().texture_array(), 256)
        .expect("plan synthetic texture upload");
    assert_eq!(plans.len(), 5);
    assert_eq!(
        plans
            .iter()
            .map(|plan| (
                plan.mip_level,
                plan.size,
                plan.bytes_per_row,
                plan.rows_per_image,
                plan.layer_source_offsets.as_ref(),
                plan.layer_staging_offsets.as_ref(),
                plan.staging_bytes,
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, 16, 256, 16, &[0][..], &[0][..], 4096),
            (1, 8, 256, 8, &[0][..], &[0][..], 2048),
            (2, 4, 256, 4, &[0][..], &[0][..], 1024),
            (3, 2, 256, 2, &[0][..], &[0][..], 512),
            (4, 1, 256, 1, &[0][..], &[0][..], 256),
        ]
    );
    assert_eq!(runtime_assets().materials().len(), 14);

    let two_layers = TextureArray {
        layers: 2,
        mips: vec![TextureMip {
            size: 2,
            rgba8: vec![0; 2 * 2 * 4 * 2].into_boxed_slice(),
        }]
        .into_boxed_slice(),
    };
    let plan = plan_texture_mip_uploads(&two_layers, 256)
        .expect("plan two-layer texture upload")
        .remove(0);
    assert_eq!(plan.layer_source_offsets.as_ref(), [0, 16]);
    assert_eq!(plan.layer_staging_offsets.as_ref(), [0, 512]);
    assert_eq!(plan.staging_bytes, 1024);
}

#[test]
fn global_texture_bind_group_rebuilds_only_for_new_asset_identity_or_revision() {
    let current = texture_identity(0x1000, 7);
    assert!(!texture_asset_needs_rebuild(Some(current), current));
    assert!(texture_asset_needs_rebuild(
        Some(current),
        texture_identity(0x2000, 7)
    ));
    assert!(texture_asset_needs_rebuild(
        Some(current),
        texture_identity(0x1000, 8)
    ));
    assert!(texture_asset_needs_rebuild(None, current));
}

fn texture_ref(page: u32, layer: u32) -> TextureRef {
    TextureRef::new(page, layer).expect("valid synthetic texture reference")
}

fn animation(flags: u32) -> Animation {
    Animation {
        frame_start: 0,
        frame_count: 3,
        ticks_per_frame: 2,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 1,
        flags,
    }
}

#[test]
fn animation_selects_current_next_cross_page_wrap_and_non_blended_frames() {
    let frames = [texture_ref(0, 7), texture_ref(0, 8), texture_ref(1, 3)];
    let material = Material {
        texture: frames[0],
        flags: 0,
        animation: 0,
    };
    let sample = |tick, partial_tick, flags| {
        select_animation_frames(
            material,
            &[animation(flags)],
            &frames,
            ChunkAnimationClock::from_parts(tick, partial_tick),
        )
    };

    assert_eq!(
        sample(0, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.0)
    );
    assert_eq!(
        sample(1, 0.5, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.75)
    );
    assert_eq!(
        sample(2, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[1], frames[2], 0.0)
    );
    assert_eq!(
        sample(5, 0.5, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[2], frames[0], 0.75)
    );
    assert_eq!(
        sample(6, 0.0, ANIMATION_FLAG_BLEND),
        AnimationFrameSample::new(frames[0], frames[1], 0.0)
    );
    assert_eq!(
        sample(3, 0.9, 0),
        AnimationFrameSample::new(frames[1], frames[1], 0.0)
    );

    let static_material = Material {
        texture: texture_ref(1, 4),
        flags: 0,
        animation: NO_ANIMATION,
    };
    assert_eq!(
        select_animation_frames(
            static_material,
            &[],
            &[],
            ChunkAnimationClock::from_parts(123, 0.75),
        ),
        AnimationFrameSample::new(static_material.texture, static_material.texture, 0.0)
    );

    let one_frame = Animation {
        frame_count: 1,
        ..animation(ANIMATION_FLAG_BLEND)
    };
    assert_eq!(
        select_animation_frames(
            material,
            &[one_frame],
            &frames[..1],
            ChunkAnimationClock::from_parts(1, 0.5),
        ),
        AnimationFrameSample::new(frames[0], frames[0], 0.0)
    );
}

#[test]
fn one_and_two_page_assets_have_one_stable_shared_binding_shape() {
    assert_eq!(
        plan_texture_page_bindings(1),
        Some([
            TexturePageBinding::Asset(0),
            TexturePageBinding::DiagnosticFallback,
        ])
    );
    assert_eq!(
        plan_texture_page_bindings(2),
        Some([TexturePageBinding::Asset(0), TexturePageBinding::Asset(1)])
    );
    assert_eq!(plan_texture_page_bindings(0), None);
    assert_eq!(plan_texture_page_bindings(3), None);
}

#[test]
fn one_page_fallback_is_a_real_one_layer_copy_of_diagnostic_mips() {
    let source = TextureArray {
        layers: 2,
        mips: [4_u32, 2, 1]
            .into_iter()
            .map(|size| {
                let layer_bytes = size as usize * size as usize * 4;
                let mut rgba8 = vec![0x11; layer_bytes];
                rgba8.extend(vec![0x22; layer_bytes]);
                TextureMip {
                    size,
                    rgba8: rgba8.into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };

    let fallback = diagnostic_texture_page(&source).expect("extract diagnostic page");
    assert_eq!(fallback.layers, 1);
    assert_eq!(fallback.mips.len(), source.mips.len());
    for (source_mip, fallback_mip) in source.mips.iter().zip(&fallback.mips) {
        let layer_bytes = source_mip.size as usize * source_mip.size as usize * 4;
        assert_eq!(
            fallback_mip.rgba8.as_ref(),
            &source_mip.rgba8[..layer_bytes]
        );
    }
}

#[test]
fn animation_clock_updates_do_not_rebuild_or_reupload_texture_assets() {
    let identity = texture_identity(0x1000, 7);
    let mut current = None;
    let mut immutable_uploads = 0;
    let mut clock_bytes = 0;
    for frame in 0..120_u32 {
        if texture_asset_needs_rebuild(current, identity) {
            immutable_uploads += 1;
            current = Some(identity);
        }
        let clock = ChunkAnimationClock::from_elapsed_seconds(f64::from(frame) / 60.0);
        assert!(clock.partial_tick() >= 0.0 && clock.partial_tick() < 1.0);
        clock_bytes += size_of::<ChunkAnimationClock>();
    }
    assert_eq!(immutable_uploads, 1);
    assert_eq!(clock_bytes, 120 * 16);

    let plugin = CHUNK_RENDERER_SOURCE;
    assert_eq!(plugin.matches("render_queue.write_texture(").count(), 1);
    assert_eq!(
        plugin.matches("render_queue.write_buffer(").count(),
        9,
        "shared writers cover immutable geometry plus bounded liquid and model transparent sorts"
    );
    assert!(plugin.contains("render_queue.write_buffer(&gpu_clock.buffer"));
}

#[test]
fn asset_revision_replacement_is_atomic_and_retains_the_previous_prepared_set_on_failure() {
    let plugin = CHUNK_RENDERER_SOURCE;
    let start = plugin
        .find("fn prepare_chunk_texture_assets(")
        .expect("texture preparation system");
    let end = plugin[start..]
        .find("\npub(in crate::chunk) fn storage_table_fits(")
        .map(|offset| start + offset)
        .expect("end of texture preparation system");
    let prepare = &plugin[start..end];

    assert!(
        !prepare.contains("gpu_assets.prepared = None"),
        "a rejected new revision must retain the previous complete GPU asset set"
    );
    let second_page = prepare
        .find("let (texture_1, view_1, padded_1) = upload_texture_page(")
        .expect("second page is prepared before publication");
    let publish = prepare
        .find("gpu_assets.prepared = Some(PreparedChunkTextureAssets {")
        .expect("complete revision publication");
    assert!(second_page < publish);
    assert!(prepare.contains("material.texture.raw()"));
    assert!(prepare.contains(".animations()"));
    assert!(prepare.contains(".animation_frames()"));
    assert!(prepare.contains("_textures: [texture_0, texture_1]"));
    assert!(prepare.contains("views: [view_0, view_1]"));
}
