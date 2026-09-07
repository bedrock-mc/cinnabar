use std::sync::OnceLock;

const CAPACITY_DATA: &str = include_str!("../data/item_capacity_1_26_40.tsv");
static CAPACITIES: OnceLock<Box<[(&'static str, u8)]>> = OnceLock::new();

/// Returns the measured vanilla capacity for a bare retail item at metadata zero.
///
/// This is a protocol-2168 baseline, not a negotiated inventory rule. Runtime consumers must
/// first bind the exact active identifier and reconcile server-provided item properties and
/// component overrides. Metadata variants and unknown identifiers deliberately return `None`.
pub fn vanilla_item_capacity(identifier: &str, metadata: u32) -> Option<u8> {
    if metadata != 0 {
        return None;
    }
    let capacities = CAPACITIES.get_or_init(parse_capacity_data);
    capacities
        .binary_search_by(|(candidate, _)| candidate.cmp(&identifier))
        .ok()
        .map(|index| capacities[index].1)
}

fn parse_capacity_data() -> Box<[(&'static str, u8)]> {
    CAPACITY_DATA
        .lines()
        .map(|line| {
            let (identifier, max_count) = line
                .split_once('\t')
                .expect("generated item capacity row must have two columns");
            let max_count = max_count
                .parse::<u8>()
                .expect("generated item capacity must fit in u8");
            (identifier, max_count)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::vanilla_item_capacity;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeSet;

    const CAPACITY_DATA: &str = include_str!("../data/item_capacity_1_26_40.tsv");
    const RETAIL_ITEMS: &str = include_str!("../data/retail_items_1_26_40.tsv");

    #[test]
    fn returns_measured_retail_capacities_for_bare_items() {
        assert_eq!(vanilla_item_capacity("minecraft:water_bucket", 0), Some(1));
        assert_eq!(vanilla_item_capacity("minecraft:bucket", 0), Some(16));
        assert_eq!(vanilla_item_capacity("minecraft:apple", 0), Some(64));
    }

    #[test]
    fn rejects_unknown_items_and_metadata_variants() {
        assert_eq!(
            vanilla_item_capacity("minecraft:not_a_retail_item", 0),
            None
        );
        assert_eq!(vanilla_item_capacity("minecraft:apple", 1), None);
    }

    #[test]
    fn generated_table_is_sorted_unique_and_covers_the_retail_allowlist() {
        let identifiers = CAPACITY_DATA
            .lines()
            .map(|line| line.split_once('\t').expect("capacity row").0)
            .collect::<Vec<_>>();
        assert_eq!(identifiers.len(), 1_485);
        assert!(identifiers.windows(2).all(|pair| pair[0] < pair[1]));

        let retail = RETAIL_ITEMS
            .lines()
            .map(|line| line.split_once('\t').expect("retail row").1)
            .collect::<BTreeSet<_>>();
        assert_eq!(identifiers.into_iter().collect::<BTreeSet<_>>(), retail);
    }

    #[test]
    fn generated_inputs_and_output_match_reviewed_hashes() {
        assert_eq!(
            sha256(CAPACITY_DATA.as_bytes()),
            "a494566eaf96fb57a38a736a1ec02d54424669e9272ae4c889be39c6f3e9caf3"
        );
        assert_eq!(
            sha256(RETAIL_ITEMS.as_bytes()),
            "ee8917e7293c89469d6d114cad634eac0b45a702a1d73e2edddd6d5eeee725d0"
        );
    }

    fn sha256(contents: &[u8]) -> String {
        format!("{:x}", Sha256::digest(contents))
    }
}
