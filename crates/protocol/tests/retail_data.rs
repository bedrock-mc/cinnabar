use std::collections::HashSet;

use protocol::vanilla_item_registry;
use sha2::{Digest, Sha256};

const RETAIL_ITEMS: &[u8] = include_bytes!("../data/retail_items_1_26_50.tsv");
const RETAIL_BIOMES: &[u8] = include_bytes!("../data/retail_biomes_1_26_50.txt");

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
        "6f186e8f781c611722cd28ece47f643112732a89e18cd9beab9d414243750821"
    );
}

#[test]
fn retail_biome_table_has_the_pinned_projection_fingerprint() {
    assert_eq!(
        format!("{:x}", Sha256::digest(canonical_text(RETAIL_BIOMES))),
        "6127c74c17455273bb5226f1e05e98709bc247c05a0137a8827cb97756c3b198"
    );
    let biomes = std::str::from_utf8(RETAIL_BIOMES)
        .expect("biome table must be UTF-8")
        .lines()
        .collect::<HashSet<_>>();
    assert!(biomes.contains("minecraft:deep_warm_ocean"));
}

#[test]
fn retail_item_table_preserves_current_network_ids_and_gaps() {
    let entries = vanilla_item_registry();
    let expected = std::str::from_utf8(RETAIL_ITEMS)
        .expect("item table must be UTF-8")
        .lines()
        .map(|line| {
            let (id, name) = line.split_once('\t').expect("item table row");
            (id.parse::<i32>().expect("network item ID"), name)
        })
        .collect::<HashSet<_>>();
    let actual = entries
        .iter()
        .map(|entry| (entry.network_id, entry.identifier.as_ref()))
        .collect::<HashSet<_>>();
    assert_eq!(actual, expected);
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
        Some(882)
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
