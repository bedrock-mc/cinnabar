use serde_json::Value;
use sha2::{Digest, Sha256};

/// Finds the unique air record using the registry schema, validating its pinned identity.
pub(crate) fn air_ids(bytes: &[u8], schema: &Value, target: &Value) -> (u32, u32) {
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        target["hashes"]["block_registry"].as_str().unwrap()
    );
    let magic = schema["magic"].as_str().unwrap().as_bytes();
    assert!(bytes.starts_with(magic), "unsupported block registry");
    let mut offset = magic.len();
    let metadata = schema["structs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["rust_name"] == "RegistryMetadata")
        .unwrap();
    let mut count = None;
    for field in metadata["fields"].as_array().unwrap() {
        assert_eq!(field["rust_type"], "u32");
        let raw = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;
        match field["rust_name"].as_str().unwrap() {
            "protocol" => assert_eq!(u64::from(raw), target["wire_protocol"].as_u64().unwrap()),
            "canonical_states" => count = Some(raw),
            _ => {}
        }
    }
    let mut air = None;
    let collision = schema["structs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["rust_name"] == "CollisionBox")
        .unwrap();
    let collision_bytes = collision["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            assert_eq!(field["rust_type"], "i32");
            4
        })
        .sum::<usize>();
    for _ in 0..count.unwrap() {
        let mut record = std::collections::BTreeMap::new();
        for field in schema["record_header"].as_array().unwrap() {
            let kind = field["type"].as_str().unwrap();
            let size = match kind {
                "u8" => 1,
                "u16" => 2,
                "u32" => 4,
                "u32[8]" => 32,
                _ => panic!("unsupported registry field type {kind}"),
            };
            let raw = if size <= 4 {
                bytes[offset..offset + size]
                    .iter()
                    .enumerate()
                    .fold(0u32, |value, (index, byte)| {
                        value | (u32::from(*byte) << (index * 8))
                    })
            } else {
                0
            };
            record.insert(field["name"].as_str().unwrap(), raw);
            offset += size;
        }
        offset += record["box_count"] as usize * collision_bytes;
        let name_len = record["name_len"] as usize;
        let name = &bytes[offset..offset + name_len];
        if name == b"minecraft:air" {
            assert!(air.is_none(), "more than one air state");
            air = Some((record["sequential_id"], record["network_hash"]));
        }
        offset += name_len + record["state_len"] as usize;
    }
    assert_eq!(offset, bytes.len(), "registry payload length mismatch");
    air.expect("active registry has an air state")
}
