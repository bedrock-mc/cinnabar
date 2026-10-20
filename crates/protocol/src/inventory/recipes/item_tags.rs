//! Vanilla item tag membership from the shared native runtime catalog.

use std::sync::OnceLock;

const TABLE: &str = include_str!("../../../data/item_tags_native.tsv");

type Tags = Vec<(&'static str, Vec<&'static str>)>;

fn tags() -> &'static Tags {
    static TAGS: OnceLock<Tags> = OnceLock::new();
    TAGS.get_or_init(|| {
        let mut tags: Tags = TABLE
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let (tag, members) = line.split_once('\t')?;
                let mut members: Vec<_> = members.split(' ').collect();
                members.sort_unstable();
                Some((tag, members))
            })
            .collect();
        tags.sort_unstable_by_key(|(tag, _)| *tag);
        tags
    })
}

/// Whether the vanilla table puts `identifier` in `tag`; `None` for a tag the
/// table does not know.
#[must_use]
pub fn vanilla_tag_contains(tag: &str, identifier: &str) -> Option<bool> {
    let tags = tags();
    let index = tags.binary_search_by_key(&tag, |(tag, _)| *tag).ok()?;
    Some(tags[index].1.binary_search(&identifier).is_ok())
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    /// Memberships stay pinned while the header identifies the shared catalog's source lock.
    #[test]
    fn memberships_are_pinned_and_share_the_registry_source_lock() {
        // Git's Windows checkout can convert the text table to CRLF.
        let canonical = TABLE.replace("\r\n", "\n");
        let (header, memberships) = canonical.split_once('\n').unwrap();
        let digest: [u8; 32] = Sha256::digest(memberships.as_bytes()).into();
        assert_eq!(
            hex(&digest),
            "6d29cac293368b245adeab4c82820d8874251b7d3464131391fb58ad1f9f62b4"
        );
        let source = header
            .split_once("source_lock_sha256=")
            .unwrap()
            .1
            .split_ascii_whitespace()
            .next()
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../assets/block-projection-v2193.json"
        ))
        .unwrap();
        assert_eq!(
            source,
            manifest["source"]["source_lock_sha256"].as_str().unwrap()
        );
        for row in TABLE.lines().filter(|line| !line.starts_with('#')) {
            let (tag, members) = row.split_once('\t').expect("item tag row");
            for identifier in members.split_whitespace() {
                assert_eq!(
                    vanilla_tag_contains(tag, identifier),
                    Some(true),
                    "{tag}: {identifier}"
                );
            }
        }
    }

    #[test]
    fn membership_is_exact_and_unknown_tags_are_none() {
        assert_eq!(
            vanilla_tag_contains("minecraft:planks", "minecraft:oak_planks"),
            Some(true)
        );
        assert_eq!(
            vanilla_tag_contains("minecraft:planks", "minecraft:oak_log"),
            Some(false)
        );
        assert_eq!(
            vanilla_tag_contains("minecraft:not_a_tag", "minecraft:oak_log"),
            None
        );
    }

    /// Encodes the independently pinned membership digest for its fixture assertion.
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
