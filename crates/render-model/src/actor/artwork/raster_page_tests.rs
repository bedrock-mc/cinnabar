use super::*;
use std::cell::Cell;

thread_local! {
    static HASHED_BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Counts only bytes passed to the pixel digest on this test thread.
pub(super) fn record_hashed_bytes(bytes: usize) {
    HASHED_BYTES.with(|count| count.set(count.get() + bytes));
}

/// Starts a fresh work interval and returns the previous interval's hashed bytes.
fn take_hashed_bytes() -> usize {
    HASHED_BYTES.with(|count| count.replace(0))
}

/// Uses distinct source allocations and alpha values to expose layer and upload mistakes.
fn raster(fill: u8) -> EquipmentRaster {
    EquipmentRaster {
        width: 8,
        height: 8,
        rgba8: vec![fill; 8 * 8 * 4].into(),
    }
}

#[test]
fn a_new_raster_hashes_only_its_own_bytes_and_shares_unchanged_pixels() {
    let rasters = [raster(7), raster(19), raster(43)];
    let updates: Vec<_> = rasters[..2].iter().cloned().enumerate().collect();
    let (original, locations) =
        ActorArtworkPages::default().with_equipment_raster_page_updates(&updates);
    assert!(locations.iter().all(Option::is_some));
    take_hashed_bytes();
    let (next, locations) = original
        .clone()
        .with_equipment_raster_page_updates(&[(2, rasters[2].clone())]);
    let incremental = take_hashed_bytes();
    assert_eq!(incremental, rasters[2].rgba8.len());
    assert_eq!(locations[0].unwrap().page(), 3);
    for (index, raster) in rasters.iter().enumerate() {
        assert!(Arc::ptr_eq(&next.pages()[index].rgba8, &raster.rgba8));
        assert_eq!(next.pages()[index].pixels(), raster.rgba8.as_ref());
    }
    assert_ne!(original.identity(), next.identity());
    let (grouped, locations) = ActorArtworkPages::default().with_equipment_rasters(&rasters);
    assert_eq!(take_hashed_bytes(), incremental * rasters.len());
    for (raster, location) in rasters.iter().zip(locations) {
        let location = location.unwrap();
        let page = &grouped.pages()[usize::from(location.page()) - 1];
        let start = location.layer() as usize * raster.rgba8.len();
        assert_eq!(
            &page.pixels()[start..start + raster.rgba8.len()],
            raster.rgba8.as_ref()
        );
    }
}

#[test]
fn replacement_updates_only_its_owned_slot_and_invalidates_its_old_witness() {
    let (mut original, locations) = ActorArtworkPages::default()
        .with_equipment_raster_page_updates(&[(0, raster(7)), (1, raster(19))]);
    original.register_equipment_artwork([1; 32], locations[0].unwrap());
    original.register_equipment_artwork([2; 32], locations[1].unwrap());
    take_hashed_bytes();
    let replacement = raster(43);
    let (next, updated) = original
        .clone()
        .with_equipment_raster_page_updates(&[(0, replacement.clone())]);
    assert_eq!(take_hashed_bytes(), replacement.rgba8.len());
    assert_eq!(updated[0], locations[0]);
    assert_eq!(next.pages().len(), original.pages().len());
    assert!(Arc::ptr_eq(
        &next.pages()[1].rgba8,
        &original.pages()[1].rgba8
    ));
    assert!(next.equipment_artwork_location([1; 32]).is_none());
    assert_eq!(next.equipment_artwork_location([2; 32]), locations[1]);
    assert!(next.valid(crate::equipment_rig_id(0), updated[0].unwrap()));
}

#[test]
fn invalid_updates_preserve_the_original_snapshot_without_hashing() {
    let (original, _) =
        ActorArtworkPages::default().with_equipment_rasters(&[raster(7), raster(19)]);
    let mut bad = raster(43);
    bad.width = 0;
    take_hashed_bytes();
    let (next, locations) = original.clone().with_equipment_raster_page_updates(&[
        (0, raster(43)),
        (1, bad),
        (usize::MAX, raster(43)),
    ]);
    assert!(locations.iter().all(Option::is_none));
    assert!(next.shares_storage_with(&original));
    assert_eq!(take_hashed_bytes(), 0);
    let image = raster(7);
    let entity = ActorArtworkPages {
        pages: Arc::from([ActorTexturePage {
            width: image.width,
            height: image.height,
            layers: 1,
            rgba8: image.rgba8,
            color_mask: false,
            multitexture: false,
        }]),
        ..Default::default()
    };
    let (next, locations) = entity
        .clone()
        .with_equipment_raster_page_updates(&[(0, raster(7))]);
    assert_eq!(locations, [None]);
    assert!(next.shares_storage_with(&entity));
}

#[test]
fn page_slots_participate_in_the_content_identity() {
    let base = ActorArtworkPages::default();
    let (original, _) = base
        .clone()
        .with_equipment_raster_page_updates(&[(0, raster(7)), (1, raster(19))]);
    let (first, _) = original
        .clone()
        .with_equipment_raster_page_updates(&[(0, raster(43))]);
    let (second, _) = original.with_equipment_raster_page_updates(&[(1, raster(43))]);
    assert_ne!(first.identity(), second.identity());
}
