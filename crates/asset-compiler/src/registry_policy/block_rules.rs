//! Compile identifier selectors once against the pinned block palette.

use super::{BlockFact, Result, Scalar, check};
use assets::facts::rules::{self, Rule, Status, property_key};
use assets::facts::shape::ShapeTraits;
use assets::registry::{BlockTraits, ModelFamily, RegistryRecord, SurvivalRule};
use std::collections::{BTreeMap, BTreeSet};

/// Rejects misspelled members, unknown domains and selectors that silently match nothing.
pub(super) fn validate(
    rule: &Rule,
    source: &[BlockFact],
    families: &[ModelFamily],
    states: &[BTreeMap<String, Scalar>],
) -> Result<()> {
    let names: BTreeSet<_> = source.iter().map(|b| b.name.as_str()).collect();
    check(
        (rule.compatibility_members.is_empty() && rule.compatibility_exclude_members.is_empty())
            || rule.fact.starts_with("family:")
            || rule.fact.starts_with("render:"),
        "compatibility selectors require a model family or render rule",
    )?;
    for member in rule
        .members
        .iter()
        .chain(&rule.exclude_members)
        .chain(&rule.compatibility_exclude_members)
    {
        check(
            names.contains(member.as_str()),
            format!("{} has unknown block {member}", rule.id),
        )?;
    }
    for family in &rule.model_family {
        check(
            ModelFamily::from_name(family).is_some(),
            format!("{} has unknown family {family}", rule.id),
        )?;
    }
    for key in rule
        .state_property
        .iter()
        .chain(&rule.state_properties)
        .chain(rule.state_values.keys())
        .chain(&rule.state_maximum)
        .chain(rule.state_minimum.keys())
    {
        check(
            property_key(key).is_some(),
            format!("{} has unknown property {key}", rule.id),
        )?;
    }
    let matched = source
        .iter()
        .zip(families)
        .zip(states)
        .any(|((block, family), state)| rule.matches(&block.name, state, *family));
    check(matched, format!("{} matches no pinned blocks", rule.id))
}

/// Resolves the first matching family rule, retaining admitted geometry for unlisted classes.
pub(super) fn family(rules: &[Rule], block: &BlockFact, old: ModelFamily) -> Result<ModelFamily> {
    let state = serde_json::from_str(&block.state)?;
    for rule in rules {
        if rule.matches(&block.name, &state, old) {
            return ModelFamily::from_name(
                rule.fact
                    .strip_prefix("family:")
                    .ok_or("invalid family rule")?,
            )
            .ok_or_else(|| "unknown model family rule".into());
        }
    }
    Ok(old)
}

