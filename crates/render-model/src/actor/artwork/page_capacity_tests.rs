use super::page_budget::{MAX_ACTOR_PAGE_BYTES, MAX_ACTOR_PAGE_LAYERS, page_layers, push_page};
use super::*;

#[test]
fn equipment_appends_and_page_updates_keep_actor_pixels_reusable() {
    let texture = assets::ActorTexture {
        source: 0,
        width: 2,
        height: 2,
        pixel_sha256: Sha256::digest([79; 16]).into(),
        rgba8: Arc::from([79; 16]),
    };
    let first = ActorArtworkPages::default().with_pack_artwork(std::slice::from_ref(&texture), &[]);
    let raster = EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([101; 4]),
    };
    let (first, _) = first.with_equipment_rasters(&[raster]);
    let replacement = EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([103; 4]),
    };
    let (first, locations) = first.with_equipment_raster_page_updates(&[(1, replacement)]);
    assert!(locations[0].is_some());
    let second = ActorArtworkPages::default().with_pack_artwork(&[texture], &[]);
    assert!(Arc::ptr_eq(
        &first.pages()[0].rgba8,
        &second.pages()[0].rgba8
    ));
}

#[test]
fn retiring_the_newest_collection_keeps_older_live_pixels_reusable() {
    let texture = assets::ActorTexture {
        source: 0,
        width: 2,
        height: 2,
        pixel_sha256: Sha256::digest([89; 16]).into(),
        rgba8: Arc::from([89; 16]),
    };
    let first = ActorArtworkPages::default().with_pack_artwork(std::slice::from_ref(&texture), &[]);
    let newer = ActorArtworkPages::default().with_pack_artwork(std::slice::from_ref(&texture), &[]);
    drop(newer);
    let last = ActorArtworkPages::default().with_pack_artwork(&[texture], &[]);
    assert!(Arc::ptr_eq(&first.pages()[0].rgba8, &last.pages()[0].rgba8));
}

#[test]
fn recompiled_actor_pages_share_pixels_and_release_them_after_the_last_owner() {
    let texture = assets::ActorTexture {
        source: 0,
        width: 2,
        height: 2,
        pixel_sha256: Sha256::digest([73; 16]).into(),
        rgba8: Arc::from([73; 16]),
    };
    let first = ActorArtworkPages::default().with_pack_artwork(std::slice::from_ref(&texture), &[]);
    let base = ActorArtworkPages {
        identity: [19; 32],
        ..Default::default()
    };
    let second = base.with_pack_artwork(&[texture], &[]);
    let pixels = Arc::downgrade(&first.pages()[0].rgba8);
    assert!(Arc::ptr_eq(
        &first.pages()[0].rgba8,
        &second.pages()[0].rgba8
    ));
    drop(first);
    assert!(pixels.upgrade().is_some());
    drop(second);
    assert!(
        pixels.upgrade().is_none(),
        "the page cache must not retain pixels"
    );
}

#[test]
fn a_pack_texture_change_or_layer_reorder_keeps_its_own_pixels() {
    let textures = [83, 149].map(|value| assets::ActorTexture {
        source: u32::from(value),
        width: 1,
        height: 1,
        pixel_sha256: Sha256::digest([value; 4]).into(),
        rgba8: Arc::from([value; 4]),
    });
    let first = ActorArtworkPages::default().with_pack_artwork(&textures, &[]);
    let rasters = textures
        .iter()
        .map(|texture| EquipmentRaster {
            width: texture.width,
            height: texture.height,
            rgba8: Arc::clone(&texture.rgba8),
        })
        .collect::<Vec<_>>();
    let (equipment, _) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
    assert!(Arc::ptr_eq(
        &first.pages()[0].rgba8,
        &equipment.pages()[0].rgba8
    ));
    let mut reversed = textures;
    reversed.reverse();
    let changed = ActorArtworkPages::default().with_pack_artwork(&reversed, &[]);
    assert_eq!(
        first.pages()[0].pixels(),
        &[83, 83, 83, 83, 149, 149, 149, 149]
    );
    assert_eq!(
        changed.pages()[0].pixels(),
        &[149, 149, 149, 149, 83, 83, 83, 83]
    );
    assert!(!Arc::ptr_eq(
        &first.pages()[0].rgba8,
        &changed.pages()[0].rgba8
    ));
}

#[test]
fn pack_artwork_retains_more_than_byte_sized_distinct_texture_pages() {
    let textures = (0..usize::from(u8::MAX) + 8)
        .map(|index| {
            let height = u16::try_from(index + 1).unwrap();
            let pixel = [index as u8, (index >> 8) as u8, 7, 255];
            let rgba8 = pixel.repeat(usize::from(height));
            assets::ActorTexture {
                source: index as u32,
                width: 1,
                height,
                pixel_sha256: Sha256::digest(&rgba8).into(),
                rgba8: rgba8.into(),
            }
        })
        .collect::<Vec<_>>();
    let bindings = textures
        .iter()
        .enumerate()
        .map(|(index, _)| assets::ActorArtworkBinding {
            rig: index as u32,
            geometry_candidate: index as u32,
            entity_symbol: index as u32,
            geometry: index as u32,
            render_controller: 0,
            texture: index as u32,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        })
        .collect::<Vec<_>>();
    let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &bindings);
    assert_eq!(pages.rejected_bindings(), 0);
    assert_eq!(pages.pages().len(), textures.len());
    for (index, texture) in textures.iter().enumerate() {
        let rig = crate::pack_rig_id(index as u32);
        let location = pages
            .route(rig)
            .expect("valid pack binding retains its page");
        assert!(pages.valid(rig, location));
        assert_eq!(pages.variant_location(rig, index as u32), Some(location));
        let page = &pages.pages()[usize::from(location.page()) - 1];
        assert_eq!(page.dimensions(), (texture.width, texture.height));
        assert_eq!(page.pixels(), texture.rgba8.as_ref());
    }
}

