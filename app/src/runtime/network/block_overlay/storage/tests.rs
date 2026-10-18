use super::super::{MAX_OVERLAY_SHARDS, MIN_TILE, textures::TextureCatalog};
use super::*;
use resource_pack::LayeredPackView;

mod capacity;

/// Admits a resource pack containing only the original synthetic files supplied by a test.
fn view(files: &[(&str, &[u8])]) -> LayeredPackView {
    use std::io::Write;
    let id = "00000000-0000-0000-0000-000000000037";
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                writer.finish().unwrap().into_inner(),
            ),
        ]),
    ))
}

/// Occupies every shard with small layers for decode, rollback and resizing fixtures.
fn builder<'a>(
    view: &'a LayeredPackView,
    keys: &assets::material_keys::MaterialKeys,
) -> Builder<'a> {
    Builder {
        fallback: None,
        base_keys: None,
        catalog: TextureCatalog::new(view, Some(keys)),
        overlay: Default::default(),
        sources: (0..super::super::MAX_OVERLAY_SHARDS)
            .flat_map(|shard| (0..3).map(move |_| (Source::Diagnostic, shard)))
            .collect(),
        source_bytes: 0,
        tiles: vec![MIN_TILE; super::super::MAX_OVERLAY_SHARDS],
        upload_bytes: assets::texture::policy::device_policy()
            .terrain_bytes(MIN_TILE, 3 * super::super::MAX_OVERLAY_SHARDS)
            .unwrap(),
        texture_policy: assets::texture::policy::device_policy(),
        max_layers: assets::MAX_TEXTURE_LAYERS,
        slots: Default::default(),
        base_assets: None,
        terrain_materials: Default::default(),
        vanilla_carried: Vec::new(),
        materials: Default::default(),
        visuals: Default::default(),
        gaps: Default::default(),
    }
}

/// Encodes original opaque pixels for decode-work and admission regressions.
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([31, 47, 59, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[test]
fn static_decode_failures_are_shared_across_selectors_and_aliases() {
    let small = png(16, 16);
    for rejected in [b"invalid PNG".as_slice(), b"".as_slice()] {
        let view = view(&[
            ("textures/rejected.png", rejected),
            ("textures/small.png", &small),
        ]);
        let keys = assets::material_keys::MaterialKeys::from_entries([
            (1, "tile"),
            (2, "alias"),
            (3, "small"),
        ])
        .with_aliases([
            ("tile", "textures/rejected"),
            ("alias", " ./textures/rejected "),
            ("small", "textures/small"),
        ])
        .with_flipbook_destinations([("tile", vec![1; 16]), ("alias", vec![1; 16])]);
        let mut builder = builder(&view, &keys);
        for index in 0..16 {
            assert!(builder.texture_slot_for("tile", (index, 0)).is_none());
            assert!(builder.texture_slot_for("alias", (index, 0)).is_none());
        }
        assert_eq!(builder.catalog.decode_attempts.get(), 1);
        assert_eq!(builder.sources.len(), 3 * super::super::MAX_OVERLAY_SHARDS);
        assert!(builder.texture_slot_for("small", (0, 0)).is_some());
        assert_eq!(builder.catalog.decode_attempts.get(), 2);
        assert_eq!(
            builder.sources.len(),
            3 * super::super::MAX_OVERLAY_SHARDS + 1
        );
    }
}

#[test]
fn failed_static_storage_does_not_hide_an_independent_animation() {
    let invalid = b"invalid PNG".as_slice();
    let small = png(16, 32);
    let view = view(&[
        ("textures/rejected.png", invalid),
        ("textures/frame.png", &small),
        (
            "textures/flipbook_textures.json",
            br#"[{
            "atlas_tile":"tile","atlas_index":1,"flipbook_texture":"textures/frame"
        }]"#,
        ),
    ]);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/rejected")])
        .with_flipbook_destinations([("tile", vec![1; 2])]);
    let mut builder = builder(&view, &keys);
    assert!(builder.texture_slot_for("tile", (0, 0)).is_none());
    let slot = builder.texture_slot_for("tile", (1, 0)).unwrap();
    assert_ne!(slot.animation, NO_ANIMATION);
    assert_eq!(
        builder.overlay.animations[slot.animation as usize].frame_count,
        2
    );
    assert!(builder.texture_slot_for("tile", (0, 1)).is_none());
    assert_eq!(builder.catalog.decode_attempts.get(), 2);
}

