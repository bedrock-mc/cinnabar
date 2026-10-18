use super::*;
use assets::material_keys::MaterialTextureSource;

#[test]
fn inherited_material_tints_split_layers_until_the_winning_catalog_replaces_them() {
    let (runtime, _) = tests::shared_carrier();
    let keys = MaterialKeys::from_entries([(1, "fixture"), (2, "fixture")])
        .with_aliases([("fixture", "textures/blocks/fixture")])
        .with_sources(
            [],
            [(1, None), (2, Some([128, 64, 32]))].map(|(id, baked_tint)| {
                (
                    id,
                    MaterialTextureSource {
                        key: "fixture".into(),
                        variant: 0,
                        path: "textures/blocks/fixture".into(),
                        raster: "textures/blocks/fixture.png".into(),
                        baked_tint,
                    },
                )
            }),
            [],
        );
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
    ImageBuffer::from_pixel(2, 2, Rgba([200u8, 160, 120, 255]))
        .save(root.path().join("textures/blocks/fixture.png"))
        .unwrap();
    for (catalog, expected) in [
        (None, [[200, 160, 120, 255], [100, 40, 15, 255]]),
        (
            Some(
                r##"{"texture_data":{"fixture":{"textures":{"path":"textures/blocks/fixture","tint_color":"#4080ff"}}}}"##,
            ),
            [[50, 80, 120, 255]; 2],
        ),
        (
            Some(r#"{"texture_data":{"fixture":{"textures":"textures/blocks/fixture"}}}"#),
            [[200, 160, 120, 255]; 2],
        ),
    ] {
        if let Some(catalog) = catalog {
            std::fs::write(root.path().join("textures/terrain_texture.json"), catalog).unwrap();
        }
        let pack = PbrPack::new(root.path(), None).with_terrain_aliases(keys.aliases());
        let (payload, _) = load(sources(&runtime, &keys), &[pack]);
        let maps = payload.unwrap().into_assets().unwrap();
        for (id, expected) in (1..=2).zip(expected) {
            assert_eq!(tests::albedo(&maps, &runtime, id), Some(expected));
        }
        assert_eq!(
            maps.authored_layer_count(),
            if catalog.is_none() { 2 } else { 1 }
        );
    }
    // Carrier source metadata must participate even when the pack files are identical.
    let pack = PbrPack::new(root.path(), None);
    let plain = keys.clone().with_sources(
        [],
        keys.material_sources().map(|(id, source)| {
            let mut source = source.clone();
            source.baked_tint = None;
            (id, source)
        }),
        [],
    );
    assert_ne!(
        cache::fingerprint(std::slice::from_ref(&pack), &sources(&runtime, &keys)),
        cache::fingerprint(std::slice::from_ref(&pack), &sources(&runtime, &plain))
    );
}