#[test]
fn a_single_layer_pack_page_shares_its_source_allocation() {
    let pixels: Arc<[u8]> = (0..68).map(|byte| byte as u8).collect::<Vec<_>>().into();
    let texture = assets::ActorTexture {
        source: 0,
        width: 17,
        height: 1,
        pixel_sha256: Sha256::digest(&pixels).into(),
        rgba8: pixels,
    };
    let pages = ActorArtworkPages::default().with_pack_artwork(std::slice::from_ref(&texture), &[]);
    assert_eq!(pages.pages().len(), 1);
    let page = &pages.pages()[0];
    assert!(Arc::ptr_eq(&page.rgba8, &texture.rgba8));
    assert_eq!(page.layers, 1);
    assert_eq!(page.pixels(), texture.rgba8.as_ref());
}

#[test]
fn a_single_layer_equipment_page_shares_its_source_allocation() {
    let raster = EquipmentRaster {
        width: 19,
        height: 1,
        rgba8: (11..87).map(|byte| byte as u8).collect::<Vec<_>>().into(),
    };
    let (pages, locations) =
        ActorArtworkPages::default().with_equipment_rasters(std::slice::from_ref(&raster));
    let location = locations[0].unwrap();
    let page = &pages.pages()[usize::from(location.page()) - 1];
    assert!(Arc::ptr_eq(&page.rgba8, &raster.rgba8));
    assert_eq!(page.layers, 1);
    assert_eq!(page.pixels(), raster.rgba8.as_ref());
}

// A pack with a texture size per page past the old 32-page cap places every texture.
#[test]
fn pack_art_of_many_sizes_gets_a_page_per_size() {
    let textures: Vec<_> = (1..=40u16)
        .map(|side| assets::ActorTexture {
            source: u32::from(side),
            width: side,
            height: 1,
            pixel_sha256: [0; 32],
            rgba8: vec![9; usize::from(side) * 4].into(),
        })
        .collect();
    let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &[]);
    assert_eq!(pages.pages().len(), 40);
}

// Same-size equipment rasters past one page's layer limit spill onto further pages.
#[test]
fn equipment_rasters_past_one_page_spill_onto_more_pages() {
    let raster = EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: vec![9; 4].into(),
    };
    let rasters = vec![raster; MAX_ACTOR_PAGE_LAYERS + 3];
    let (pages, locations) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
    assert_eq!(pages.pages().len(), 2);
    assert!(locations.iter().all(Option::is_some));
}

// Past the byte budget a page is downscaled to fit rather than dropped.
#[test]
fn a_page_past_the_byte_budget_is_downscaled_not_dropped() {
    let page = ActorTexturePage {
        width: 16,
        height: 16,
        layers: 1,
        rgba8: vec![9; 16 * 16 * 4].into(),
        color_mask: false,
        multitexture: false,
    };
    let mut pages = Vec::new();
    let mut gpu_bytes = MAX_ACTOR_GPU_PIXEL_BYTES - 16 * 16;
    assert_eq!(push_page(&mut pages, &mut gpu_bytes, page), Some(1));
    assert_eq!(pages[0].dimensions(), (8, 8));
    assert_eq!(gpu_bytes, MAX_ACTOR_GPU_PIXEL_BYTES);
}

// Same-size pack textures over the page byte cap become several bounded GPU allocations.
#[test]
fn large_same_size_textures_split_into_pages_within_the_byte_cap() {
    let (width, height) = (4000u16, 1280u16);
    let layer = usize::from(width) * usize::from(height) * 4;
    let textures: Vec<_> = (0..3u8)
        .map(|value| {
            let rgba8: Arc<[u8]> = vec![value; layer].into();
            assets::ActorTexture {
                source: u32::from(value),
                width,
                height,
                pixel_sha256: Sha256::digest(&rgba8).into(),
                rgba8,
            }
        })
        .collect();
    let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &[]);
    assert_eq!(pages.pages().len(), 3);
    assert!(
        pages
            .pages()
            .iter()
            .all(|page| page.rgba8.len() <= MAX_ACTOR_PAGE_BYTES)
    );
    assert_eq!(page_layers(128, 128), MAX_ACTOR_PAGE_LAYERS);
    assert_eq!(page_layers(u16::MAX, u16::MAX), 1);
}

// A group over the remaining budget shares one downscale, so every member keeps a location.
#[test]
fn a_split_group_downscales_together_so_every_member_is_admitted() {
    let side = 1024u16;
    let layer = usize::from(side) * usize::from(side) * 4;
    let members: Vec<usize> = (0..9).collect();
    let mut pages = Vec::new();
    let mut gpu_bytes = MAX_ACTOR_GPU_PIXEL_BYTES - 32 * 1024 * 1024;
    let placed = push_group(
        &mut pages,
        &mut gpu_bytes,
        &members,
        [side, side],
        false,
        |chunk| ActorTexturePage {
            width: side,
            height: side,
            layers: chunk.len() as u32,
            rgba8: vec![9; layer * chunk.len()].into(),
            color_mask: false,
            multitexture: false,
        },
    );
    assert_eq!(
        placed.iter().map(|(chunk, _)| chunk.len()).sum::<usize>(),
        9
    );
    assert!(placed.iter().all(|(_, page)| page.is_some()));
    assert!(pages.iter().all(|page| page.dimensions() == (512, 512)));
    assert!(gpu_bytes <= MAX_ACTOR_GPU_PIXEL_BYTES);
}
