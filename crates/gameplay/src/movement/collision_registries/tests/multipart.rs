use std::collections::BTreeSet;

use assets::RegistryRecord;
use serde_json::{Map, Value};

use super::super::multipart::second_part_offset;
use super::{BREG_V2193, active_content_registry_protocol};

/// Loads the retained registry for the active protocol.
fn records() -> Box<[RegistryRecord]> {
    assets::read_registry_for_protocol(BREG_V2193, active_content_registry_protocol()).unwrap()
}

/// Parses a registry record's canonical state fields.
fn states(record: &RegistryRecord) -> Map<String, Value> {
    serde_json::from_str(&record.canonical_state).unwrap()
}

/// Resolves the other-part offset from a registry record.
fn record_offset(record: &RegistryRecord) -> Option<[i32; 3]> {
    second_part_offset(&record.name, &states(record))
}

/// The offset of the first `name` state whose fields hold every `wanted` value.
fn offset(records: &[RegistryRecord], name: &str, wanted: &[(&str, Value)]) -> Option<[i32; 3]> {
    let record = records
        .iter()
        .find(|record| {
            let states = states(record);
            record.name.as_ref() == name
                && wanted.iter().all(|(key, value)| {
                    states.get(*key).and_then(|state| state.get("value")) == Some(value)
                })
        })
        .unwrap_or_else(|| panic!("{name} {wanted:?}"));
    second_part_offset(&record.name, &states(record))
}

/// The blocks whose native `getSecondPart` reports another block. A new block in the registry
/// that stores a half or bed state fails here until it is classified.
#[test]
fn only_blocks_with_a_native_second_part_point_at_one() {
    let records = records();
    let mut paired = BTreeSet::new();
    for record in records.iter() {
        if let Some(offset) = record_offset(record) {
            assert_eq!(
                offset.map(i32::abs).iter().sum::<i32>(),
                1,
                "{}",
                record.name
            );
            let name = record
                .name
                .strip_prefix("minecraft:")
                .unwrap_or(&record.name);
            if !name.ends_with("_door") {
                paired.insert(name.to_owned());
            }
        }
    }
    let expected = [
        "bed",
        "large_fern",
        "lilac",
        "peony",
        "pitcher_crop",
        "pitcher_plant",
        "rose_bush",
        "sunflower",
        "tall_grass",
    ];
    assert_eq!(paired, expected.map(str::to_owned).into());
    let doors = records
        .iter()
        .filter(|record| record.name.ends_with("_door"))
        .collect::<Vec<_>>();
    assert!(!doors.is_empty());
    assert!(doors.iter().all(|record| record_offset(record).is_some()));
}

#[test]
fn halves_and_ends_point_at_each_other() {
    let records = records();
    let door = "minecraft:wooden_door";
    let upper = |bit: i64| offset(&records, door, &[("upper_block_bit", bit.into())]);
    assert_eq!(upper(0), Some([0, 1, 0]));
    assert_eq!(upper(1), Some([0, -1, 0]));
    let bed = |direction: i64, head: i64| {
        offset(
            &records,
            "minecraft:bed",
            &[
                ("direction", direction.into()),
                ("head_piece_bit", head.into()),
            ],
        )
    };
    assert_eq!(
        bed(0, 0),
        Some([0, 0, 1]),
        "a south-facing bed's head is south of its foot"
    );
    assert_eq!(bed(0, 1), Some([0, 0, -1]));
    assert_eq!(bed(1, 0), Some([-1, 0, 0]));
    assert_eq!(bed(2, 0), Some([0, 0, -1]));
    assert_eq!(bed(3, 0), Some([1, 0, 0]));
}

#[test]
fn young_pitcher_crops_and_lone_halves_have_no_other_part() {
    let records = records();
    let crop = |growth: i64| {
        offset(
            &records,
            "minecraft:pitcher_crop",
            &[("growth", growth.into()), ("upper_block_bit", 0.into())],
        )
    };
    assert_eq!(
        crop(2),
        None,
        "a pitcher crop is one block tall until growth 3"
    );
    assert_eq!(crop(3), Some([0, 1, 0]));
    for name in [
        "minecraft:pale_moss_carpet",
        "minecraft:small_dripleaf_block",
    ] {
        assert_eq!(
            offset(&records, name, &[("upper_block_bit", 0.into())]),
            None
        );
    }
    assert_eq!(
        offset(
            &records,
            "minecraft:seagrass",
            &[("sea_grass_type", "double_bot".into())]
        ),
        None,
        "tall seagrass keeps the native default"
    );
}