#[test]
fn failed_grid_admission_does_not_poison_another_grid_of_the_same_image() {
    let raster = png(16, 16);
    let view = view(&[
        ("textures/shared.png", &raster),
        (
            "textures/terrain_texture.json",
            br#"{"texture_data":{
            "invalid":{"textures":"textures/shared","quad":5},
            "valid":{"textures":"textures/shared","quad":0}
        }}"#,
        ),
    ]);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "invalid"), (2, "valid")]);
    let mut builder = builder(&view, &keys);
    assert!(builder.texture_slot_for("invalid", (0, 0)).is_none());
    assert!(builder.texture_slot_for("valid", (0, 0)).is_some());
    assert_eq!(builder.catalog.decode_attempts.get(), 2);
}

#[test]
fn animation_source_failures_decode_once_across_wildcard_variants() {
    let malformed_strip = png(48, 32);
    let fallback = png(16, 16);
    for rejected in [
        malformed_strip.as_slice(),
        b"invalid PNG".as_slice(),
        b"".as_slice(),
    ] {
        let view = view(&[
            ("textures/rejected.png", rejected),
            ("textures/fallback.png", &fallback),
            (
                "textures/flipbook_textures.json",
                br#"[{"atlas_tile":"tile","flipbook_texture":"textures/rejected"}]"#,
            ),
        ]);
        let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
            .with_aliases([("tile", "textures/fallback")])
            .with_flipbook_destinations([("tile", vec![16])]);
        let mut builder = builder(&view, &keys);
        let first = builder.texture_slot_for("tile", (0, 0)).unwrap();
        for variant in 1..16 {
            let slot = builder.texture_slot_for("tile", (0, variant)).unwrap();
            assert_eq!(slot.layer, first.layer);
            assert_eq!(slot.animation, NO_ANIMATION);
        }
        assert_eq!(builder.catalog.decode_attempts.get(), 2);
        assert_eq!(
            builder.sources.len(),
            3 * super::super::MAX_OVERLAY_SHARDS + 1
        );
    }
}

#[test]
fn destination_timing_failure_does_not_poison_shared_animation_storage() {
    let strip = png(16, 32);
    let fallback = png(16, 16);
    let description = serde_json::to_vec(&(0..3).map(|variant| serde_json::json!({
        "atlas_tile":"tile", "atlas_tile_variant":variant,
        "flipbook_texture":"textures/strip", "ticks_per_frame":if variant == 1 { 2 } else { u32::MAX }
    })).collect::<Vec<_>>()).unwrap();
    let view = view(&[
        ("textures/strip.png", &strip),
        ("textures/fallback.png", &fallback),
        ("textures/flipbook_textures.json", &description),
    ]);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/fallback")])
        .with_flipbook_destinations([("tile", vec![3])]);
    let mut builder = builder(&view, &keys);
    let first = builder.texture_slot_for("tile", (0, 0)).unwrap();
    assert_eq!(first.animation, NO_ANIMATION);
    let animated = builder.texture_slot_for("tile", (0, 1)).unwrap();
    assert_ne!(animated.animation, NO_ANIMATION);
    assert_eq!(
        builder.overlay.animations[animated.animation as usize].frame_count,
        2
    );
    let last = builder.texture_slot_for("tile", (0, 2)).unwrap();
    assert_eq!(last.layer, first.layer);
    assert_eq!(last.animation, NO_ANIMATION);
    assert_eq!(builder.catalog.decode_attempts.get(), 3);
}

