use std::collections::HashSet;

use protocol::vanilla_item_registry;
use sha2::{Digest, Sha256};

const RETAIL_ITEMS: &[u8] = include_bytes!("../data/retail_items_1_26_40.tsv");
const RETAIL_BIOMES: &[u8] = include_bytes!("../data/retail_biomes_1_26_40.txt");

fn canonical_text(bytes: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"\r\n") {
            normalized.push(b'\n');
            cursor += 2;
        } else {
            normalized.push(bytes[cursor]);
            cursor += 1;
        }
    }
    normalized
}

#[test]
fn retail_item_table_has_the_pinned_projection_fingerprint() {
    assert_eq!(
        format!("{:x}", Sha256::digest(canonical_text(RETAIL_ITEMS))),
        "ee8917e7293c89469d6d114cad634eac0b45a702a1d73e2edddd6d5eeee725d0"
    );
}

#[test]
fn retail_biome_table_has_the_pinned_projection_fingerprint() {
    assert_eq!(
        format!("{:x}", Sha256::digest(canonical_text(RETAIL_BIOMES))),
        "df7e18c18e939e21f387838479ee9c79b0d7eb798fb1bd906f51c56963058574"
    );
    let biomes = std::str::from_utf8(RETAIL_BIOMES)
        .expect("biome table must be UTF-8")
        .lines()
        .collect::<HashSet<_>>();
    assert_eq!(biomes.len(), 88);
    assert!(biomes.contains("minecraft:deep_warm_ocean"));
}

#[test]
fn retail_item_table_preserves_current_network_ids_and_gaps() {
    let entries = vanilla_item_registry();
    assert_eq!(entries.len(), 1_485);
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:stone")
            .map(|entry| entry.network_id),
        Some(1)
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:diamond_sword")
            .map(|entry| entry.network_id),
        Some(318)
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == "minecraft:apple")
            .map(|entry| entry.network_id),
        Some(878)
    );

    let ids: HashSet<_> = entries.iter().map(|entry| entry.network_id).collect();
    let names: HashSet<_> = entries
        .iter()
        .map(|entry| entry.identifier.as_ref())
        .collect();
    assert_eq!(ids.len(), entries.len());
    assert_eq!(names.len(), entries.len());
    assert!(!ids.contains(&-1121), "an omitted ID must remain a gap");
}
