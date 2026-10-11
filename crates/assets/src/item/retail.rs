//! The independent retail identity inventory used for item descriptor admission.

use std::{collections::BTreeSet, sync::OnceLock};

/// The existing protocol inventory included in descriptor carrier fingerprints.
pub const SOURCE_BYTES: &[u8] = include_bytes!("../../../protocol/data/retail_items_1_26_50.tsv");
/// Logical source identity retained in an entity carrier.
pub const SOURCE_PATH: &str = "registry/retail-items-v2193.tsv";
/// Checkout input read by the entity carrier recipe.
pub const INPUT_PATH: &str = "crates/protocol/data/retail_items_1_26_50.tsv";

/// Reads the separately maintained identity inventory once, outside rendering paths.
pub fn identifiers() -> &'static BTreeSet<&'static str> {
    static IDENTIFIERS: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    IDENTIFIERS.get_or_init(|| {
        let rows = std::str::from_utf8(SOURCE_BYTES).expect("retail inventory is UTF-8");
        let identifiers = rows
            .lines()
            .filter_map(|row| row.split('\t').nth(1))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            identifiers.len(),
            rows.lines().count(),
            "retail identities are unique"
        );
        identifiers
    })
}
