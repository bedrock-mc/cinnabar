//! Compile identifier selectors once against the pinned block palette.

use super::{BlockFact, Result, Scalar, check};
use assets::facts::shape::ShapeTraits;
use assets::registry::{BlockTraits, ModelFamily, RegistryRecord, SurvivalRule};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Status {
    Verified,
    Provisional,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rule {
    pub(super) id: String,
    pub(super) fact: String,
    status: Status,
    #[serde(default = "enabled")]
    value: bool,
    #[serde(default)]
    members: Vec<String>,
    suffix: Option<String>,
    prefix: Option<String>,
    contains: Option<String>,
    state_property: Option<String>,
    #[serde(default)]
    state_properties: Vec<String>,
    #[serde(default)]
    exact_properties: bool,
    #[serde(default)]
    state_values: BTreeMap<String, serde_json::Value>,
    state_maximum: Option<String>,
    #[serde(default)]
    state_minimum: BTreeMap<String, i64>,
    #[serde(default)]
    model_family: Vec<String>,
    #[serde(default)]
    exclude_members: Vec<String>,
    exclude_contains: Option<String>,
    /// Name-only compatibility selectors are used by the historical Go generator.
    #[serde(default)]
    compatibility_members: Vec<String>,
    #[serde(default)]
    compatibility_exclude_members: Vec<String>,
}

/// Rules add their fact unless an explicit exclusion declares otherwise.
fn enabled() -> bool {
    true
}

impl Rule {
    /// Evaluates selectors once during compilation, with exclusions and state values as filters.
    pub(super) fn matches(
        &self,
        name: &str,
        state: &BTreeMap<String, Scalar>,
        family: ModelFamily,
    ) -> bool {
        if self.exclude_members.iter().any(|m| m == name)
            || self
                .exclude_contains
                .as_ref()
                .is_some_and(|s| name.contains(s))
            || self.state_minimum.iter().any(|(key, minimum)| {
                state
                    .get(key)
                    .and_then(|s| s.value.as_i64())
                    .is_none_or(|value| value < *minimum)
            })
            || self.state_maximum.as_ref().is_some_and(|key| {
                let maximum =
                    assets::facts::BlockProperty::from_name(key).and_then(|p| match p.domain() {
                        assets::facts::PropertyDomain::Int(values) => values.iter().max().copied(),
                        _ => None,
                    });
                state.get(key).and_then(|s| s.value.as_i64()) != maximum.map(i64::from)
            })
            || self
                .state_values
                .iter()
                .any(|(key, value)| state.get(key).is_none_or(|s| &s.value != value))
        {
            return false;
        }
        self.members.iter().any(|m| m == name)
            || self.suffix.as_ref().is_some_and(|s| name.ends_with(s))
            || self.prefix.as_ref().is_some_and(|s| {
                name.strip_prefix("minecraft:")
                    .unwrap_or(name)
                    .starts_with(s)
            })
            || self.contains.as_ref().is_some_and(|s| name.contains(s))
            || self
                .state_property
                .as_ref()
                .is_some_and(|s| state.contains_key(s))
            || (!self.state_properties.is_empty()
                && self.state_properties.iter().all(|s| state.contains_key(s))
                && (!self.exact_properties || state.len() == self.state_properties.len()))
            || self
                .model_family
                .iter()
                .any(|s| ModelFamily::from_name(s) == Some(family))
    }

    /// Rejects misspelled members, unknown domains and selectors that silently match nothing.
    pub(super) fn validate(
        &self,
        source: &[BlockFact],
        families: &[ModelFamily],
        states: &[BTreeMap<String, Scalar>],
    ) -> Result<()> {
        let names: BTreeSet<_> = source.iter().map(|b| b.name.as_str()).collect();
        check(
            (self.compatibility_members.is_empty()
                && self.compatibility_exclude_members.is_empty())
                || self.fact.starts_with("family:"),
            "compatibility selectors require a model family rule",
        )?;
        for member in self
            .members
            .iter()
            .chain(&self.exclude_members)
            .chain(&self.compatibility_exclude_members)
        {
            check(
                names.contains(member.as_str()),
                format!("{} has unknown block {member}", self.id),
            )?;
        }
        for family in &self.model_family {
            check(
                ModelFamily::from_name(family).is_some(),
                format!("{} has unknown family {family}", self.id),
            )?;
        }
        for key in self
            .state_property
            .iter()
            .chain(&self.state_properties)
            .chain(self.state_values.keys())
            .chain(&self.state_maximum)
            .chain(self.state_minimum.keys())
        {
            check(
                assets::facts::BlockProperty::from_name(key).is_some(),
                format!("{} has unknown property {key}", self.id),
            )?;
        }
        let matched = source
            .iter()
            .zip(families)
            .zip(states)
            .any(|((block, family), state)| self.matches(&block.name, state, *family));
        check(matched, format!("{} matches no pinned blocks", self.id))
    }
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
) -> Result<Vec<u8>> {
    let families: Vec<_> = records.iter().map(|r| r.model_family).collect();
    let states: Vec<BTreeMap<String, Scalar>> = source
        .iter()
        .map(|b| serde_json::from_str(&b.state))
        .collect::<std::result::Result<_, _>>()?;
    let mut ids = BTreeSet::new();
    for rule in policy.model_family_rules.iter().chain(&policy.block_rules) {
        check(ids.insert(&rule.id), "duplicate block rule id")?;
        rule.validate(source, &families, &states)?;
        if let Some(value) = rule.fact.strip_prefix("shape:") {
            check(
                ShapeTraits::from_name(value).is_some(),
                format!("unknown shape trait {value}"),
            )?;
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
    let components = components(source)?;
    let backings = backings();
    let mut expansion =
        BTreeMap::<String, (BlockTraits, ShapeTraits, BTreeSet<String>, BTreeSet<String>)>::new();
    for ((block, record), state) in source.iter().zip(records).zip(&states) {
        let mut traits = intrinsic(state, record.model_family);
        let mut shapes = ShapeTraits::empty();
        let mut survival = None;
        let mut applied = BTreeSet::new();
        let mut provisional = BTreeSet::new();
        let mut decided = BlockTraits::empty();
        if let Some(rule) = policy
            .model_family_rules
            .iter()
            .find(|rule| rule.matches(&block.name, state, record.model_family))
        {
            applied.insert(rule.id.clone());
        }
        if let Some(flags) = components.get(&block.name) {
            traits |= *flags;
            applied.insert("pinned-components".into());
        }
        if backings.contains(block.name.as_str()) {
            traits |= BlockTraits::BLOCK_ENTITY_BACKING;
            applied.insert("block-entity-backings".into());
        }
        decided |= traits;
        if record.flags.contains(assets::BlockFlags::FIRE_TOP_SUPPORT) {
            traits |= BlockTraits::FULL_TOP_SUPPORT;
            decided |= BlockTraits::FULL_TOP_SUPPORT;
        }
        for rule in &policy.block_rules {
            if !rule.matches(&block.name, &state, record.model_family) {
                continue;
            }
            applied.insert(rule.id.clone());
            if matches!(rule.status, Status::Provisional) {
                provisional.insert(rule.id.clone());
            }
            if let Some(value) = rule.fact.strip_prefix("shape:") {
                shapes.set(
                    ShapeTraits::from_name(value).ok_or("unknown shape trait")?,
                    rule.value,
                );
            } else if let Some(value) = rule.fact.strip_prefix("survival:") {
                if survival.is_none() {
                    survival = SurvivalRule::from_name(value);
                }
            } else {
                let fact = BlockTraits::from_name(&rule.fact).ok_or("unknown block trait")?;
                if !decided.contains(fact) {
                    traits.set(fact, rule.value);
                    decided |= fact;
                }
            }
        }
        let survival = survival.unwrap_or(SurvivalRule::None);
        traits.set(BlockTraits::SURVIVAL_RULE, survival != SurvivalRule::None);
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
    let mut text = String::from("identifier\ttraits\trules\tprovisional\n");
    for (name, (traits, shapes, rules, provisional)) in expansion {
        let names = traits
            .iter_names()
            .map(|(name, _)| name.to_owned())
            .chain(shapes.iter_names().map(|(name, _)| format!("shape:{name}")))
            .collect::<Vec<_>>()
            .join(",");
        text.push_str(&format!(
            "{name}\t{names}\t{}\t{}\n",
            rules.into_iter().collect::<Vec<_>>().join(","),
            if provisional.is_empty() {
                "-".to_owned()
            } else {
                provisional.into_iter().collect::<Vec<_>>().join(",")
            }
        ));
    }
    Ok(text.into_bytes())
}

/// Resolves compiler-only shape classes and derives traits for older carriers.
pub(super) fn for_record(record: &RegistryRecord, rules: &[Rule]) -> (BlockTraits, ShapeTraits) {
    let state = serde_json::from_str(&record.canonical_state).unwrap_or_default();
    let historical = record.traits.is_empty();
    let mut traits = if historical {
        intrinsic(&state, record.model_family)
    } else {
        record.traits
    };
    if historical && record.flags.contains(assets::BlockFlags::FIRE_TOP_SUPPORT) {
        traits |= BlockTraits::FULL_TOP_SUPPORT;
    }
    let mut decided = traits;
    let mut shapes = ShapeTraits::empty();
    for rule in rules {
        if let Some(name) = rule.fact.strip_prefix("shape:") {
            if rule.matches(&record.name, &state, record.model_family) {
                shapes.set(
                    ShapeTraits::from_name(name).expect("validated shape rule"),
                    rule.value,
                );
            }
        } else if historical {
            let Some(fact) = BlockTraits::from_name(&rule.fact) else {
                continue;
            };
            if !decided.contains(fact) && rule.matches(&record.name, &state, record.model_family) {
                traits.set(fact, rule.value);
                decided |= fact;
            }
        }
    }
    (traits, shapes)
}

/// Gives state-derived memberships priority over lower-order identifier rules.
fn intrinsic(state: &BTreeMap<String, Scalar>, family: ModelFamily) -> BlockTraits {
    let mut traits = BlockTraits::empty();
    traits.set(
        BlockTraits::TORCH,
        state.contains_key("torch_facing_direction"),
    );
    traits.set(
        BlockTraits::MULTIFACE,
        state.contains_key("multi_face_direction_bits"),
    );
    if state
        .keys()
        .any(|key| key.starts_with("minecraft:connection_"))
    {
        traits |= if family == ModelFamily::Fence {
            BlockTraits::CONNECTS_FENCE
        } else {
            BlockTraits::CONNECTS_THIN
        };
    }
    traits.set(
        BlockTraits::DOOR,
        state.contains_key("open_bit") && state.contains_key("upper_block_bit"),
    );
    traits.set(
        BlockTraits::LANTERN_LIKE,
        state.len() == 1 && state.contains_key("hanging"),
    );
    traits
}

/// Validates the pinned component derivative against the source palette.
fn components(source: &[BlockFact]) -> Result<BTreeMap<String, BlockTraits>> {
    let names: BTreeSet<_> = source.iter().map(|b| b.name.as_str()).collect();
    let mut facts = BTreeMap::new();
    for row in include_str!("../../../assets/data/block-components-v2193.tsv")
        .lines()
        .skip(1)
    {
        let (name, components) = row.split_once('\t').ok_or("invalid block component row")?;
        check(
            names.contains(name),
            format!("unknown component block {name}"),
        )?;
        let mut traits = BlockTraits::empty();
        for component in components.split(',').filter(|s| !s.is_empty() && *s != "-") {
            traits |= match component {
                "minecraft:replaceable" => BlockTraits::REPLACEABLE,
                "minecraft:flammable" => BlockTraits::FIRE_FLAMMABLE,
                _ => return Err("unknown pinned block component".into()),
            };
        }
        check(
            facts.insert(name.into(), traits).is_none(),
            "duplicate component block",
        )?;
    }
    Ok(facts)
}

/// Reads backing membership from the existing block-entity carrier without duplicating its names.
fn backings() -> BTreeSet<String> {
    let document: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../assets/data/block-entities-v1001.json"
    ))
    .expect("pinned block entities");
    document["entries"]
        .as_array()
        .expect("block entity entries")
        .iter()
        .flat_map(|e| e["backing_blocks"].as_array().expect("backing blocks"))
        .map(|b| b["name"].as_str().expect("backing identifier").to_owned())
        .collect()
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
    for row in include_str!("../../../protocol/data/item_tags_dragonfly.tsv")
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
        }
    }

    #[test]
    fn selectors_reject_empty_expansions_and_stale_property_keys() {
        let source = [torch()];
        let states = [serde_json::from_str(&source[0].state).unwrap()];
        let rule: Rule = serde_json::from_value(serde_json::json!({
            "id":"torch", "fact":"TORCH", "status":"verified", "state_property":"torch_facing_direction"
        })).unwrap();
        assert!(
            rule.validate(&source, &[ModelFamily::Torch], &states)
                .is_ok()
        );
        let empty: Rule = serde_json::from_value(serde_json::json!({
            "id":"lantern", "fact":"LANTERN_LIKE", "status":"verified", "state_property":"hanging"
        }))
        .unwrap();
        assert!(
            empty
                .validate(&source, &[ModelFamily::Torch], &states)
                .is_err()
        );
        let stale: Rule = serde_json::from_value(serde_json::json!({
            "id":"lantern", "fact":"LANTERN_LIKE", "status":"verified", "state_property":"hanging_bit"
        })).unwrap();
        assert!(
            stale
                .validate(&source, &[ModelFamily::Torch], &states)
                .is_err()
        );
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
}
