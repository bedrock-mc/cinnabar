//! Retained equipment sources survive page appends and reject changed public page data.

use super::*;

#[test]
fn retained_equipment_bindings_keep_their_pixel_and_layout_witness() {
    let raster = EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([10, 20, 30, 255]),
    };
    let (mut pages, locations) =
        ActorArtworkPages::default().with_equipment_rasters(std::slice::from_ref(&raster));
    let location = locations[0].unwrap();
    let identity = [1; 32];
    pages.register_equipment_artwork(identity, location);
    assert_eq!(pages.equipment_artwork_location(identity), Some(location));
    assert!(pages.shares_storage_with(&pages.clone()));
    let (mut appended, _) = pages.clone().with_equipment_rasters(&[raster]);
    assert_eq!(
        appended.equipment_artwork_location(identity),
        Some(location)
    );
    // Changing a public page must invalidate the retained source, even at the same location.
    let page = &mut Arc::make_mut(&mut appended.pages)[usize::from(location.page) - 1];
    Arc::make_mut(&mut page.rgba8)[0] = 99;
    assert!(appended.equipment_artwork_location(identity).is_none());
    assert_eq!(pages.equipment_artwork_location(identity), Some(location));
    let invalid = ActorArtworkLocation {
        page: 0,
        ..location
    };
    pages.register_equipment_artwork([2; 32], invalid);
    assert!(pages.equipment_artwork_location([2; 32]).is_none());
}
