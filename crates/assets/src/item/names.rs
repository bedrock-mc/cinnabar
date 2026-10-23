//! Item and backing-block serialization identities for localized display names.

use std::{collections::BTreeMap, sync::OnceLock};

const SOURCE: &str = include_str!("../../data/item-name-identities-1.26.50.tsv");
/// The pinned item registry table, retained as an entity carrier source.
pub const SOURCE_BYTES: &[u8] = SOURCE.as_bytes();
/// Logical source identity retained in an entity carrier.
pub const SOURCE_PATH: &str = "registry/item-name-identities-1.26.50.tsv";

/// Every pinned registry item, sorted; a superset of the creative inventory.
pub fn identifiers() -> impl Iterator<Item = &'static str> {
    identities().keys().copied()
}

/// Returns the pinned item and optional backing-block description identities.
pub fn serialization_ids(identifier: &str) -> Option<[&'static str; 2]> {
    identities().get(identifier).copied()
}

/// Reads the generated identities once and rejects unordered or duplicate rows.
fn identities() -> &'static BTreeMap<&'static str, [&'static str; 2]> {
    static IDENTITIES: OnceLock<BTreeMap<&'static str, [&'static str; 2]>> = OnceLock::new();
    IDENTITIES.get_or_init(|| {
        let mut entries = BTreeMap::new();
        let mut previous = "";
        for row in SOURCE.lines().skip(1) {
            let mut fields = row.split('\t');
            let identifier = fields.next().expect("item identity");
            let item = fields.next().expect("item serialization identity");
            let block = fields.next().expect("block serialization identity");
            let block = if block == "-" { "" } else { block };
            assert!(identifier > previous && !item.is_empty() && fields.next().is_none());
            entries.insert(identifier, [item, block]);
            previous = identifier;
        }
        entries
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_cover_retail_items_and_match_the_source_pin() {
        let digest =
            crate::canonical_source_manifest_sha256(crate::VANILLA_SOURCE_MANIFEST.as_bytes());
        assert_eq!(
            SOURCE.lines().next().unwrap(),
            format!(
                "# source_manifest_sha256\t{}",
                digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            )
        );
        for identifier in super::super::retail::identifiers() {
            assert!(
                serialization_ids(identifier).is_some(),
                "missing {identifier}"
            );
        }
        assert!(serialization_ids("example:cod").is_none());
    }

    #[test]
    fn identities_match_pinned_metadata() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let metadata = root
            .join(&*crate::vanilla_source().cache_dir)
            .join("metadata/vanilladata_modules");
        let items = metadata.join("mojang-items.json");
        let blocks = metadata.join("mojang-blocks.json");
        if !items.is_file() || !blocks.is_file() {
            eprintln!(
                "skipping identities_match_pinned_metadata: missing pinned mojang-items.json or mojang-blocks.json"
            );
            return;
        }
        let read = |path| {
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap()).unwrap()
        };
        let items = read(items);
        let blocks = read(blocks);
        let blocks = blocks["data_items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry["name"].as_str().unwrap(),
                    entry["serialization_id"].as_str().unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let expected = items["data_items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let identifier = entry["name"].as_str().unwrap();
                (
                    identifier,
                    [
                        entry["serialization_id"].as_str().unwrap(),
                        blocks.get(identifier).copied().unwrap_or(""),
                    ],
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(identities(), &expected);
    }
}