/// Compiles traits and support rules, then emits a complete membership and uncertainty diff.
pub(super) fn compile(
    source: &[BlockFact],
    records: &mut [RegistryRecord],
    policy: &super::Policy,
    legacy: &[RegistryRecord],
) -> Result<Vec<u8>> {
    policy.pack_rules.validate()?;
    let families: Vec<_> = records.iter().map(|r| r.model_family).collect();
    let states: Vec<BTreeMap<String, Scalar>> = source
        .iter()
        .map(|b| serde_json::from_str(&b.state))
        .collect::<std::result::Result<_, _>>()?;
    let mut ids = BTreeSet::new();
    for rule in policy.model_family_rules.iter().chain(&policy.block_rules) {
        check(ids.insert(&rule.id), "duplicate block rule id")?;
        validate(rule, source, &families, &states)?;
        if let Some(value) = rule.fact.strip_prefix("shape:") {
            check(
                ShapeTraits::from_name(value).is_some(),
                format!("unknown shape trait {value}"),
            )?;
        } else if let Some(kind) = rule.fact.strip_prefix("render:") {
            let _: assets::facts::BlockRenderKind =
                serde_json::from_value(serde_json::Value::String(kind.into()))?;
        } else if let Some(tint) = rule.fact.strip_prefix("render_tint:") {
            let _: assets::facts::BlockRenderTint =
                serde_json::from_value(serde_json::Value::String(tint.into()))?;
        } else if let Some(flag) = rule.fact.strip_prefix("render_flag:") {
            check(
                matches!(
                    flag,
                    "fire_top" | "fire_flammable" | "flat_icon" | "fast_sword"
                ),
                "unknown render flag",
            )?;
        } else if rule.fact.starts_with("legacy_alias:") || rule.fact == "model_variant" {
            presentation(rule, &mut assets::facts::BlockRenderFact::default())?;
        } else if !rule.fact.starts_with("family:") && !rule.fact.starts_with("survival:") {
            check(
                BlockTraits::from_name(&rule.fact).is_some(),
                format!("unknown block trait {}", rule.fact),
            )?;
        } else if let Some(value) = rule.fact.strip_prefix("survival:") {
            check(
                SurvivalRule::from_name(value).is_some(),
                "unknown survival rule",
            )?;
        }
    }
    let legacy_names: BTreeSet<_> = legacy.iter().map(|record| record.name.as_ref()).collect();
    for rule in &policy.block_rules {
        for name in &rule.compatibility_members {
            check(
                legacy_names.contains(name.as_str()),
                "unknown historical render member",
            )?;
        }
    }
    let components = components(source)?;
    let backings = rules::backings();
    let mut render_facts = BTreeMap::<String, assets::facts::BlockRenderFact>::new();
    let mut kinds_decided = BTreeSet::new();
    let mut tints_decided = BTreeSet::new();
    let mut expansion =
        BTreeMap::<String, (BlockTraits, ShapeTraits, BTreeSet<String>, BTreeSet<String>)>::new();
    for ((block, record), state) in source.iter().zip(records).zip(&states) {
        let (mut traits, survival, shapes) = rules::evaluate(
            &block.name,
            state,
            record.model_family,
            record.flags,
            &policy.block_rules,
        );
        traits.set(
            BlockTraits::CAN_CONTAIN_LIQUID_SOURCE,
            block.can_contain_liquid_source,
        );
        let mut applied = BTreeSet::new();
        applied.insert("pinned-liquid-containment".into());
        let mut provisional = BTreeSet::new();
        if let Some(rule) = policy
            .model_family_rules
            .iter()
            .find(|rule| rule.matches(&block.name, state, record.model_family))
        {
            applied.insert(rule.id.clone());
        }
        if components.contains_key(&block.name) {
            applied.insert("pinned-components".into());
        }
        if backings.contains(block.name.as_str()) {
            applied.insert("block-entity-backings".into());
        }
        for rule in &policy.block_rules {
            if !rule.matches(&block.name, state, record.model_family) {
                continue;
            }
            applied.insert(rule.id.clone());
            presentation(rule, render_facts.entry(block.name.clone()).or_default())?;
            if matches!(rule.status, Status::Provisional) {
                provisional.insert(rule.id.clone());
            }
            if let Some(kind) = rule.fact.strip_prefix("render:")
                && kinds_decided.insert(block.name.clone())
            {
                render_facts.entry(block.name.clone()).or_default().kind =
                    serde_json::from_value(serde_json::Value::String(kind.into()))?;
            } else if let Some(tint) = rule.fact.strip_prefix("render_tint:")
                && tints_decided.insert(block.name.clone())
            {
                render_facts.entry(block.name.clone()).or_default().tint =
                    serde_json::from_value(serde_json::Value::String(tint.into()))?;
            } else if let Some(flag) = rule.fact.strip_prefix("render_flag:") {
                let render = render_facts.entry(block.name.clone()).or_default();
                match flag {
                    "fire_flammable" => render.fire_flammable = rule.value,
                    "fire_top" => render.fire_top = rule.value,
                    "flat_icon" => render.flat_icon = rule.value,
                    "fast_sword" => render.fast_sword = rule.value,
                    _ => return Err("unknown render flag".into()),
                }
            }
        }
        if traits.contains(BlockTraits::PORTAL) && !kinds_decided.contains(&block.name) {
            render_facts.entry(block.name.clone()).or_default().kind =
                assets::facts::BlockRenderKind::Portal;
        }
        if traits.contains(BlockTraits::BLOCK_ENTITY_DRAWN) && !kinds_decided.contains(&block.name)
        {
            render_facts.entry(block.name.clone()).or_default().kind =
                assets::facts::BlockRenderKind::EntityDrawn;
        }
        let entry = expansion.entry(block.name.clone()).or_default();
        entry.0 |= traits;
        entry.1 |= shapes;
        entry.2.extend(applied);
        entry.2.insert(format!("survival:{survival:?}"));
        entry.3.extend(provisional);
        if record.name.as_ref() != super::RESERVED_BLOCK_NAME {
            record.traits = traits;
            record.survival = survival;
        } else {
            record.survival = SurvivalRule::Unresolved;
            entry.3.insert("unadmitted".into());
        }
    }
    for record in legacy {
        if expansion.contains_key(record.name.as_ref()) {
            continue;
        }
        let state = serde_json::from_str(&record.canonical_state)?;
        let mut render = assets::facts::BlockRenderFact::default();
        for rule in &policy.block_rules {
            if let Some(kind) = rule.fact.strip_prefix("render:")
                && (rule.matches(&record.name, &state, record.model_family)
                    || rule
                        .compatibility_members
                        .iter()
                        .any(|name| name == record.name.as_ref()))
            {
                render.kind = serde_json::from_value(serde_json::Value::String(kind.into()))?;
                break;
            }
        }
        for rule in &policy.block_rules {
            if rule.matches(&record.name, &state, record.model_family) {
                presentation(rule, &mut render)?;
            }
        }
        if render.kind == assets::facts::BlockRenderKind::Ordinary
            && policy.block_rules.iter().any(|rule| {
                rule.fact == "BLOCK_ENTITY_DRAWN"
                    && rule.value
                    && rule.matches(&record.name, &state, record.model_family)
            })
        {
            render.kind = assets::facts::BlockRenderKind::EntityDrawn;
        }
        if render.kind == assets::facts::BlockRenderKind::Ordinary
            && policy.block_rules.iter().any(|rule| {
                rule.fact == "PORTAL"
                    && rule.value
                    && rule.matches(&record.name, &state, record.model_family)
            })
        {
            render.kind = assets::facts::BlockRenderKind::Portal;
        }
        if render != assets::facts::BlockRenderFact::default() {
            render_facts.insert(record.name.to_string(), render);
            expansion
                .entry(record.name.to_string())
                .or_default()
                .3
                .insert("historical-render".into());
        }
    }
    let mut text = String::from("identifier\ttraits\trules\tprovisional\trender\n");
    for (name, (traits, shapes, rules, provisional)) in expansion {
        let names = traits
            .iter_names()
            .map(|(name, _)| name.to_owned())
            .chain(shapes.iter_names().map(|(name, _)| format!("shape:{name}")))
            .collect::<Vec<_>>()
            .join(",");
        text.push_str(&format!(
            "{name}\t{names}\t{}\t{}\t{}\n",
            rules.into_iter().collect::<Vec<_>>().join(","),
            if provisional.is_empty() {
                "-".to_owned()
            } else {
                provisional.into_iter().collect::<Vec<_>>().join(",")
            },
            serde_json::to_string(
                render_facts
                    .get(&name)
                    .unwrap_or(&assets::facts::BlockRenderFact::default())
            )?
        ));
    }
    Ok(text.into_bytes())
}