#[test]
fn identical_pixels_at_distinct_source_frames_share_a_physical_layer() {
    let raster = png(16, 32);
    let view = view(&[
        ("textures/strip.png", &raster),
        (
            "textures/flipbook_textures.json",
            br#"[{"atlas_tile":"tile","flipbook_texture":"textures/strip","frames":[1,0,1]}]"#,
        ),
    ]);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/strip")]);
    let mut builder = builder(&view, &keys);
    builder.sources.truncate(1);
    builder.tiles.clear();
    builder.upload_bytes = builder.texture_policy.terrain_bytes(MIN_TILE, 1).unwrap();
    builder.max_layers = 2;
    let slot = builder.texture_slot_for("tile", (0, 0)).unwrap();
    assert_eq!(builder.sources.len(), 2);
    let animation = &builder.overlay.animations[slot.animation as usize];
    assert_eq!(animation.frame_count, 3);
    assert!(
        builder
            .overlay
            .animation_frames
            .iter()
            .all(|&frame| frame.layer() == slot.layer)
    );
}

#[test]
fn resolved_animation_aliases_reuse_frames_at_capacity_with_independent_timing() {
    let strip = png(16, 32);
    let view = view(&[
        ("textures/strip.png", &strip),
        ("textures/flipbook_textures.json", br#"[
            {"atlas_tile":"tile","atlas_index":0,"flipbook_texture":"textures/strip","ticks_per_frame":3},
            {"atlas_tile":"tile","atlas_index":1,"flipbook_texture":"textures/strip.png","ticks_per_frame":7}
        ]"#),
    ]);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/static")])
        .with_flipbook_destinations([("tile", [1, 1])]);
    let mut builder = builder(&view, &keys);
    builder.sources.truncate(1);
    builder.tiles.clear();
    builder.upload_bytes = builder.texture_policy.terrain_bytes(MIN_TILE, 1).unwrap();
    builder.max_layers = 2;
    let first = builder.texture_slot_for("tile", (0, 0)).unwrap();
    let second = builder.texture_slot_for("tile", (1, 0)).unwrap();
    assert_eq!(builder.sources.len(), 2);
    assert_eq!(builder.catalog.decode_attempts.get(), 1);
    assert_eq!(first.layer, second.layer);
    assert_ne!(first.animation, second.animation);
    assert_eq!(builder.overlay.animation_frames.len(), 2);
    for (slot, selector, ticks) in [(first, 0, 3), (second, 1, 7)] {
        let animation = &builder.overlay.animations[slot.animation as usize];
        assert_eq!(animation.atlas_index, selector);
        assert_eq!(animation.ticks_per_frame, ticks);
        assert_eq!(animation.frame_count, 2);
    }
}

#[test]
fn rollback_restores_shard_cost_and_cached_animation_page_bindings() {
    let small = png(16, 16);
    let strip = png(32, 64);
    let view = view(&[
        ("textures/small.png", &small), ("textures/strip.png", &strip),
        ("textures/flipbook_textures.json", br#"[
            {"atlas_tile":"tile","flipbook_texture":"textures/strip","ticks_per_frame":3},
            {"atlas_tile":"tile","atlas_tile_variant":1,"flipbook_texture":"textures/strip","ticks_per_frame":7}
        ]"#),
    ]);
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "small"), (2, "tile")])
        .with_aliases([("small", "textures/small"), ("tile", "textures/strip")])
        .with_flipbook_destinations([("tile", [2])]);
    let mut builder = builder(&view, &keys);
    builder.sources.truncate(1);
    builder.tiles.clear();
    builder.upload_bytes = builder.texture_policy.terrain_bytes(MIN_TILE, 1).unwrap();
    builder.max_layers = 2;
    builder.texture_slot_for("small", (0, 0)).unwrap();
    let checkpoint = builder.texture_checkpoint();
    let bytes = builder.upload_bytes;
    let first = builder.texture_slot_for("tile", (0, 0)).unwrap();
    let second = builder.texture_slot_for("tile", (0, 1)).unwrap();
    assert_eq!(first.page, 2);
    assert_eq!((second.page, second.layer), (first.page, first.layer));
    assert_ne!(first.animation, second.animation);
    assert!(
        builder
            .overlay
            .animation_frames
            .iter()
            .all(|frame| frame.page() == 2)
    );
    builder.rollback_textures(checkpoint);
    assert_eq!(builder.upload_bytes, bytes);
    assert_eq!(builder.tiles, [MIN_TILE]);
    assert!(builder.overlay.animation_frames.is_empty());
    assert_eq!(builder.texture_slot_for("tile", (0, 0)).unwrap().page, 2);
    let admitted_bytes = builder.upload_bytes;
    let compiled = builder.finish().unwrap();
    assert_eq!(
        admitted_bytes,
        compiled
            .overlay
            .textures
            .iter()
            .flat_map(|page| &page.mips)
            .map(|mip| mip.rgba8.len())
            .sum::<usize>()
    );
}

