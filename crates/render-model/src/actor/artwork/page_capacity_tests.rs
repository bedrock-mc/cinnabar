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
