use super::*;

#[test]
fn crafter_plain_and_typed_bits_keep_unknown_distinct_from_false() {
    for (json, expected) in [
        (r#"{"triggered_bit":false}"#, Some(false)),
        (r#"{"triggered_bit":{"type":"byte","value":1}}"#, Some(true)),
        (r#"{"triggered_bit":{"value":0}}"#, Some(false)),
        (r#"{"triggered_bit":true}"#, Some(true)),
        (r#"{"triggered_bit":"false"}"#, None),
        (r#"{"triggered_bit":-1}"#, None),
        ("{}", None),
        ("malformed", None),
    ] {
        let state = BlockPresentationState::decode("minecraft:crafter", json);
        assert_eq!(state.crafter_triggered, expected, "{json}");
        assert_eq!(state.portal_axis, None);
    }
}

#[test]
fn admitted_registry_covers_every_portal_and_crafter_state_in_both_id_spaces() {
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    let admitted = BlockPresentationStates::from_records(&records);
    let mut portal_count = 0;
    let mut crafter_count = 0;
    for record in records.iter() {
        let source: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let expected = match record.name.as_ref() {
            crate::NETHER_PORTAL_IDENTIFIER => {
                portal_count += 1;
                BlockPresentationState {
                    portal_axis: Some(match source["portal_axis"]["value"].as_str().unwrap() {
                        "x" => PortalAxis::X,
                        "z" => PortalAxis::Z,
                        _ => PortalAxis::Unknown,
                    }),
                    crafter_triggered: None,
                }
            }
            "minecraft:crafter" => {
                crafter_count += 1;
                BlockPresentationState {
                    crafter_triggered: Some(
                        source["triggered_bit"]["value"].as_u64().unwrap() != 0,
                    ),
                    portal_axis: None,
                }
            }
            _ => continue,
        };
        assert_eq!(
            admitted.get(NetworkIdMode::Sequential, record.sequential_id),
            expected
        );
        assert_eq!(
            admitted.get(NetworkIdMode::Hashed, record.network_hash),
            expected
        );
    }
    assert!(portal_count > 0 && crafter_count > 0);
}

#[test]
fn replacing_a_registry_replaces_its_typed_facts_without_reusing_old_ids() {
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    let mut record = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:crafter")
        .unwrap()
        .clone();
    record.canonical_state = r#"{"triggered_bit":true}"#.into();
    let first = BlockPresentationStates::from_records(&[record.clone()]);
    record.canonical_state = r#"{"triggered_bit":false}"#.into();
    let second = BlockPresentationStates::from_records(&[record.clone()]);
    assert_eq!(
        first
            .get(NetworkIdMode::Sequential, record.sequential_id)
            .crafter_triggered,
        Some(true)
    );
    assert_eq!(
        second
            .get(NetworkIdMode::Sequential, record.sequential_id)
            .crafter_triggered,
        Some(false)
    );
    let old_id = record.sequential_id;
    record.sequential_id = u32::MAX;
    let third = BlockPresentationStates::from_records(&[record]);
    assert_eq!(
        third
            .get(NetworkIdMode::Sequential, old_id)
            .crafter_triggered,
        None
    );
    assert_eq!(
        third
            .get(NetworkIdMode::Sequential, u32::MAX)
            .crafter_triggered,
        Some(false)
    );
}

#[test]
fn admitted_portal_axis_lookup_allocates_nothing() {
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    let record = records
        .iter()
        .find(|record| record.name.as_ref() == crate::NETHER_PORTAL_IDENTIFIER)
        .unwrap();
    let states = crate::pinned_block_presentation_states();
    let (old, old_allocations) = super::test_allocations::measure(|| {
        serde_json::from_str::<serde_json::Value>(&record.canonical_state).unwrap()
    });
    let (state, new_allocations) = super::test_allocations::measure(|| {
        states.get(crate::NetworkIdMode::Sequential, record.sequential_id)
    });
    assert!(state.portal_axis.is_some());
    assert!(old.get("portal_axis").is_some());
    assert_eq!(new_allocations, 0);
    println!(
        "portal state allocations: parse={old_allocations}, admitted lookup={new_allocations}"
    );
}
