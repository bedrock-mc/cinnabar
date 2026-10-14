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
