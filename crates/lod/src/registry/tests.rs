use super::*;

fn custom(name: &str, states: &str) -> MaterialKey {
    MaterialKey::Custom(CustomStateKey::new(name, states).expect("valid custom key"))
}

fn sample_registry() -> MaterialRegistry {
    let mut registry = MaterialRegistry::new();
    for key in [
        MaterialKey::Network(0x1234_5678),
        custom("cinnabar:lamp", "lit=true"),
        MaterialKey::Network(0),
        custom("cinnabar:lamp", "lit=false"),
        custom("cinnabar:plain", ""),
        MaterialKey::Network(u32::MAX),
    ] {
        registry.get_or_insert(&key).expect("room for keys");
    }
    registry
}

#[test]
fn ids_are_dense_stable_and_never_reused() {
    let mut registry = MaterialRegistry::new();
    let stone = MaterialKey::Network(7);
    let lamp = custom("cinnabar:lamp", "lit=true");
    assert_eq!(registry.get_or_insert(&stone), Ok(MaterialId::FIRST_BLOCK));
    let lamp_id = registry.get_or_insert(&lamp).expect("room");
    assert_eq!(lamp_id.raw(), MaterialId::FIRST_BLOCK.raw() + 1);
    assert_eq!(registry.get_or_insert(&stone), Ok(MaterialId::FIRST_BLOCK));
    assert_eq!(registry.len(), 2);
    assert_eq!(registry.get(&lamp), Some(lamp_id));
    assert_eq!(registry.key(lamp_id), Some(&lamp));
    assert_eq!(registry.key(MaterialId::AIR), None);
    assert_eq!(registry.key(MaterialId::HIDDEN), None);
    let ids: Vec<_> = registry.iter().map(|(id, _)| id).collect();
    assert_eq!(ids, [MaterialId::FIRST_BLOCK, lamp_id]);
}

#[test]
fn custom_states_never_alias_network_hashes_or_other_states() {
    let mut registry = MaterialRegistry::new();
    let network = registry
        .get_or_insert(&MaterialKey::Network(1))
        .expect("room");
    let lit = registry
        .get_or_insert(&custom("a:b", "lit=true"))
        .expect("room");
    let unlit = registry
        .get_or_insert(&custom("a:b", "lit=false"))
        .expect("room");
    let other_name = registry
        .get_or_insert(&custom("a:c", "lit=true"))
        .expect("room");
    let ids = [network, lit, unlit, other_name];
    for (index, id) in ids.iter().enumerate() {
        assert!(!ids[index + 1..].contains(id));
    }
}

#[test]
fn invalid_custom_keys_are_rejected() {
    assert_eq!(
        CustomStateKey::new("", "x=1"),
        Err(RegistryError::EmptyName)
    );
    let long = "s".repeat(MAX_KEY_TEXT_BYTES + 1);
    assert_eq!(
        CustomStateKey::new("a:b", &long),
        Err(RegistryError::TextLength(long.len()))
    );
    assert!(CustomStateKey::new("a:b", &long[1..]).is_ok());
}

#[test]
fn the_id_space_ends_before_hidden() {
    let mut registry = MaterialRegistry::new();
    for hash in 0..MAX_MATERIALS as u32 {
        registry.push(MaterialKey::Network(hash)).expect("room");
    }
    let last = registry.iter().last().map(|(id, _)| id);
    assert_eq!(last, Some(MaterialId::LAST_BLOCK));
    assert_eq!(
        registry.get_or_insert(&MaterialKey::Network(u32::MAX)),
        Err(RegistryError::Full)
    );
    assert_eq!(
        registry.get_or_insert(&MaterialKey::Network(3)),
        Ok(MaterialId::new(4).expect("id"))
    );
}

