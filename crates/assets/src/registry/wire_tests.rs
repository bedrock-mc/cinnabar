use super::*;

/// Decode the synthetic carrier checked against the Go encoder.
fn fixture() -> Vec<u8> {
    let hex = include_str!("../../tests/fixtures/registry-wire.hex")
        .split_whitespace()
        .collect::<String>();
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn reads_the_go_wire_contract_without_reinterpreting_fields() {
    let records = read_registry_for_protocol(&fixture(), 2193).unwrap();
    assert_eq!(records.len(), 2);
    let leaf = &records[0];
    assert_eq!(leaf.sequential_id, 7);
    assert_eq!(leaf.network_hash, 0x1234_5678);
    assert_eq!(leaf.name.as_ref(), "fixture:leaf");
    assert_eq!(
        leaf.canonical_state.as_ref(),
        r#"{"color":{"type":"int","value":3}}"#
    );
    assert_eq!(
        leaf.flags,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL
    );
    assert_eq!(leaf.model_family, ModelFamily::Leaves);
    assert_eq!(leaf.contributor_role, ContributorRole::Primary);
    assert_eq!(leaf.model_state.mask(), 0xff);
    for (field, value) in [
        (ModelStateField::Orientation, 1),
        (ModelStateField::Half, 2),
        (ModelStateField::Open, 3),
        (ModelStateField::Hinge, 4),
        (ModelStateField::Connections, 5),
        (ModelStateField::Growth, 6),
        (ModelStateField::LiquidDepth, 7),
        (ModelStateField::Flags, 8),
    ] {
        assert_eq!(leaf.model_state.get(field), Some(value));
    }
    assert_eq!(leaf.face_coverage, 0x3f);
    assert_eq!(leaf.provenance, RegistryProvenance::all());
    assert_eq!(leaf.collision_seed.shape_id, 0x1234);
    assert_eq!(
        leaf.collision_seed.confidence,
        CollisionConfidence::ReviewedVisibleBounds
    );
    assert_eq!(
        leaf.collision_seed.boxes.as_ref(),
        &[CollisionBox {
            min_x: -1,
            min_y: 0,
            min_z: 1,
            max_x: 100_000_000,
            max_y: 99_999_999,
            max_z: 50_000_000,
        }]
    );
    let resin = &records[1];
    assert_eq!(resin.sequential_id, 8);
    assert_eq!(resin.network_hash, 0x8765_4321);
    assert_eq!(resin.name.as_ref(), "fixture:resin");
    assert_eq!(resin.canonical_state.as_ref(), "{}");
    assert_eq!(resin.model_family, ModelFamily::ResinClump);
    assert_eq!(resin.contributor_role, ContributorRole::LiquidAdditional);
    assert_eq!(resin.model_state.get(ModelStateField::Orientation), None);
    assert_eq!(resin.model_state.get(ModelStateField::Flags), Some(0xfeed));
    assert!(resin.collision_seed.boxes.is_empty());
}

#[test]
fn rejects_reserved_discriminants_bits_and_inconsistent_lengths() {
    // Fixture offsets are independent of the generated decoder.
    for (offset, value) in [
        (44, 0x80),
        (45, 0xff),
        (46, 0xff),
        (49, 0xff),
        (50, 0x80),
        (51, 8),
    ] {
        let mut bytes = fixture();
        bytes[offset] = value;
        assert!(
            read_registry_for_protocol(&bytes, 2193).is_err(),
            "offset {offset}"
        );
    }
    let mut bytes = fixture();
    bytes[56..60].copy_from_slice(&1_048_577u32.to_le_bytes());
    assert!(matches!(
        read_registry_for_protocol(&bytes, 2193),
        Err(AssetError::RegistryStateTooLarge { .. })
    ));
    let bytes = fixture();
    assert!(read_registry_for_protocol(&bytes[..bytes.len() - 1], 2193).is_err());
    let mut bytes = fixture();
    bytes.push(0);
    assert!(matches!(
        read_registry_for_protocol(&bytes, 2193),
        Err(AssetError::TrailingRegistryBytes { .. })
    ));
}

#[test]
fn accepts_compiled_enrichment_flags_without_using_source_only_mask() {
    let mut bytes = fixture();
    bytes[44] |= BlockFlags::SEASONAL_REPLACEABLE.bits()
        | BlockFlags::FIRE_FLAMMABLE.bits()
        | BlockFlags::FIRE_TOP_SUPPORT.bits();
    let records = read_registry_for_protocol(&bytes, 2193).unwrap();
    assert!(records[0].flags.contains(BlockFlags::SEASONAL_REPLACEABLE));
    assert!(records[0].flags.contains(BlockFlags::FIRE_FLAMMABLE));
    assert!(records[0].flags.contains(BlockFlags::FIRE_TOP_SUPPORT));
}