/// Resolves compiler-only shape classes from the same selectors as registry admission.
pub(super) fn for_record(record: &RegistryRecord, rules: &[Rule]) -> (BlockTraits, ShapeTraits) {
    let state = serde_json::from_str(&record.canonical_state).expect("decoded canonical state");
    let (_, _, shapes) = rules::evaluate(
        &record.name,
        &state,
        record.model_family,
        record.flags,
        rules,
    );
    (record.traits, shapes)
}

/// Rejects component entries that do not belong to the source palette.
fn components(source: &[BlockFact]) -> Result<&'static BTreeMap<String, BlockTraits>> {
    let names: BTreeSet<_> = source.iter().map(|b| b.name.as_str()).collect();
    let components = rules::components();
    for name in components.keys() {
        check(
            names.contains(name.as_str()),
            format!("unknown component block {name}"),
        )?;
    }
    Ok(components)
}

/// Combines all pinned name domains into one sorted vocabulary for compiler guards.
pub(super) fn vocabulary(root: &std::path::Path, blocks: &[BlockFact]) -> Result<Vec<u8>> {
    let mut names: BTreeSet<_> = blocks.iter().map(|b| b.name.clone()).collect();
    for row in include_str!("../../../assets/data/vanilla-pack-vocabulary-1.26.50.tsv").lines() {
        let (domain, name) = row.split_once('\t').ok_or("invalid pack vocabulary row")?;
        if matches!(domain, "block" | "item" | "actor" | "property") {
            names.insert(name.to_owned());
        }
    }
    for path in [
        "crates/protocol/data/retail_items_1_26_50.tsv",
        "crates/protocol/data/retail_actors_1_26_50.txt",
        "crates/protocol/data/retail_biomes_1_26_50.txt",
    ] {
        for row in std::fs::read_to_string(root.join(path))?.lines() {
            names.insert(
                row.split_once('\t')
                    .map_or(row, |(_, name)| name)
                    .to_owned(),
            );
        }
    }
    for row in include_str!("../../../protocol/data/item_tags_native.tsv")
        .lines()
        .filter(|r| !r.starts_with('#'))
    {
        if let Some((name, _)) = row.split_once('\t') {
            names.insert(name.to_owned());
        }
    }
    Ok(names
        .into_iter()
        .map(|name| format!("{name}\n"))
        .collect::<String>()
        .into_bytes())
}

