use super::*;

#[test]
fn protocol_air_fallbacks_select_the_pinned_air_state() {
    let records = read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    let air = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:air")
        .unwrap();
    assert_eq!(protocol_ids::SEQUENTIAL_AIR_NETWORK_ID, air.sequential_id);
    assert_eq!(protocol_ids::HASHED_AIR_NETWORK_ID, air.network_hash);
    assert!(air.flags.contains(BlockFlags::AIR));
}

/// Decode the synthetic carrier checked against the Go encoder.
fn fixture() -> Vec<u8> {
    let hex = include_str!("../../tests/fixtures/registry-wire.hex")
        .split_whitespace()
        .collect::<String>();
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn reads_the_go_wire_contract_without_reinterpreting_fields() {
    let records = read_registry_for_protocol(&fixture(), 2193).unwrap();
    assert_eq!(records.len(), 2);
    let leaf = &records[0];
    assert_eq!(leaf.sequential_id, 7);
    assert_eq!(leaf.beacon_tint, BeaconTint::Red);
    assert_eq!(leaf.network_hash, 0x1234_5678);
    assert_eq!(leaf.name.as_ref(), "fixture:leaf");
    assert_eq!(
        leaf.canonical_state.as_ref(),
        r#"{"growth":{"type":"int","value":3}}"#
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
    assert_eq!(resin.beacon_tint, BeaconTint::NoTinting);
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
fn active_registry_rejects_duplicate_sequential_ids() {
    let mut bytes = crate::pinned_block_registry_bytes().to_vec();
    let mut reader = Reader::new(&bytes);
    reader
        .read_exact(schema::REGISTRY_MAGIC.len(), "registry magic")
        .unwrap();
    let metadata = read_registry_metadata(&mut reader).unwrap();
    assert!(metadata.canonical_states >= 2);
    let first = read_record_header(&mut reader, false).unwrap();
    for _ in 0..first.box_count {
        read_collision_box(&mut reader).unwrap();
    }
    reader
        .read_exact(usize::from(first.name_len), "first name")
        .unwrap();
    reader
        .read_exact(first.state_len as usize, "first state")
        .unwrap();
    let second_prefix = bytes.len() - reader.remaining();
    bytes[second_prefix..second_prefix + size_of::<u32>()]
        .copy_from_slice(&first.sequential_id.to_le_bytes());
    assert!(matches!(
        read_registry_for_protocol(&bytes, crate::active_content_registry_protocol()),
        Err(AssetError::DuplicateSequentialId(id)) if id == first.sequential_id
    ));
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
        (92, 0xff),
        (100, 0x80),
        (101, 0xff),
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

#[test]
fn stale_active_registry_requires_rebuilding_and_legacy_beacon_tint_stays_unresolved() {
    let mut stale = fixture();
    stale[..8].copy_from_slice(b"BREG1003");
    let error = read_registry_for_protocol(&stale, 2193).unwrap_err();
    assert!(error.to_string().contains("make assets"));
    let legacy = read_registry(include_bytes!("../../data/block-registry-v1001.bin")).unwrap();
    assert!(
        legacy
            .iter()
            .all(|record| record.beacon_tint == BeaconTint::Unresolved)
    );
}

#[test]
fn historical_registry_restores_traits_and_survival_before_admission() {
    use crate::registry::{BlockTraits as T, SurvivalRule as S};
    let records = read_registry(include_bytes!("../../data/block-registry-v1001.bin")).unwrap();
    for (name, trait_, survival) in [
        ("minecraft:fern", T::REPLACEABLE, S::Unresolved),
        ("minecraft:water", T::REPLACEABLE | T::WATER, S::None),
        (
            "minecraft:dark_oak_button",
            T::TOGGLES_BY_HAND | T::ORIENTATION_SENSITIVE,
            S::Button,
        ),
        ("minecraft:blue_carpet", T::SURVIVAL_RULE, S::Carpet),
        ("minecraft:birch_door", T::DOOR | T::MULTIPART, S::Door),
    ] {
        let variants: Vec<_> = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect();
        assert!(!variants.is_empty(), "missing legacy {name}");
        for record in variants {
            assert!(
                record.traits.contains(trait_),
                "{name} {}",
                record.canonical_state
            );
            assert_eq!(record.survival, survival, "{name}");
        }
    }
}

#[test]
fn current_empty_traits_remain_explicit() {
    let records = read_registry_for_protocol(&fixture(), 2193).unwrap();
    assert!(records[0].traits.is_empty());
}

#[test]
fn pinned_identity_projection_borrows_exact_text_with_one_allocation() {
    let bytes = crate::pinned_block_registry_bytes();
    let protocol = crate::active_content_registry_protocol();
    let (identities, allocations) =
        test_alloc::count(|| identity::read_identities(bytes, protocol).unwrap());
    assert_eq!(allocations, 1, "only the identity index may allocate");
    let records = read_registry_for_protocol(bytes, protocol).unwrap();
    assert_eq!(identities.len(), records.len());
    let byte_range = bytes.as_ptr_range();
    for (index, (identity, record)) in identities.iter().zip(records.iter()).enumerate() {
        assert_eq!(identity.sequential_id, record.sequential_id);
        assert_eq!(identity.sequential_id as usize, index);
        assert_eq!(identity.name, record.name.as_ref());
        assert_eq!(identity.canonical_state, record.canonical_state.as_ref());
        assert!(byte_range.contains(&identity.name.as_ptr()));
        assert!(byte_range.contains(&identity.canonical_state.as_ptr()));
        assert_eq!(
            crate::pinned_block_identity(record.sequential_id),
            Some((record.name.as_ref(), record.canonical_state.as_ref()))
        );
    }
    assert_eq!(crate::pinned_block_identity(u32::MAX), None);
}

/// Finds text extents through the same framing reader without assuming fixed payload offsets.
fn first_identity_text_offsets(bytes: &[u8]) -> (usize, usize) {
    let mut reader = Reader::new(bytes);
    reader
        .read_exact(schema::REGISTRY_MAGIC.len(), "magic")
        .unwrap();
    read_registry_metadata(&mut reader).unwrap();
    let header = read_record_header(&mut reader, false).unwrap();
    for _ in 0..header.box_count {
        read_collision_box(&mut reader).unwrap();
    }
    let name = bytes.len() - reader.remaining();
    (name, name + usize::from(header.name_len))
}

#[test]
fn identity_projection_checks_text_framing_without_decoding_state_facts() {
    let bytes = fixture();
    let (_, state) = first_identity_text_offsets(&bytes);
    let mut nonsemantic = bytes.clone();
    nonsemantic[state] = b'!';
    let identities = identity::read_identities(&nonsemantic, 2193).unwrap();
    assert!(identities[0].canonical_state.starts_with('!'));
    assert!(read_registry_for_protocol(&nonsemantic, 2193).is_err());
    for field in [first_identity_text_offsets(&bytes).0, state] {
        let mut malformed = bytes.clone();
        malformed[field] = 0xff;
        assert!(matches!(
            identity::read_identities(&malformed, 2193),
            Err(AssetError::InvalidRegistryUtf8 { .. })
        ));
    }
    assert!(identity::read_identities(&bytes, 0).is_err());
    assert!(identity::read_identities(&bytes[..bytes.len() - 1], 2193).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        identity::read_identities(&trailing, 2193),
        Err(AssetError::TrailingRegistryBytes { .. })
    ));
    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'?';
    assert!(matches!(
        identity::read_identities(&bad_magic, 2193),
        Err(AssetError::InvalidRegistryMagic)
    ));
}
