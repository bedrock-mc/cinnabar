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
