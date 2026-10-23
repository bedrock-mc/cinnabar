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
    use sha2::{Digest, Sha256};

    /// Checks formatting, the active version and independently pinned owner membership.
    fn assert_valid_catalog(source: &str) {
        let mut rows = source.lines();
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
        // Refresh this pin from the target version's complete registration keys,
        // independently of this catalog. Statefulness alone does not imply aux ownership.
        let mut hash = Sha256::new();
        for line in source.lines() {
            hash.update(line.as_bytes());
            hash.update(b"\n");
        }
        assert_eq!(
            format!("{:x}", hash.finalize()),
            "e86ea4da57802dd11f117c71c59e896d4529fa6e7fd72ff7435bd5969f391ec4",
            "aux-state owner membership differs from the pinned source"
        );
    }

    #[test]
    fn aux_state_owners_match_the_pinned_membership_and_active_version() {
        assert_valid_catalog(SOURCE);
    }

    #[test]
    fn aux_state_owner_validation_rejects_an_omission() {
        let omitted = SOURCE.replace("minecraft:anvil\n", "");
        assert_ne!(omitted, SOURCE);
        assert!(std::panic::catch_unwind(|| assert_valid_catalog(&omitted)).is_err());
    }

    #[test]
    fn aux_state_owner_validation_rejects_an_extra() {
        let extra = SOURCE.replace(
            "minecraft:blue_candle\n",
            "minecraft:blue_candle\nminecraft:blue_terracotta\n",
        );
        assert_ne!(extra, SOURCE);
        assert!(std::panic::catch_unwind(|| assert_valid_catalog(&extra)).is_err());
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
