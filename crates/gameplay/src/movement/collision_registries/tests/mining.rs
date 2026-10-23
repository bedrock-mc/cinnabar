use super::*;
use assets::NetworkIdMode;
use protocol::CustomStateValue;

#[test]
fn native_mining_states_are_available_in_both_id_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    for axis in ["x", "y", "z"] {
        let record =
            records
                .iter()
                .find(|record| {
                    record.name.as_ref() == "minecraft:oak_log"
                        && serde_json::from_str::<serde_json::Value>(&record.canonical_state)
                            .unwrap()["pillar_axis"]["value"]
                            == axis
                })
                .unwrap();
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let states = registries.block_states(mode, id).unwrap();
            assert_eq!(
                states
                    .iter()
                    .find(|(name, _)| name.as_ref() == "pillar_axis")
                    .unwrap()
                    .1,
                CustomStateValue::String(axis.into())
            );
            assert!(!registries.block_tags_complete(mode, id));
        }
    }
}

#[test]
fn custom_mining_states_and_complete_tags_retire_in_both_id_spaces() {
    let version = active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(BREG_V2193, version).unwrap();
    let mut registries = bind(
        BREG_V2193,
        &synthetic_preg(version, BREG_V2193, &records),
        version,
    )
    .unwrap();
    let mut block = custom_block("test:states", 2);
    Arc::make_mut(&mut block.visual).state_axes = Box::new([protocol::CustomStateAxis {
        name: "test:enabled".into(),
        values: Box::new([CustomStateValue::Bool(false), CustomStateValue::Bool(true)]),
    }]);
    let hashes = block.hashed_states();
    let definitions = protocol::CustomBlocks {
        blocks: Arc::from([block]),
        ..Default::default()
    };
    let (range, _) = registries
        .begin_session_custom_blocks(&definitions)
        .unwrap();
    registries.begin_session_hashed_custom_blocks(&definitions);
    for (index, state) in hashes.iter().enumerate() {
        for (mode, id) in [
            (NetworkIdMode::Sequential, range.start + index as u32),
            (NetworkIdMode::Hashed, state.hash),
        ] {
            assert_eq!(
                registries.block_states(mode, id).unwrap(),
                [(
                    Arc::from("test:enabled"),
                    CustomStateValue::Bool(index != 0)
                )]
            );
            assert!(registries.block_tags_complete(mode, id));
        }
    }
    registries
        .begin_session_custom_blocks(&Default::default())
        .unwrap();
    registries.begin_session_hashed_custom_blocks(&Default::default());
    assert!(
        registries
            .block_states(NetworkIdMode::Sequential, range.start)
            .is_none()
    );
    assert!(
        registries
            .block_states(NetworkIdMode::Hashed, hashes[0].hash)
            .is_none()
    );
    assert!(!registries.block_tags_complete(NetworkIdMode::Sequential, range.start));
}