/// Assigns registry-bound model metadata without making the runtime repeat name selection.
fn presentation(rule: &Rule, render: &mut assets::facts::BlockRenderFact) -> Result<()> {
    if let Some(alias) = rule.fact.strip_prefix("legacy_alias:") {
        render.legacy_alias = Some(serde_json::from_value(serde_json::Value::String(
            alias.into(),
        ))?);
    } else if rule.fact == "model_variant" {
        let variant = rule.model_variant.ok_or("missing model variant")?;
        if let assets::facts::BlockModelVariant::Skull(index) = variant {
            check(index <= 6, "invalid skull model index")?;
        }
        render.variant = Some(variant);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses one actual pinned torch state for selector rejection checks.
    fn torch() -> BlockFact {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:copper_torch")
            .unwrap();
        BlockFact {
            sequential_id: record.sequential_id,
            network_hash: record.network_hash,
            name: record.name.to_string(),
            state: record.canonical_state.to_string(),
            shape_id: record.collision_seed.shape_id,
            boxes: None,
            friction: 0.0,
            emission: 0,
            filter: 0,
            can_contain_liquid_source: false,
        }
    }

    #[test]
    fn selectors_reject_empty_expansions_and_stale_property_keys() {
        let source = [torch()];
        let states = [serde_json::from_str(&source[0].state).unwrap()];
        let rule: Rule = serde_json::from_value(serde_json::json!({
            "id":"torch", "fact":"TORCH", "status":"verified", "state_property":"torch_facing_direction"
        })).unwrap();
        assert!(validate(&rule, &source, &[ModelFamily::Torch], &states).is_ok());
        let empty: Rule = serde_json::from_value(serde_json::json!({
            "id":"lantern", "fact":"LANTERN_LIKE", "status":"verified", "state_property":"hanging"
        }))
        .unwrap();
        assert!(validate(&empty, &source, &[ModelFamily::Torch], &states).is_err());
        let stale: Rule = serde_json::from_value(serde_json::json!({
            "id":"lantern", "fact":"LANTERN_LIKE", "status":"verified", "state_property":"hanging_bit"
        })).unwrap();
        assert!(validate(&stale, &source, &[ModelFamily::Torch], &states).is_err());
    }

    #[test]
    fn rules_require_explicit_confidence_and_reject_unrecognized_fields() {
        assert!(serde_json::from_value::<Rule>(serde_json::json!({ "id":"torch", "fact":"TORCH", "state_property":"torch_facing_direction" })).is_err());
        assert!(serde_json::from_value::<Rule>(serde_json::json!({ "id":"torch", "fact":"TORCH", "status":"verified", "suffixes":["torch"] })).is_err());
    }

    #[test]
    fn historical_name_selectors_do_not_change_active_state_classification() {
        let policy: super::super::Policy = serde_json::from_slice(super::super::POLICY).unwrap();
        let torch = torch();
        let state = serde_json::from_str(&torch.state).unwrap();
        let empty = BTreeMap::new();
        let rule = policy
            .model_family_rules
            .iter()
            .find(|r| r.id == "family-torch-22")
            .unwrap();
        assert!(rule.matches(&torch.name, &state, ModelFamily::Unknown));
        assert!(!rule.matches(&torch.name, &empty, ModelFamily::Unknown));
        let shelf = policy
            .model_family_rules
            .iter()
            .find(|r| r.id == "family-shelf")
            .unwrap();
        for name in &shelf.compatibility_exclude_members {
            assert!(shelf.matches(name, &empty, ModelFamily::Unknown));
        }
    }

    #[test]
    fn every_shape_trait_has_a_validated_policy_rule() {
        let policy: super::super::Policy = serde_json::from_slice(super::super::POLICY).unwrap();
        let declared: BTreeSet<_> = policy
            .block_rules
            .iter()
            .filter_map(|rule| rule.fact.strip_prefix("shape:"))
            .collect();
        for (name, _) in ShapeTraits::all().iter_names() {
            assert!(
                declared.contains(name),
                "shape trait {name} has no membership rule"
            );
        }
        for name in declared {
            assert!(
                ShapeTraits::from_name(name).is_some(),
                "unknown shape trait {name}"
            );
        }
    }

    #[test]
    fn every_rule_property_selector_ignores_namespace_spelling() {
        let policy: super::super::Policy = serde_json::from_slice(super::super::POLICY).unwrap();
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        for record in &records {
            let original: BTreeMap<String, Scalar> =
                serde_json::from_str(&record.canonical_state).unwrap();
            let bare: BTreeMap<_, _> = original
                .iter()
                .map(|(name, scalar)| {
                    (
                        name.split_once(':')
                            .map_or(name.as_str(), |(_, bare)| bare)
                            .to_owned(),
                        scalar.clone(),
                    )
                })
                .collect();
            let namespaced: BTreeMap<_, _> = bare
                .iter()
                .map(|(name, scalar)| (format!("minecraft:{name}"), scalar.clone()))
                .collect();
            for rule in policy
                .model_family_rules
                .iter()
                .chain(&policy.block_rules)
                .chain(&policy.placement_rules)
            {
                let expected = rule.matches(&record.name, &original, record.model_family);
                for state in [&bare, &namespaced] {
                    assert_eq!(
                        rule.matches(&record.name, state, record.model_family),
                        expected,
                        "{} {} {}",
                        rule.id,
                        record.name,
                        record.canonical_state
                    );
                }
            }
        }
    }
}
