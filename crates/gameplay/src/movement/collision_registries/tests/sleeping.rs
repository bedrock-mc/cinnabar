use assets::NetworkIdMode;
use std::sync::Arc;

use super::{BREG_V2193, active_content_registry_protocol, bind, custom_block, synthetic_preg};

#[test]
fn compiled_headings_match_canonical_states_in_both_identity_spaces() {
    let protocol = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(protocol, BREG_V2193, &records),
        protocol,
    )
    .unwrap();
    let mut directions = std::collections::BTreeSet::new();
    for record in records.iter() {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state
            .get("direction")
            .and_then(|value| value.get("value").unwrap_or(value).as_i64());
        let expected =
            direction.map(|value| (90 - value.rem_euclid(4) * 90).rem_euclid(360) as f32);
        if record.name.as_ref() == "minecraft:bed" {
            directions.insert(direction.unwrap());
        }
        assert_eq!(
            registries.bed_rotation_degrees(NetworkIdMode::Sequential, record.sequential_id),
            expected,
            "{}",
            record.canonical_state
        );
        assert_eq!(
            registries.bed_rotation_degrees(NetworkIdMode::Hashed, record.network_hash),
            expected,
            "{}",
            record.canonical_state
        );
    }
    assert_eq!(directions, [0, 1, 2, 3].into());
    // Equal integers in the two identity spaces must still select different records.
    let east = records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:bed"
                && registries.bed_rotation_degrees(NetworkIdMode::Sequential, record.sequential_id)
                    == Some(180.0)
        })
        .unwrap();
    let south = records
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:bed"
                && registries.bed_rotation_degrees(NetworkIdMode::Sequential, record.sequential_id)
                    == Some(90.0)
        })
        .unwrap();
    let south_fact = registries.hashed_interaction_blocks[&south.network_hash].clone();
    Arc::make_mut(&mut registries.hashed_interaction_blocks).insert(east.sequential_id, south_fact);
    assert_eq!(
        registries.bed_rotation_degrees(NetworkIdMode::Sequential, east.sequential_id),
        Some(180.0)
    );
    assert_eq!(
        registries.bed_rotation_degrees(NetworkIdMode::Hashed, east.sequential_id),
        Some(90.0)
    );
}

#[test]
fn custom_session_replacement_never_inherits_a_compiled_heading() {
    let protocol = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, protocol).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(protocol, BREG_V2193, &records),
        protocol,
    )
    .unwrap();
    let custom = protocol::CustomBlocks {
        blocks: Arc::from([custom_block("test:sleep_fixture", 1)]),
        ..Default::default()
    };
    let hash = custom.blocks[0].hashed_states()[0].hash;
    let (range, _) = registries.begin_session_custom_blocks(&custom).unwrap();
    registries.begin_session_hashed_custom_blocks(&custom);
    for (mode, id) in [
        (NetworkIdMode::Sequential, range.start),
        (NetworkIdMode::Hashed, hash),
    ] {
        assert!(registries.block_canonical_state(mode, id).is_none());
        assert_eq!(registries.bed_rotation_degrees(mode, id), None);
    }
    registries
        .begin_session_custom_blocks(&Default::default())
        .unwrap();
    registries.begin_session_hashed_custom_blocks(&Default::default());
    assert_eq!(
        registries.bed_rotation_degrees(NetworkIdMode::Sequential, range.start),
        None
    );
    assert_eq!(
        registries.bed_rotation_degrees(NetworkIdMode::Hashed, hash),
        None
    );
}
