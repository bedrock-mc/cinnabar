use super::*;

fn raster(identifier: &str, width: u16, height: u16) -> EquipmentTexture {
    EquipmentTexture {
        identifier: identifier.into(),
        width,
        height,
        rgba8: vec![37; usize::from(width) * usize::from(height) * 4].into(),
    }
}

#[test]
fn equipment_texture_retains_full_server_raster_dimensions() {
    let texture = raster("textures/items/custom_sword", 260, 301);
    let original = Arc::clone(&texture.rgba8);
    let catalog = RuntimeEquipmentCatalog::from_parts([1; 32], vec![], vec![texture]).unwrap();
    let texture = catalog.texture("textures/items/custom_sword").unwrap();
    assert_eq!((texture.width, texture.height), (260, 301));
    assert_eq!(texture.rgba8, original);
}

#[test]
fn equipment_texture_path_identity_survives_encoding_and_changes_with_paths() {
    let catalog = RuntimeEquipmentCatalog::from_parts(
        [1; 32],
        vec![],
        vec![
            raster("textures/items/b", 1, 1),
            raster("textures/items/a", 1, 1),
        ],
    )
    .unwrap();
    let encoded =
        encode_equipment_catalog_with_textures([1; 32], [2; 32], &[], catalog.textures()).unwrap();
    let decoded = RuntimeEquipmentCatalog::decode(&encoded).unwrap();
    assert_eq!(
        catalog.texture_paths_sha256(),
        decoded.texture_paths_sha256()
    );
    let changed = RuntimeEquipmentCatalog::from_parts(
        [1; 32],
        vec![],
        vec![
            raster("textures/items/a", 1, 1),
            raster("textures/items/c", 1, 1),
        ],
    )
    .unwrap();
    assert_eq!(
        decoded.source_manifest_sha256(),
        changed.source_manifest_sha256()
    );
    assert_ne!(
        decoded.texture_paths_sha256(),
        changed.texture_paths_sha256()
    );
}

#[test]
fn equipment_texture_large_raster_round_trips_the_cache_carrier() {
    let textures = [raster("textures/items/large_custom_sword", 2048, 1025)];
    let encoded = encode_equipment_catalog_with_textures([1; 32], [2; 32], &[], &textures)
        .expect("a bounded high resolution raster remains cacheable");
    let catalog = RuntimeEquipmentCatalog::decode(&encoded).unwrap();
    let decoded = catalog
        .texture("textures/items/large_custom_sword")
        .unwrap();
    assert_eq!((decoded.width, decoded.height), (2048, 1025));
    assert_eq!(decoded.rgba8, textures[0].rgba8);
}

#[test]
fn equipment_texture_catalog_rejects_aggregate_pixel_overflow() {
    let rgba8: Arc<[u8]> = vec![0; 1024 * 1024 * 4].into();
    let textures = (0..=crate::MAX_EQUIPMENT_PIXEL_BYTES / rgba8.len())
        .map(|index| EquipmentTexture {
            identifier: format!("textures/items/{index:03}").into(),
            width: 1024,
            height: 1024,
            rgba8: Arc::clone(&rgba8),
        })
        .collect();
    assert!(RuntimeEquipmentCatalog::from_parts([1; 32], vec![], textures).is_err());
}
