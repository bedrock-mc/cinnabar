use std::path::Path;

use assets::RegistryRecord;
use image::{Rgba, RgbaImage};

use super::terrain::read_terrain;

/// Uses an independently admitted state identity with the supplied test properties.
fn record(properties: &str) -> RegistryRecord {
    let protocol = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(
        include_bytes!("../../../../assets/data/block-registry-v2193.bin"),
        protocol,
    )
    .unwrap();
    let mut record = records.first().unwrap().clone();
    record.name = "test:model".into();
    record.canonical_state = properties.into();
    record
}

/// Builds original texture input with unknown, singleton and bounded wheat arrays.
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let source = serde_json::json!({"texture_data": {
        "test:array": {"textures": ["textures/first", "textures/second"]},
        "test:single": {"textures": ["textures/first"]},
        "wheat": {"textures": (0..8).map(|i| format!("textures/stage_{i}")).collect::<Vec<_>>()}
    }});
    std::fs::write(
        root.path().join("terrain.json"),
        serde_json::to_vec(&source).unwrap(),
    )
    .unwrap();
    let textures = root.path().join("textures");
    std::fs::create_dir(&textures).unwrap();
    for (name, color) in [
        ("first", [40, 175, 90, 255]),
        ("second", [65, 115, 200, 255]),
    ] {
        tile(color)
            .save(textures.join(format!("{name}.png")))
            .unwrap();
    }
    for stage in 0..8 {
        tile([170 + stage * 10, 150 + stage * 8, 50, 255])
            .save(textures.join(format!("stage_{stage}.png")))
            .unwrap();
    }
    root
}

/// Creates a textured tile from original, fixture-owned colours.
fn tile(color: [u8; 4]) -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        let mut pixel = color;
        if (x / 4 + y / 4) % 2 == 0 {
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_sub(25);
            }
        }
        Rgba(pixel)
    })
}

/// Captures the production-selected unknown array beside an unchanged wheat selection.
fn capture(root: &Path, unknown: Option<(&str, u32)>, wheat: Option<(&str, u32)>) {
    let Some(output) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") else {
        return;
    };
    let mut image = RgbaImage::from_pixel(512, 256, Rgba([28, 30, 34, 255]));
    for (index, selection) in [unknown, wheat].into_iter().enumerate() {
        let texture = selection.map_or_else(
            || tile([230, 45, 190, 255]),
            |(path, _)| {
                image::open(root.join(format!("{path}.png")))
                    .unwrap()
                    .into_rgba8()
            },
        );
        let texture =
            image::imageops::resize(&texture, 192, 192, image::imageops::FilterType::Nearest);
        image::imageops::overlay(&mut image, &texture, 32 + index as i64 * 256, 32);
    }
    image
        .save(Path::new(&output).join("model-texture-selectors.png"))
        .unwrap();
}

