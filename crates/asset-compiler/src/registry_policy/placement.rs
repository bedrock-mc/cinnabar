//! Compile placement admission and counterpart joins without changing the block wire format.

use std::collections::{BTreeMap, BTreeSet};

use assets::{
    RegistryRecord,
    facts::{BlockProperty as P, PlacementRule as R, SupportKind},
    registry::{BlockTraits as T, ModelFamily as F},
};

use super::{Result, check};

/// Selects transformations from state shape and previously compiled identity facts.
fn rule(record: &RegistryRecord, policy: &super::Policy) -> Result<R> {
    let state = serde_json::from_str(&record.canonical_state)?;
    for rule in &policy.placement_rules {
        if rule.fact.starts_with("placement:")
            && rule.matches(&record.name, &state, record.model_family)
        {
            return R::from_name(rule.fact.strip_prefix("placement:").unwrap())
                .ok_or_else(|| "unknown placement rule".into());
        }
    }
    let traits = record.traits;
    Ok(if traits.contains(T::TORCH) {
        R::Torch
    } else if traits.contains(T::MULTIFACE) {
        R::Multiface
    } else if traits.contains(T::LANTERN_LIKE) {
        R::Lantern
    } else if traits.contains(T::DOOR) {
        R::Door
    } else if traits.contains(T::SNOW_LAYER) {
        R::Snow
    } else if traits.contains(T::CANDLE) {
        R::Candle
    } else if traits.contains(T::STANDING_SIGN) {
        R::StandingSign
    } else if traits.contains(T::WALL_SIGN) {
        R::WallSign
    } else if traits.contains(T::HOPPER) {
        R::Hopper
    } else if traits.contains(T::BODY_RELATIVE_FACING) {
        if traits.contains(T::PISTON) {
            R::Piston
        } else {
            R::Dispenser
        }
    } else if traits.intersects(T::CONNECTS_THIN | T::CONNECTS_FENCE) {
        R::Connections
    } else if state.contains_key(P::ClusterCount.name()) {
        R::SeaPickle
    } else {
        match record.model_family {
            F::Stair => R::Stair,
            F::Slab if traits.contains(T::DOUBLE_SLAB) => R::DoubleSlab,
            F::Slab => R::Slab,
            F::Trapdoor => R::Trapdoor,
            F::Lever => R::Lever,
            F::Button => R::Button,
            F::Vine => R::Vine,
            _ if state.contains_key(P::PillarAxis.name()) => R::Axis,
            _ if record.properties.is_empty() && record.canonical_state.as_ref() == "{}" => {
                R::Identity
            }
            _ => R::Unresolved,
        }
    })
}

/// Expands the existing naming contract once, requiring a collected counterpart.
fn counterpart(name: &str, rule: R, names: &BTreeSet<&str>) -> Option<String> {
    let target = match rule {
        R::Slab => {
            if let Some(prefix) = name.strip_suffix("cut_copper_slab") {
                format!("{prefix}double_cut_copper_slab")
            } else {
                format!("{}_double_slab", name.strip_suffix("_slab")?)
            }
        }
        R::StandingSign => format!("{}wall_sign", name.strip_suffix("standing_sign")?),
        R::WallSign => format!("{}standing_sign", name.strip_suffix("wall_sign")?),
        _ => return None,
    };
    names.contains(target.as_str()).then_some(target)
}

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
        selector.validate(source, &families, &states)?;
        check(
            selector
                .fact
                .strip_prefix("placement:")
                .and_then(R::from_name)
                .is_some()
                || selector
                    .fact
                    .strip_prefix("support:")
                    .and_then(SupportKind::from_name)
                    .is_some(),
            "unknown placement fact",
        )?;
    }
    let names = records.iter().map(|r| r.name.as_ref()).collect();
    let mut rows = BTreeMap::new();
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() != super::RESERVED)
    {
        let state = serde_json::from_str(&record.canonical_state)?;
        let support = policy
            .placement_rules
            .iter()
            .find(|r| {
                r.fact.starts_with("support:")
                    && r.matches(&record.name, &state, record.model_family)
            })
            .map_or("Unresolved", |r| r.fact.strip_prefix("support:").unwrap());
        let compiled = rule(record, policy)?;
        let row = rows
            .entry(record.name.as_ref())
            .or_insert((compiled, support));
        if row.0 != compiled {
            row.0 = R::Unresolved;
        }
        check(
            row.1 == support,
            "state-dependent support requires a compiled trait",
        )?;
    }
    let mut text = String::from("identifier\tplacement\tsupport\talternate\n");
    for (name, (rule, support)) in rows {
        text.push_str(&format!(
            "{name}\t{rule:?}\t{support}\t{}\n",
            counterpart(name, rule, &names).as_deref().unwrap_or("-")
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
        let policy = serde_json::from_slice(super::super::POLICY).unwrap();
        let names = records.iter().map(|r| r.name.as_ref()).collect();
        let mut slabs = 0;
        let mut signs = 0;
        for record in &records {
            let rule = rule(record, &policy).unwrap();
            if matches!(rule, R::Slab | R::StandingSign | R::WallSign) {
                let Some(target) = counterpart(&record.name, rule, &names) else {
                    continue;
                };
                let other = records.iter().find(|r| r.name.as_ref() == target).unwrap();
                assert!(matches!(
                    (rule, self::rule(other, &policy).unwrap()),
                    (R::Slab, R::DoubleSlab)
                        | (R::StandingSign, R::WallSign)
                        | (R::WallSign, R::StandingSign)
                ));
                assert!(counterpart(&record.name, rule, &BTreeSet::new()).is_none());
                if rule == R::Slab {
                    slabs += 1;
                } else {
                    signs += 1;
                }
            }
        }
        assert!(slabs > 0 && signs > 0);
    }
}