#[test]
fn encoding_round_trips_and_appends_keep_earlier_ids() {
    let mut registry = sample_registry();
    let decoded = MaterialRegistry::decode(&registry.encode()).expect("valid registry");
    assert_eq!(decoded, registry);
    for (id, key) in registry.iter() {
        assert_eq!(decoded.get(key), Some(id));
    }
    let added = registry
        .get_or_insert(&custom("cinnabar:new", "age=2"))
        .expect("room");
    let reloaded = MaterialRegistry::decode(&registry.encode()).expect("valid registry");
    assert_eq!(reloaded.key(added), registry.key(added));
    for (id, key) in decoded.iter() {
        assert_eq!(reloaded.get(key), Some(id));
    }
    let empty = MaterialRegistry::new();
    assert_eq!(MaterialRegistry::decode(&empty.encode()), Ok(empty));
}

/// Re-signs `bytes` after a deliberate edit so the structural checks are reached.
fn resigned(mut bytes: Vec<u8>) -> Vec<u8> {
    let body = bytes.len() - CRC_LEN;
    let crc = crc32(&bytes[..body]);
    bytes[body..].copy_from_slice(&crc.to_le_bytes());
    bytes
}

#[test]
fn corrupt_encodings_are_rejected() {
    let bytes = sample_registry().encode();
    for len in [0, PREAMBLE_LEN + CRC_LEN - 1] {
        assert_eq!(
            MaterialRegistry::decode(&bytes[..len]),
            Err(RegistryError::Truncated(len))
        );
    }
    let mut flipped = bytes.clone();
    flipped[PREAMBLE_LEN + 2] ^= 0x40;
    assert!(matches!(
        MaterialRegistry::decode(&flipped),
        Err(RegistryError::Checksum { .. })
    ));
    let mut magic = bytes.clone();
    magic[0] = b'X';
    assert!(matches!(
        MaterialRegistry::decode(&magic),
        Err(RegistryError::Magic(_))
    ));
    let mut version = bytes.clone();
    version[4] = 9;
    assert_eq!(
        MaterialRegistry::decode(&version),
        Err(RegistryError::Version(9))
    );

    let count_at = PREAMBLE_LEN - 4;
    let mut huge = bytes.clone();
    huge[count_at..PREAMBLE_LEN].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        MaterialRegistry::decode(&resigned(huge)),
        Err(RegistryError::Count(u32::MAX))
    );
    let mut more = bytes.clone();
    more[count_at] += 1;
    assert!(matches!(
        MaterialRegistry::decode(&resigned(more)),
        Err(RegistryError::Truncated(_))
    ));
    let mut fewer = bytes.clone();
    fewer[count_at] -= 1;
    assert!(matches!(
        MaterialRegistry::decode(&resigned(fewer)),
        Err(RegistryError::TrailingBytes(_))
    ));
    let mut tag = bytes.clone();
    tag[PREAMBLE_LEN] = 7;
    assert_eq!(
        MaterialRegistry::decode(&resigned(tag)),
        Err(RegistryError::Tag { entry: 0, tag: 7 })
    );
}

#[test]
fn invalid_entries_are_rejected() {
    let encode = |keys: &[MaterialKey]| {
        let mut registry = MaterialRegistry::new();
        for key in keys {
            registry.push(key.clone()).expect("room");
        }
        registry.encode()
    };
    let twice = encode(&[MaterialKey::Network(5), MaterialKey::Network(5)]);
    assert_eq!(
        MaterialRegistry::decode(&twice),
        Err(RegistryError::Duplicate(1))
    );
    let mut bad_utf8 = encode(&[custom("a:b", "x")]);
    let state_byte = bad_utf8.len() - CRC_LEN - 1;
    bad_utf8[state_byte] = 0xff;
    assert_eq!(
        MaterialRegistry::decode(&resigned(bad_utf8)),
        Err(RegistryError::Utf8(0))
    );
    let mut empty_name = encode(&[custom("a", "")]);
    let name_at = PREAMBLE_LEN + 1;
    empty_name[name_at..name_at + 2].copy_from_slice(&0_u16.to_le_bytes());
    empty_name.remove(name_at + 2);
    assert_eq!(
        MaterialRegistry::decode(&resigned(empty_name)),
        Err(RegistryError::EmptyName)
    );
}