#[test]
fn custom_fallback_answers_decode_misses_but_keeps_admission_refusals() {
    let materials = [assets::Material::unvaried(); 2];
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/rejected")]);
    for (bytes, max_layers, expected_page) in [
        (
            b"invalid PNG".as_slice(),
            assets::MAX_TEXTURE_LAYERS,
            Some(0),
        ),
        (b"invalid PNG".as_slice(), 3, None),
    ] {
        let view = view(&[("textures/rejected.png", bytes)]);
        let mut builder = builder(&view, &keys);
        builder.fallback = Some(assets::VanillaFallback::new(&keys, &materials));
        builder.max_layers = max_layers;
        for _ in 0..2 {
            assert_eq!(
                builder.texture_slot("tile").map(|slot| slot.page),
                expected_page
            );
        }
        assert_eq!(builder.sources.len(), 3 * super::super::MAX_OVERLAY_SHARDS);
        assert_eq!(
            builder.catalog.decode_attempts.get(),
            usize::from(max_layers > 3)
        );
        assert!(builder.texture_slot_for("tile", (0, 0)).is_none());
    }
}

#[test]
fn full_arrays_distinguish_absent_pack_sources_from_present_inputs_without_decoding() {
    let materials = [assets::Material::unvaried(); 2];
    let keys = assets::material_keys::MaterialKeys::from_entries([(1, "tile")])
        .with_aliases([("tile", "textures/rejected")]);
    for (files, expected_page) in [
        (Vec::new(), Some(0)),
        (
            vec![("textures/rejected.png", b"invalid PNG".as_slice())],
            None,
        ),
        (
            vec![("textures/rejected.tga", b"invalid TGA".as_slice())],
            None,
        ),
        (
            vec![("textures/rejected.texture_set.json", b"{}".as_slice())],
            None,
        ),
    ] {
        let view = view(&files);
        let mut builder = builder(&view, &keys);
        builder.fallback = Some(assets::VanillaFallback::new(&keys, &materials));
        builder.max_layers = 3;
        assert_eq!(builder.remaining_layers(), 0);
        for _ in 0..2 {
            assert_eq!(
                builder.texture_slot("tile").map(|slot| slot.page),
                expected_page
            );
        }
        assert_eq!(builder.sources.len(), 3 * super::super::MAX_OVERLAY_SHARDS);
        assert_eq!(builder.catalog.decode_attempts.get(), 0);
        assert_eq!(
            builder.gaps.saturated_textures,
            u32::from(expected_page.is_none())
        );
    }
}

