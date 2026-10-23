use super::*;
use assets::texture::policy::{MIN_TERRAIN_BYTES, TexturePolicy};

/// Builds distinct solid-colour frames at the requested terrain resolution.
fn hd_frames(side: u32, count: u32) -> DecodedTexture {
    DecodedTexture {
        width: side,
        height: side * count,
        rgba8: (0..count)
            .flat_map(|index| {
                [index as u8, (index >> 8) as u8, 77, 255].repeat((side * side) as usize)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

#[test]
fn mid_tier_admits_a_realistic_hd_pack_at_full_or_reduced_resolution() {
    let view = view(&[]);
    let tier = resource_pack::subpacks::device_memory_tier(8 << 30);
    let mut builder = occupied_builder(&view, 2048, &[MIN_TILE], &[1]);
    builder.texture_policy = TexturePolicy::for_terrain(2048, tier);
    let wanted = 600 + 20 * 32;
    for side in [128, 256, 512] {
        let plan = builder
            .frame_resolution(&vec![[side; 2]; wanted], 0)
            .expect("every valid HD frame must fit");
        assert_eq!(
            plan.placements
                .iter()
                .map(|placement| placement.count)
                .sum::<usize>(),
            wanted
        );
        assert!(
            builder.upload_bytes
                + plan
                    .placements
                    .iter()
                    .map(|placement| placement.upload_bytes)
                    .sum::<usize>()
                <= builder.texture_policy.storage_bytes()
        );
        assert_eq!(plan.side, if side == 512 { 256 } else { side });
    }
}

#[test]
fn low_tier_resizes_a_complete_animation_without_losing_frames() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 2048, &[512; 4], &[10; 4]);
    builder.texture_policy = TexturePolicy::for_terrain(2048, 0);
    let source = animation_source();
    let storage = builder
        .store_frames("tile", 0, hd_frames(512, 32), Some(&source))
        .unwrap();
    let slot = builder.bind_frames(storage, Some(&source), (0, 0)).unwrap();
    assert_eq!(
        builder.overlay.animations[slot.animation as usize].frame_count,
        32
    );
    assert_eq!(builder.gaps, Default::default());
    assert!(builder.source_bytes <= MIN_TERRAIN_BYTES);
    assert!(builder.upload_bytes <= MIN_TERRAIN_BYTES);
    assert!(
        builder
            .sources
            .iter()
            .any(|(source, _)| matches!(source, Source::Image(texture) if texture.width < 512))
    );
    assert_animation(builder, slot, 32);
}

#[test]
fn byte_pressure_can_reduce_existing_pages_and_rollback_keeps_accounting_valid() {
    let view = view(&[]);
    let mut builder = occupied_builder(&view, 2048, &[512; 4], &[11; 4]);
    builder.texture_policy = TexturePolicy::for_terrain(2048, 0);
    let checkpoint = builder.texture_checkpoint();
    let original_layers = builder.sources.len();
    let source = animation_source();
    builder
        .store_frames("tile", 0, hd_frames(512, 32), Some(&source))
        .unwrap();
    // Fill the remaining byte room with frames at the minimum terrain size. All pages
    // already have a large tile, so admitting these requires reducing a retained page.
    builder
        .store_frames("more", 0, hd_frames(MIN_TILE, 256), Some(&source))
        .unwrap();
    assert!(builder.tiles.iter().any(|&tile| tile < 512));
    builder.rollback_textures(checkpoint);
    assert_eq!(builder.sources.len(), original_layers);
    let upload_bytes = builder.upload_bytes;
    let compiled = builder.finish().unwrap();
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
    assert!(upload_bytes <= MIN_TERRAIN_BYTES);
}

#[test]
fn device_layers_are_available_independently_on_every_page() {
    let view = view(&[]);
    let layers = assets::MAX_TEXTURE_LAYERS;
    let mut builder = occupied_builder(&view, layers, &[MIN_TILE], &[1]);
    assert_eq!(
        builder.remaining_layers(),
        layers * super::super::super::MAX_OVERLAY_SHARDS - 1
    );
    let wanted = layers + 1;
    let placements = builder.place_frames(MIN_TILE, wanted);
    assert_eq!(
        placements
            .iter()
            .map(|placement| placement.count)
            .sum::<usize>(),
        wanted
    );
    assert!(
        placements
            .iter()
            .all(|placement| placement.layer as usize + placement.count <= layers)
    );
    for placement in placements {
        builder.tiles.resize(placement.shard + 1, MIN_TILE);
        builder.sources.extend(
            std::iter::repeat_with(|| (Source::Diagnostic, placement.shard)).take(placement.count),
        );
    }
    let compiled = builder
        .finish()
        .expect("all device pages must survive publication");
    assert_eq!(
        compiled
            .overlay
            .textures
            .iter()
            .map(|page| page.layers as usize)
            .sum::<usize>(),
        wanted + 1
    );
}

#[test]
fn finish_accepts_pages_above_the_terrain_storage_floor_on_higher_tiers() {
    let view = view(&[]);
    let side = 512;
    let policy = TexturePolicy::for_terrain(side, 1);
    let layer_bytes = policy.terrain_bytes(side, 1).unwrap();
    let per_page = MIN_TERRAIN_BYTES / (layer_bytes * MAX_OVERLAY_SHARDS) + 1;
    let mut builder = occupied_builder(
        &view,
        assets::MAX_TEXTURE_LAYERS,
        &[side; MAX_OVERLAY_SHARDS],
        &[per_page; MAX_OVERLAY_SHARDS],
    );
    builder.texture_policy = policy;
    let upload_bytes = builder.upload_bytes;
    assert!(upload_bytes > MIN_TERRAIN_BYTES);
    assert!(upload_bytes <= policy.storage_bytes());
    let compiled = builder.finish().expect("pages fit the higher-tier budget");
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
fn finish_rejects_pages_that_together_exceed_the_terrain_byte_budget() {
    let view = view(&[]);
    let side = 512;
    for tier in [0, 1] {
        let policy = TexturePolicy::for_terrain(side, tier);
        let layer_bytes = policy.terrain_bytes(side, 1).unwrap();
        let per_page = policy.storage_bytes() / (layer_bytes * MAX_OVERLAY_SHARDS) + 1;
        let mut builder = occupied_builder(
            &view,
            assets::MAX_TEXTURE_LAYERS,
            &[side; MAX_OVERLAY_SHARDS],
            &[per_page; MAX_OVERLAY_SHARDS],
        );
        builder.texture_policy = policy;
        assert!(per_page <= builder.max_layers);
        assert!(policy.terrain_bytes(side, per_page).is_some());
        assert!(builder.upload_bytes > policy.storage_bytes());
        assert!(builder.finish().is_none());
    }
}

/// The reporter's mixed-size five-pack stack fits the 64 MiB floor:
/// thousands of small sprites share small tiles while the HD tail keeps its
/// own pages, so no route refuses on bytes.
#[test]
fn reporter_histogram_fits_the_storage_floor() {
    let view = view(&[]);
    let keys = assets::material_keys::MaterialKeys::default();
    let mut builder = builder(&view, &keys);
    builder.texture_policy = TexturePolicy::for_terrain(2048, 0);
    builder.max_layers = 2048;
    builder.overlay.max_page_layers = 2048;
    // Measured size classes (own tile side, route count) from the report.
    let histogram = [
        (16u32, 3116usize),
        (32, 325),
        (64, 518),
        (128, 214),
        (256, 11),
    ];
    let mut admitted = 0;
    for (side, count) in histogram {
        for index in 0..count {
            let key = format!("route{side}_{index}");
            builder
                .store_frames(&key, 0, hd_frames(side, 1), None)
                .unwrap_or_else(|_| panic!("{key} refused"));
            admitted += 1;
        }
    }
    assert_eq!(admitted, 3116 + 325 + 518 + 214 + 11);
    assert!(builder.upload_bytes <= builder.texture_policy.storage_bytes());
    let compiled = builder.finish().expect("fits the floor");
    assert_eq!(compiled.gaps.missing_textures, 0);
    assert_eq!(compiled.gaps.saturated_textures, 0);
}
#[test]
fn multi_page_base_reserves_arrays_before_admitting_overlay_frames() {
    let view = view(&[]);
    let keys = assets::material_keys::MaterialKeys::default();
    let mut builder = builder(&view, &keys);
    builder.sources = vec![(Source::Diagnostic, 0)];
    builder.tiles = vec![MIN_TILE];
    builder.overlay.base_texture_pages = 2;
    builder.max_layers = 2;
    builder.overlay.max_page_layers = 2;
    builder.recount_texture_bytes();
    let capacity = builder.max_layers * builder.max_shards() - 1;
    assert_eq!(builder.remaining_layers(), capacity);
    let mut references = Vec::new();
    for index in 0..capacity {
        let storage = builder
            .store_frames(&format!("route{index}"), 0, hd_frames(MIN_TILE, 1), None)
            .unwrap();
        references.push((storage.page, storage.layer));
    }
    assert_eq!(references, [(2, 1), (3, 0), (3, 1), (4, 0), (4, 1)]);
    assert_eq!(builder.remaining_layers(), 0);
    assert!(
        builder
            .store_frames("overflow", 0, hd_frames(MIN_TILE, 1), None)
            .is_err()
    );
    let compiled = builder.finish().unwrap();
    assert_eq!(
        compiled.overlay.textures.len(),
        assets::MAX_TEXTURE_PAGES - 2
    );
    assert!(
        compiled
            .overlay
            .textures
            .iter()
            .all(|page| page.layers == 2)
    );
}