#[test]
fn unknown_model_arrays_use_first_entry_ignoring_unrelated_properties() {
    let root = fixture();
    let terrain = read_terrain(&root.path().join("terrain.json")).unwrap();
    let unknown = terrain.get_for_model_record("test:array", &record("{}"));
    let wheat =
        terrain.get_for_model_record("wheat", &record(r#"{"growth":{"type":"int","value":7}}"#));
    capture(root.path(), unknown, wheat);
    for properties in [
        "{}",
        r#"{"growth":{"type":"int","value":3}}"#,
        r#"{"growth_stage":{"type":"int","value":3}}"#,
        r#"{"age":{"type":"int","value":3}}"#,
    ] {
        assert_eq!(
            terrain.get_for_model_record("test:array", &record(properties)),
            Some(("textures/first", 0))
        );
    }
    assert_eq!(
        terrain.get_for_model_record("test:single", &record("{}")),
        Some(("textures/first", 0))
    );
    assert_eq!(wheat, Some(("textures/stage_7", 7)));
}

#[test]
fn model_array_ranges_are_declared_and_checked() {
    let root = fixture();
    let terrain = read_terrain(&root.path().join("terrain.json")).unwrap();
    for growth in 0..8 {
        let state = record(&format!(
            r#"{{"growth":{{"type":"int","value":{growth}}}}}"#
        ));
        assert_eq!(
            terrain.get_for_model_record("wheat", &state).unwrap().1,
            growth
        );
    }
    for properties in [
        "{}",
        r#"{"growth":{"type":"int","value":-1}}"#,
        r#"{"growth":{"type":"int","value":8}}"#,
        r#"{"age":{"type":"int","value":2}}"#,
    ] {
        assert_eq!(
            terrain.get_for_model_record("wheat", &record(properties)),
            None
        );
    }
    let source = serde_json::json!({"texture_data":{"wheat":{"textures":["textures/first","textures/second"]}}});
    std::fs::write(
        root.path().join("terrain.json"),
        serde_json::to_vec(&source).unwrap(),
    )
    .unwrap();
    let terrain = read_terrain(&root.path().join("terrain.json")).unwrap();
    assert_eq!(
        terrain.get_for_model_record("wheat", &record(r#"{"growth":{"type":"int","value":1}}"#)),
        None
    );
}

#[test]
fn lightning_rod_model_arrays_keep_powered_selection_with_custom_keys() {
    let root = fixture();
    let terrain = read_terrain(&root.path().join("terrain.json")).unwrap();
    let records = assets::read_registry_for_protocol(
        include_bytes!("../../../../assets/data/block-registry-v2193.bin"),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let mut selected = std::collections::BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| crate::lightning_rod::is_record(record))
    {
        let (_, powered) = crate::lightning_rod::state(record).unwrap();
        assert_eq!(
            terrain.get_for_model_record("test:array", record),
            Some((
                if powered == 0 {
                    "textures/first"
                } else {
                    "textures/second"
                },
                powered as u32
            ))
        );
        selected.insert((record.name.as_ref(), powered));
    }
    assert_eq!(
        selected.len(),
        16,
        "every rod variant retains both powered states"
    );
}

#[test]
fn lightning_rod_model_arrays_reject_incompatible_ranges() {
    let root = fixture();
    let records = assets::read_registry_for_protocol(
        include_bytes!("../../../../assets/data/block-registry-v2193.bin"),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    for count in [1, 3] {
        let source = serde_json::json!({"texture_data":{"test:array":{"textures":vec!["textures/first"; count]}}});
        std::fs::write(
            root.path().join("terrain.json"),
            serde_json::to_vec(&source).unwrap(),
        )
        .unwrap();
        let terrain = read_terrain(&root.path().join("terrain.json")).unwrap();
        for record in records
            .iter()
            .filter(|record| crate::lightning_rod::is_record(record))
        {
            assert_eq!(terrain.get_for_model_record("test:array", record), None);
        }
    }
}

/// Each registered state of `name` in the active registry.
fn registered(name: &str) -> Vec<RegistryRecord> {
    assets::read_registry_for_protocol(
        include_bytes!("../../../../assets/data/block-registry-v2193.bin"),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .filter(|record| record.name.as_ref() == name)
    .cloned()
    .collect()
}

#[test]
fn native_block_classes_pick_their_array_entry_for_every_key() {
    let root = tempfile::tempdir().unwrap();
    let source = serde_json::json!({"texture_data": {
        "three": {"textures": ["textures/a", "textures/b", "textures/c"]},
        "two": {"textures": ["textures/a", "textures/b"]},
        "one": {"textures": "textures/a"}
    }});
    std::fs::write(
        root.path().join("terrain.json"),
        serde_json::to_vec(&source).unwrap(),
    )
    .unwrap();
    std::fs::create_dir(root.path().join("textures")).unwrap();
    for name in ["a", "b", "c"] {
        tile([90, 90, 90, 255])
            .save(root.path().join(format!("textures/{name}.png")))
            .unwrap();
    }
    let terrain = read_terrain(&root.path().join("terrain.json")).unwrap();
    let flag = |record: &RegistryRecord, property: &str| {
        u32::from(
            record
                .canonical_state
                .contains(&format!(r#""{property}":{{"type":"byte","value":1}}"#)),
        )
    };
    let mut checked = 0;
    for (name, expected) in [
        ("minecraft:carved_pumpkin", None),
        ("minecraft:lit_pumpkin", Some(1)),
        ("minecraft:pumpkin", Some(2)),
        ("minecraft:cave_vines", Some(0)),
        ("minecraft:cave_vines_body_with_berries", Some(1)),
        ("minecraft:cave_vines_head_with_berries", Some(1)),
        ("minecraft:pale_hanging_moss", None),
        ("minecraft:mangrove_propagule", None),
    ] {
        let records = registered(name);
        assert!(!records.is_empty(), "{name}");
        for record in records {
            let variant = expected.unwrap_or_else(|| match name {
                "minecraft:pale_hanging_moss" => flag(&record, "tip"),
                "minecraft:mangrove_propagule" => flag(&record, "hanging"),
                _ => 0,
            });
            let path = |index: u32| ["textures/a", "textures/b", "textures/c"][index as usize];
            for select in [
                super::terrain::TerrainTextureMap::get_for_model_record,
                super::terrain::TerrainTextureMap::get_for_record_variant,
            ] {
                assert_eq!(
                    select(&terrain, "three", &record),
                    Some((path(variant), variant)),
                    "{name} {}",
                    record.canonical_state
                );
                let short = if variant < 2 { variant } else { 0 };
                assert_eq!(select(&terrain, "two", &record), Some((path(short), short)));
                assert_eq!(select(&terrain, "one", &record), Some(("textures/a", 0)));
            }
            checked += 1;
        }
    }
    assert!(checked > 8);
}

/// Vanilla arrays of distinct entries still drawn at entry zero: some are picked by a dedicated
/// rule's own descriptors, the rest await their class's variant source. A new one fails the check.
const UNSELECTED_VANILLA_ARRAYS: &[&str] = &[
    "acacia_leaves",
    "azalea_leaves",
    "azalea_leaves_flowered",
    "barrel_top",
    "bee_nest_front",
    "beehive_front",
    "big_oak_leaves",
    "birch_leaves",
    "black_candle",
    "blue_candle",
    "brown_candle",
    "bubble_column_down_top",
    "bubble_column_mid",
    "bubble_column_outer",
    "bubble_column_up_top",
    "cake_west",
    "candle",
    "cherry_leaves",
    "chiseled_bookshelf_front",
    "chorus_flower",
    "cocoa",
    "composter_top",
    "copper_bulb",
    "crafter_east",
    "crafter_north",
    "crafter_south",
    "crafter_top",
    "crafter_west",
    "creaking_heart_side",
    "creaking_heart_top",
    "cyan_candle",
    "daylight_detector_top",
    "decorated_pot",
    "dirt",
    "dried_ghast_back",
    "dried_ghast_bottom",
    "dried_ghast_front",
    "dried_ghast_left",
    "dried_ghast_right",
    "dried_ghast_top",
    "exposed_copper_bulb",
    "farmland",
    "firefly_bush",
    "gray_candle",
    "green_candle",
    "jungle_leaves",
    "light_blue_candle",
    "light_gray_candle",
    "lime_candle",
    "magenta_candle",
    "mangrove_leaves",
    "mycelium_side",
    "nether_wart",
    "oak_leaves",
    "observer_south",
    "open_eyeblossom",
    "orange_candle",
    "orange_poplar_leaves",
    "oxidized_copper_bulb",
    "pale_oak_leaves",
    "pink_candle",
    "pink_petals",
    "pitcher_crop_lower_flower",
    "pitcher_crop_upper_flower",
    "planks",
    "pointed_dripstone_base",
    "pointed_dripstone_frustum",
    "pointed_dripstone_merge",
    "pointed_dripstone_middle",
    "pointed_dripstone_tip",
    "prismarine",
    "purple_candle",
    "reactor_core",
    "red_candle",
    "red_poplar_leaves",
    "redsandstone_bottom",
    "redsandstone_side",
    "sandstone_bottom",
    "sandstone_side",
    "sculk_catalyst_side",
    "sculk_catalyst_top",
    "sniffer_egg_bottom",
    "sniffer_egg_east",
    "sniffer_egg_north",
    "sniffer_egg_south",
    "sniffer_egg_top",
    "sniffer_egg_west",
    "spruce_leaves",
    "stone",
    "stonebrick",
    "structure_block",
    "sulfur_spike_base",
    "sulfur_spike_frustum",
    "sulfur_spike_merge",
    "sulfur_spike_middle",
    "sulfur_spike_tip",
    "sunflower_additional",
    "suspicious_gravel",
    "suspicious_sand",
    "trial_spawner_side",
    "trial_spawner_top",
    "turtle_egg",
    "vault_bottom",
    "vault_front",
    "vault_side",
    "vault_top",
    "weathered_copper_bulb",
    "white_candle",
    "wildflowers",
    "yellow_candle",
    "yellow_poplar_leaves",
];

/// Against the pinned vanilla pack, every multi-entry array a registered block draws picks its
/// entry by class or state, or is a known open family.
#[test]
fn vanilla_multi_entry_arrays_are_selected_or_known_open() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ROOT").map(std::path::PathBuf::from) else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ROOT is not set");
        return;
    };
    let pack = super::read_pack(&root).unwrap();
    let mut unselected = std::collections::BTreeSet::new();
    for record in assets::read_registry_for_protocol(
        include_bytes!("../../../../assets/data/block-registry-v2193.bin"),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    {
        for face in assets::BlockFace::ALL {
            let Some(key) = super::resolve_texture_key(&pack.blocks, record, face).key else {
                continue;
            };
            if pack.terrain.selects_entry(&key, record) == Some(false) {
                unselected.insert(key.to_string());
            }
        }
    }
    let expected = UNSELECTED_VANILLA_ARRAYS
        .iter()
        .map(|key| (*key).to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unselected, expected);
}
