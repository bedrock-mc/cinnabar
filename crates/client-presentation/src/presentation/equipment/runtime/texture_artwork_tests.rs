//! Equipment artwork lookup work stays independent of the texture catalog size.

use super::*;

/// Builds a sorted catalog with unrelated textures before the selected final entry.
fn catalog(count: usize) -> Arc<RuntimeEquipmentCatalog> {
    let textures = (0..count)
        .map(|index| assets::EquipmentTexture {
            identifier: if index == count - 1 {
                "textures/fixture/selected".into()
            } else {
                format!("textures/fixture/{index:03}").into()
            },
            width: 1,
            height: 1,
            rgba8: Arc::from([index as u8, 0, 0, 255]),
        })
        .collect::<Vec<_>>();
    Arc::new(
        RuntimeEquipmentCatalog::decode(
            &assets::encode_equipment_catalog_with_textures([1; 32], [2; 32], &[], &textures)
                .unwrap(),
        )
        .unwrap(),
    )
}

/// Places startup artwork using the production map builder and isolated entity fixtures.
fn runtime(catalog: Arc<RuntimeEquipmentCatalog>) -> (EquipmentRuntime, ActorArtworkPages) {
    let (fixture, _, _) = block_fixture();
    let (runtime, pages, _) = EquipmentRuntime::build(
        fixture.assets,
        Some(catalog),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    (runtime, pages)
}

/// Resolves one frame's repeated draws, checking catalog ownership, index and placement.
fn assert_draw_work(
    runtime: &EquipmentRuntime,
    catalog: &Arc<RuntimeEquipmentCatalog>,
    index: usize,
    location: ActorArtworkLocation,
    source: (Arc<RuntimeEquipmentCatalog>, bool),
) {
    let source = super::pack::BindingSource {
        catalog: source.0,
        from_pack: source.1,
        index: 0,
        startup: None,
    };
    let (path, catalog_accesses) = assets::equipment::count_texture_slice_accesses(|| {
        catalog.textures()[index].identifier.as_ref()
    });
    assert_eq!(
        catalog_accesses, 1,
        "texture slice access must be observable"
    );
    let ((_, allocations), catalog_accesses) =
        assets::equipment::count_texture_slice_accesses(|| {
            test_alloc::count(|| {
                for _ in 0..128 {
                    let (resolved, selected, placed) =
                        std::hint::black_box(runtime.draw_artwork(path, &source)).unwrap();
                    assert!(Arc::ptr_eq(&resolved, catalog));
                    assert_eq!(selected, index);
                    assert_eq!(placed, location);
                }
            })
        });
    assert_eq!(allocations, 0, "artwork lookup must not allocate per draw");
    assert_eq!(
        catalog_accesses, 0,
        "artwork lookup must not access the catalog texture slice per draw"
    );
}

#[test]
fn startup_texture_lookup_does_not_scan_per_draw() {
    for count in [1, 64, assets::equipment::MAX_EQUIPMENT_TEXTURES] {
        let catalog = catalog(count);
        let (runtime, _) = runtime(Arc::clone(&catalog));
        let index = count - 1;
        let path = catalog.textures()[index].identifier.as_ref();
        let location = runtime.texture_location(path, false).unwrap();
        assert_draw_work(
            &runtime,
            &catalog,
            index,
            location,
            (Arc::clone(&catalog), false),
        );
    }
}

#[test]
fn pack_texture_lookup_does_not_scan_or_reuse_replaced_indices() {
    let startup = catalog(3);
    let (mut runtime, pages) = runtime(Arc::clone(&startup));
    let path = startup.textures()[1].identifier.as_ref();
    let startup_location = runtime.texture_location(path, false).unwrap();
    for count in [64, assets::equipment::MAX_EQUIPMENT_TEXTURES, 4] {
        let catalog = catalog(count);
        let (_, mut locations) = pages
            .clone()
            .with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
        let index = count - 1;
        let location = locations[index].unwrap();
        // Missing placements must not compact the catalog indices.
        locations[..index].fill(None);
        runtime.set_pack_layer(Some((
            Arc::clone(&runtime.assets),
            Some(Arc::clone(&catalog)),
            locations,
        )));
        for source in [(Arc::clone(&startup), false), (Arc::clone(&catalog), true)] {
            assert_draw_work(&runtime, &catalog, index, location, source);
        }
        assert_draw_work(
            &runtime,
            &startup,
            1,
            startup_location,
            (Arc::clone(&startup), false),
        );
        assert!(
            runtime
                .draw_artwork(
                    "textures/missing",
                    &super::pack::BindingSource {
                        catalog,
                        from_pack: true,
                        index: 0,
                        startup: None,
                    },
                )
                .is_none()
        );
    }
    runtime.set_pack_layer(None);
    let selected_location = runtime
        .texture_location(&startup.textures()[2].identifier, false)
        .unwrap();
    assert_draw_work(
        &runtime,
        &startup,
        2,
        selected_location,
        (Arc::clone(&startup), false),
    );
}
