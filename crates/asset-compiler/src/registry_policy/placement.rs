//! Compile placement admission and counterpart joins without changing the block wire format.

use std::collections::BTreeSet;

use assets::{RegistryRecord, facts::PlacementRule as R};

use super::{Result, check};

/// Writes one reviewed identity row; state shape disagreements remain unresolved.
pub(super) fn compile(
    source: &[super::BlockFact],
    records: &[RegistryRecord],
    policy: &super::Policy,
) -> Result<Vec<u8>> {
    let families = records.iter().map(|r| r.model_family).collect::<Vec<_>>();
    let states = source
        .iter()
        .map(|b| serde_json::from_str(&b.state))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut ids = BTreeSet::new();
    for selector in &policy.placement_rules {
        check(ids.insert(&selector.id), "duplicate placement rule id")?;
        super::block_rules::validate(selector, source, &families, &states)?;
        check(
            selector
                .fact
                .strip_prefix("placement:")
                .and_then(R::from_name)
                .is_some(),
            "unknown placement fact",
        )?;
    }
    let rows = assets::facts::placement_catalog(records, &policy.placement_rules)?;
    let mut text = String::from("identifier\tplacement\tsupport\talternate\n");
    for (name, entry) in rows {
        text.push_str(&format!(
            "{name}\t{:?}\t{:?}\t{}\n",
            entry.rule,
            entry.support,
            entry.alternate.as_deref().unwrap_or("-")
        ));
    }
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counterpart_joins_require_collected_independent_identities() {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let policy: super::super::Policy = serde_json::from_slice(super::super::POLICY).unwrap();
        let catalog = assets::facts::placement_catalog(&records, &policy.placement_rules).unwrap();
        let mut slabs = 0;
        let mut signs = 0;
        for entry in catalog.values() {
            let Some(target) = entry.alternate.as_deref() else {
                continue;
            };
            let other = catalog.get(target).unwrap();
            assert!(matches!(
                (entry.rule, other.rule),
                (R::Slab, R::DoubleSlab)
                    | (R::StandingSign, R::WallSign)
                    | (R::WallSign, R::StandingSign)
            ));
            if entry.rule == R::Slab {
                slabs += 1;
            } else {
                signs += 1;
            }
        }
        assert!(slabs > 0 && signs > 0);
    }
}
