//! Block identities whose legacy aux values select block states.

use std::sync::OnceLock;

const SOURCE: &str = include_str!("../../data/block-aux-state-owners.tsv");

/// Whether an unresolved aux route may require a different block state or identity.
/// Exact visual definitions remain authoritative for these blocks.
pub fn block_item_has_aux_states(identifier: &str) -> bool {
    static IDENTIFIERS: OnceLock<Vec<&str>> = OnceLock::new();
    IDENTIFIERS
        .get_or_init(|| SOURCE.lines().skip(1).collect())
        .binary_search(&identifier)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aux_state_owners_match_the_active_version_and_are_sorted() {
        let mut rows = SOURCE.lines();
        let (field, version) = rows.next().unwrap().split_once('\t').unwrap();
        assert_eq!(field, "game_version");
        let target: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json"))
                .unwrap();
        assert_eq!(version, target["game_version"].as_str().unwrap());
        let identifiers: Vec<_> = rows.collect();
        assert!(!identifiers.is_empty());
        assert!(identifiers.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(identifiers.iter().all(|identifier| {
            identifier.strip_prefix("minecraft:").is_some_and(|name| {
                !name.is_empty()
                    && name.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
            })
        }));
    }

    #[test]
    fn stateful_aux_blocks_are_distinct_from_flattened_materials() {
        for identifier in ["minecraft:stone", "minecraft:dirt", "minecraft:oak_stairs"] {
            assert!(block_item_has_aux_states(identifier), "{identifier}");
        }
        for identifier in [
            "minecraft:granite",
            "minecraft:coarse_dirt",
            "minecraft:blue_terracotta",
        ] {
            assert!(!block_item_has_aux_states(identifier), "{identifier}");
        }
    }
}