/// Builds retained pages with matching tile, source, and upload accounting.
fn occupied_builder<'a>(
    view: &'a LayeredPackView,
    layers: usize,
    tiles: &[u32],
    counts: &[usize],
) -> Builder<'a> {
    let keys = assets::material_keys::MaterialKeys::default();
    let mut result = builder(view, &keys);
    result.sources.clear();
    result.source_bytes = 0;
    result.tiles = tiles.to_vec();
    result.upload_bytes = 0;
    result.max_layers = layers;
    result.overlay.max_page_layers = layers as u32;
    for (shard, (&tile, &count)) in tiles.iter().zip(counts).enumerate() {
        result.upload_bytes += result.texture_policy.terrain_bytes(tile, count).unwrap();
        for layer in 0..count {
            let source = if tile > MIN_TILE && layer == 0 {
                let rgba8 = vec![255; (tile * tile * 4) as usize].into_boxed_slice();
                result.source_bytes += rgba8.len();
                Source::Image(DecodedTexture {
                    width: tile,
                    height: tile,
                    rgba8,
                })
            } else {
                Source::Diagnostic
            };
            result.sources.push((source, shard));
        }
    }
    result
}

/// Creates distinct original square frames whose texels identify their timeline index.
fn strip(frames: u32) -> DecodedTexture {
    DecodedTexture {
        width: MIN_TILE,
        height: MIN_TILE * frames,
        rgba8: (0..MIN_TILE * frames)
            .flat_map(|y| {
                let index = y / MIN_TILE;
                [index as u8, (index >> 8) as u8, 77, 255].repeat(MIN_TILE as usize)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

/// Supplies an ordinary authored animation that plays every physical frame in order.
fn animation_source() -> FlipbookSource {
    serde_json::from_value(serde_json::json!({"flipbook_texture":"textures/strip", "atlas_tile":"tile", "ticks_per_frame":3})).unwrap()
}

/// Checks timeline texels, page bounds, and exact upload accounting after placement.
fn assert_animation(builder: Builder<'_>, slot: TextureSlot, expected: usize) {
    let upload_bytes = builder.upload_bytes;
    let byte_budget = builder.texture_policy.storage_bytes();
    let compiled = builder.finish().unwrap();
    let animation = &compiled.overlay.animations[slot.animation as usize];
    assert_eq!(animation.frame_count as usize, expected);
    assert_eq!(animation.ticks_per_frame, 3);
    let frames = &compiled.overlay.animation_frames[animation.frame_start as usize..][..expected];
    for (index, reference) in frames.iter().enumerate() {
        let page = &compiled.overlay.textures[reference.page() as usize - 1];
        assert!(reference.layer() < page.layers);
        assert!(page.layers <= compiled.overlay.max_page_layers);
        let mip = &page.mips[0];
        let offset = reference.layer() as usize * (mip.size * mip.size * 4) as usize;
        assert_eq!(
            &mip.rgba8[offset..offset + 4],
            &[index as u8, (index >> 8) as u8, 77, 255]
        );
    }
    assert_eq!(
        upload_bytes,
        compiled
            .overlay
            .textures
            .iter()
            .flat_map(|page| &page.mips)
            .map(|mip| mip.rgba8.len())
            .sum::<usize>()
    );
    assert!(upload_bytes <= byte_budget);
}

#[test]
fn complete_flipbooks_skip_a_nearly_full_shard() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 2048, &[MIN_TILE], &[2047]);
    let source = animation_source();
    let storage = builder
        .store_frames("tile", 0, strip(32), Some(&source))
        .unwrap();
    let slot = builder.bind_frames(storage, Some(&source), (0, 0)).unwrap();
    assert_eq!(slot.page, 2);
    assert_eq!(builder.shard_len(0), 2047);
    assert_eq!(builder.shard_len(1), 32);
    assert_animation(builder, slot, 32);
}

#[test]
fn flipbooks_cross_shards_without_losing_or_reordering_frames() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 4, &[MIN_TILE], &[3]);
    let source = animation_source();
    let storage = builder
        .store_frames("tile", 0, strip(8), Some(&source))
        .unwrap();
    let slot = builder.bind_frames(storage, Some(&source), (0, 0)).unwrap();
    assert!(
        builder
            .overlay
            .animation_frames
            .iter()
            .any(|frame| frame.page() != slot.page)
    );
    assert_animation(builder, slot, 8);
}

