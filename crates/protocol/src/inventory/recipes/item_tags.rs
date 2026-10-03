//! Vanilla item tag membership from the pinned Dragonfly-derived table.

use std::sync::OnceLock;

const TABLE: &str = include_str!("../../../data/item_tags_dragonfly.tsv");

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

    /// The embedded table's LF contents are exactly the pinned artifact, and its header's
    /// source digest is recorded in the data-sources manifest.
    #[test]
    fn table_is_pinned_and_its_source_is_in_the_manifest() {
        // Git's Windows checkout can convert the text table to CRLF.
        let canonical = TABLE.replace("\r\n", "\n");
        let digest: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
        assert_eq!(
            hex(&digest),
            "f291e91d363203b16f92c6625361e09ed367d65ed67d9fb056b527019867c3ac"
        );
        let header = TABLE.lines().next().unwrap();
        let source = header
            .split_once("sha256=")
            .unwrap()
            .1
            .split(';')
            .next()
            .unwrap();
        let manifest = include_str!("../../../../../assets/block-data-sources.json");
        assert!(manifest.contains(&format!("\"sha256\": \"{source}\"")));
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

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
