use super::*;

#[test]
fn a_native_texture_override_keeps_its_material_without_promoting_equipment() {
    let route = ActorArtworkLocation {
        page: 1,
        layer: 0,
        pose_mode: assets::ActorPoseMode::CompiledLiteral,
        multitexture: None,
    };
    let pages = ActorArtworkPages {
        pages: Arc::from([ActorTexturePage {
            width: 1,
            height: 1,
            layers: 1,
            rgba8: Arc::from([10, 20, 30, 3]),
            color_mask: true,
            multitexture: false,
        }]),
        source_locations: Arc::new(BTreeMap::from([(7, route)])),
        entity_locations: Arc::new(BTreeSet::from([(1, 0)])),
        routes: Arc::new(BTreeMap::from([(EntityRigId(0), route)])),
        ..Default::default()
    };
    let raster = EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([20, 30, 40, 1]),
    };
    let pages = pages.with_source_texture_overrides(&[(7, raster.clone())]);
    let location = pages.variant_location(EntityRigId(0), 7).unwrap();
    assert!(pages.pages[usize::from(location.page) - 1].color_mask);
    let (pages, locations) = pages.with_equipment_rasters(&[raster]);
    let location = locations[0].unwrap();
    assert!(!pages.pages[usize::from(location.page) - 1].color_mask);
}

#[test]
#[ignore = "requires rebuilt carriers; set CINNABAR_ENTITY_CARRIER and CINNABAR_ACTOR_CARRIER"]
fn native_color_mask_textures_have_independent_opaque_material_pages() {
    let entity_bytes = std::fs::read(std::env::var_os("CINNABAR_ENTITY_CARRIER").unwrap()).unwrap();
    let actor_bytes = std::fs::read(std::env::var_os("CINNABAR_ACTOR_CARRIER").unwrap()).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &actor_bytes,
        &assets::RuntimeEntityAssets::decode(&entity_bytes).unwrap(),
    )
    .unwrap();
    let pages = ActorArtworkPages::new(&catalog);
    let mut masks = 0;
    let mut neutral = 0;
    for (index, texture) in catalog.textures().iter().enumerate() {
        let location = pages.source_locations[&texture.source];
        let page = &pages.pages[usize::from(location.page) - 1];
        assert_eq!(page.color_mask, catalog.texture_uses_color_mask(index));
        let bytes = usize::from(page.width) * usize::from(page.height) * 4;
        let start = location.layer as usize * bytes;
        assert_eq!(&page.rgba8[start..start + bytes], texture.rgba8.as_ref());
        if page.color_mask {
            masks += 1;
        } else {
            neutral += 1;
        }
    }
    assert_eq!(masks, 2);
    assert!(
        neutral > 0,
        "neutral textures do not inherit the native material"
    );
}
