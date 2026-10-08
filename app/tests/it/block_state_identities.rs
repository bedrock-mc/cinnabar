use protocol::CustomStateValue;

#[test]
fn block_state_hashes_match_every_pinned_typed_block_state() {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    for record in records.iter().filter(|record| {
        record.name.starts_with("minecraft:") && record.name.as_ref() != "minecraft:unknown"
    }) {
        let raw: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let states = raw
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, scalar)| {
                let value = match scalar["type"].as_str().unwrap() {
                    "byte" => match scalar["value"].as_i64().unwrap() {
                        0 => CustomStateValue::Bool(false),
                        1 => CustomStateValue::Bool(true),
                        value => panic!("unsupported byte block state {value}"),
                    },
                    "int" => CustomStateValue::Int(scalar["value"].as_i64().unwrap()),
                    "string" => CustomStateValue::String(scalar["value"].as_str().unwrap().into()),
                    kind => panic!("unsupported registry state type {kind}"),
                };
                (key.as_str(), value)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            protocol::block_state_network_hash(
                &record.name,
                states.iter().map(|(key, value)| (*key, value)),
            ),
            record.network_hash,
            "{} {}",
            record.name,
            record.canonical_state
        );
    }
}
