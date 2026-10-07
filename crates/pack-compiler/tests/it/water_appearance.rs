//! Water render attributes retain their alpha when a biome only replaces RGB.
use std::{fs, path::Path};

use assets::{BiomeRegistryRecord, TINT_MAP_SIZE, TintMapId};
use image::{Rgb, RgbImage};
use pack_compiler::compile_biome_assets;

fn fixture(root: &Path, appearance: &str) {
    let resource = root.join("resource_pack");
    fs::create_dir_all(resource.join("biomes")).unwrap();
    fs::create_dir_all(resource.join("textures/colormap")).unwrap();
    fs::create_dir_all(root.join("behavior_pack/biomes")).unwrap();
    fs::write(
        resource.join("biomes/frozen_river.client_biome.json"),
        format!(
            r#"{{"minecraft:client_biome":{{"description":{{"identifier":"minecraft:frozen_river"}},"components":{appearance}}}}}"#
        ),
    )
    .unwrap();
    fs::write(
        root.join("behavior_pack/biomes/frozen_river.biome.json"),
        r#"{"minecraft:biome":{"description":{"identifier":"minecraft:frozen_river"},"components":{"minecraft:climate":{"temperature":0.0,"downfall":0.5}}}}"#,
    )
    .unwrap();
    for map in TintMapId::ALL {
        RgbImage::from_pixel(TINT_MAP_SIZE, TINT_MAP_SIZE, Rgb([100, 150, 100]))
            .save(resource.join(format!("textures/colormap/{}.png", map.source_name())))
            .unwrap();
    }
}

#[test]
fn missing_water_opacity_keeps_native_alpha_not_opaque() {
    // Vanilla water colour reads the default RGBA.
    // Frozen river authors only RGB, exactly as in the reported ocean scene.
    for (appearance, expected) in [
        ("{}", 166.0 / 255.0),
        (
            r##"{"minecraft:water_appearance":{"surface_color":"#185390"}}"##,
            165.0 / 255.0,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        fixture(directory.path(), appearance);
        let compiled = compile_biome_assets(
            &directory.path().join("resource_pack"),
            &directory.path().join("behavior_pack"),
            &[BiomeRegistryRecord {
                id: 7,
                name: "minecraft:frozen_river".into(),
            }],
        )
        .unwrap();
        let resolved = compiled.resolve_live(&[]).unwrap();
        assert_eq!(compiled.rules[0].water_opacity(), expected);
        assert_eq!(resolved.records[1].water[3], expected);
        assert_eq!(resolved.records[0].water[3], 166.0 / 255.0);
    }
}

#[test]
fn authored_water_opacity_overrides_native_default_including_endpoints() {
    for (opacity, expected) in [(0.0, 0.0), (0.65, 165.0 / 255.0), (1.0, 1.0)] {
        let directory = tempfile::tempdir().unwrap();
        fixture(
            directory.path(),
            &format!(
                r##"{{"minecraft:water_appearance":{{"surface_color":"#185390","surface_opacity":{opacity}}}}}"##
            ),
        );
        let compiled = compile_biome_assets(
            &directory.path().join("resource_pack"),
            &directory.path().join("behavior_pack"),
            &[BiomeRegistryRecord {
                id: 7,
                name: "minecraft:frozen_river".into(),
            }],
        )
        .unwrap();
        assert_eq!(compiled.rules[0].water_opacity(), expected);
    }
}
