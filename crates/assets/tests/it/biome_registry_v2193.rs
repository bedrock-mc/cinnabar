use std::collections::BTreeSet;

use assets::read_biome_registry;
use sha2::{Digest, Sha256};

const REGISTRY: &[u8] = include_bytes!("../../data/biome-registry-v2193.bin");
const ALLOWLIST: &str = include_str!("../../../protocol/data/retail_biomes_1_26_50.txt");

#[test]
fn checked_in_v2193_biome_registry_exactly_matches_the_retail_allowlist() {
    let records = read_biome_registry(REGISTRY).expect("decode checked-in v2193 BIOREG01");
    assert!(records.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert_eq!(
        format!("{:x}", Sha256::digest(REGISTRY)),
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../assets/bedrock-target.json"
        ))
        .unwrap()["hashes"]["biome_registry"]
            .as_str()
            .unwrap()
    );

    let decoded = records
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<BTreeSet<_>>();
    let allowed = ALLOWLIST.lines().collect::<BTreeSet<_>>();
    assert_eq!(records.len(), allowed.len(), "one record per allowed biome");
    assert_eq!(decoded, allowed);
}

#[test]
fn production_decoder_rejects_malformed_duplicate_range_and_trailing_v2193_data() {
    let malformed = &REGISTRY[..REGISTRY.len() - 1];
    assert!(read_biome_registry(malformed).is_err());

    let mut duplicate = b"BIOREG01".to_vec();
    duplicate.extend_from_slice(&2_u32.to_le_bytes());
    for name in ["minecraft:first", "minecraft:second"] {
        duplicate.extend_from_slice(&1_u32.to_le_bytes());
        duplicate.extend_from_slice(&(name.len() as u16).to_le_bytes());
        duplicate.extend_from_slice(name.as_bytes());
    }
    assert!(read_biome_registry(&duplicate).is_err());

    let mut out_of_range = b"BIOREG01".to_vec();
    out_of_range.extend_from_slice(&1_u32.to_le_bytes());
    out_of_range.extend_from_slice(&65_536_u32.to_le_bytes());
    out_of_range.extend_from_slice(&1_u16.to_le_bytes());
    out_of_range.push(b'x');
    assert!(read_biome_registry(&out_of_range).is_err());

    let mut trailing = REGISTRY.to_vec();
    trailing.push(0);
    assert!(read_biome_registry(&trailing).is_err());
}