#[test]
fn flipbooks_truncate_only_after_all_shard_capacity_is_exhausted() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 2, &[MIN_TILE; 4], &[1, 2, 2, 1]);
    let source = animation_source();
    let storage = builder
        .store_frames("tile", 0, strip(5), Some(&source))
        .unwrap();
    let slot = builder.bind_frames(storage, Some(&source), (0, 0)).unwrap();
    assert_eq!(builder.remaining_layers(), 0);
    assert_animation(builder, slot, 2);
}

#[test]
fn complete_flipbooks_retain_more_than_256_distinct_frames_when_they_fit() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 2048, &[MIN_TILE], &[1]);
    let source = animation_source();
    let storage = builder
        .store_frames("tile", 0, strip(300), Some(&source))
        .unwrap();
    let slot = builder.bind_frames(storage, Some(&source), (0, 0)).unwrap();
    assert_animation(builder, slot, 300);
}

#[test]
fn texture_growth_uses_the_cheapest_fitting_shard_instead_of_the_smallest_tile() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 2048, &[16, 32, 64, 128], &[1001, 1, 1, 1]);
    let texture = DecodedTexture {
        width: 256,
        height: 256,
        rgba8: vec![255; 256 * 256 * 4].into(),
    };
    let storage = builder.store_frames("large", 0, texture, None).unwrap();
    assert_eq!(storage.page, 4);
    assert_eq!(builder.shard_len(0), 1001);
    let upload_bytes = builder.upload_bytes;
    let compiled = builder.finish().unwrap();
    assert!(upload_bytes < 3 * 1024 * 1024);
    assert_eq!(
        upload_bytes,
        compiled
            .overlay
            .textures
            .iter()
            .flat_map(|page| &page.mips)
            .map(|mip| mip.rgba8.len())
            .sum::<usize>()
    );
}

#[test]
fn finish_rejects_sources_that_exceed_the_device_layer_limit() {
    let view = view(&[]);
    for max_layers in [2, 256, 2048, assets::MAX_TEXTURE_LAYERS] {
        assert!(
            occupied_builder(&view, max_layers, &[MIN_TILE], &[max_layers + 1])
                .finish()
                .is_none()
        );
    }
}

#[test]
fn finish_accepts_pages_above_the_former_aggregate_layer_limit() {
    let view = view(&[]);
    let per_page = assets::MAX_TEXTURE_LAYERS / MAX_OVERLAY_SHARDS + 1;
    let mut builder = occupied_builder(
        &view,
        per_page,
        &[MIN_TILE; MAX_OVERLAY_SHARDS],
        &[per_page; MAX_OVERLAY_SHARDS],
    );
    builder.texture_policy = assets::texture::policy::TexturePolicy::for_terrain(MIN_TILE, 0);
    let upload_bytes = builder.upload_bytes;
    assert!(upload_bytes <= builder.texture_policy.storage_bytes());
    let compiled = builder.finish().expect("every device page fits");
    assert_eq!(compiled.overlay.max_page_layers, per_page as u32);
    assert_eq!(
        compiled
            .overlay
            .textures
            .iter()
            .map(|page| page.layers as usize)
            .collect::<Vec<_>>(),
        vec![per_page; MAX_OVERLAY_SHARDS]
    );
    assert_eq!(
        compiled
            .overlay
            .textures
            .iter()
            .flat_map(|page| &page.mips)
            .map(|mip| mip.rgba8.len())
            .sum::<usize>(),
        upload_bytes
    );
}

#[test]
fn invalid_grid_shifts_reject_admission_without_changing_retained_pages() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 2048, &[MIN_TILE], &[1]);
    let upload_bytes = builder.upload_bytes;
    assert!(
        builder
            .store_frames("invalid", u32::BITS as u8, strip(1), None)
            .is_err()
    );
    assert_eq!(builder.upload_bytes, upload_bytes);
    assert_eq!(builder.sources.len(), 1);
}
