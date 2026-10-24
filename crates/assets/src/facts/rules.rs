//! Shared block selectors evaluated at compilation and historical registry admission.

use super::{BlockProperty, shape::ShapeTraits};
use crate::{
    BlockFlags, RegistryRecord,
    registry::{BlockTraits, ModelFamily, SurvivalRule},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

/// The single policy source used by the registry compiler and historical decoder.
pub const BLOCK_POLICY_BYTES: &[u8] =
    include_bytes!("../../../asset-compiler/data/registry-policy.json");

/// A canonical scalar with its wire kind retained for compiler projections.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scalar {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: serde_json::Value,
}

/// The block subset of the shared registry policy.
#[derive(Deserialize)]
pub struct BlockPolicy {
    pub block_rules: Vec<Rule>,
    pub placement_rules: Vec<Rule>,
}

/// Loads embedded selectors once for compatibility admission.
pub fn policy() -> &'static BlockPolicy {
    static POLICY: OnceLock<BlockPolicy> = OnceLock::new();
    POLICY.get_or_init(|| serde_json::from_slice(BLOCK_POLICY_BYTES).expect("valid block policy"))
}

/// Evidence status carried by a compiled membership rule.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Verified,
    Provisional,
}

/// A bounded registry selector and the fact it assigns.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub fact: String,
    pub model_variant: Option<super::BlockModelVariant>,
    pub status: Status,
    #[serde(default = "enabled")]
    pub value: bool,
    #[serde(default)]
    pub members: Vec<String>,
    pub suffix: Option<String>,
    pub prefix: Option<String>,
    pub contains: Option<String>,
    pub state_property: Option<String>,
    #[serde(default)]
    pub state_properties: Vec<String>,
    #[serde(default)]
    pub exact_properties: bool,
    #[serde(default)]
    pub state_values: BTreeMap<String, serde_json::Value>,
    pub state_maximum: Option<String>,
    #[serde(default)]
    pub state_minimum: BTreeMap<String, i64>,
    #[serde(default)]
    pub model_family: Vec<String>,
    #[serde(default)]
    pub exclude_members: Vec<String>,
    pub exclude_contains: Option<String>,
    /// Name-only compatibility selectors are used by the historical Go generator.
    #[serde(default)]
    pub compatibility_members: Vec<String>,
    #[serde(default)]
    pub compatibility_exclude_members: Vec<String>,
}

/// Rules add their fact unless an explicit exclusion declares otherwise.
fn enabled() -> bool {
    true
}

/// Treats namespaced and unnamespaced property spellings as the same selector key.
fn bare_identifier(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, bare)| bare)
}

/// Finds a state scalar using the selector's namespace-independent property name.
fn property<'a>(state: &'a BTreeMap<String, Scalar>, key: &str) -> Option<&'a Scalar> {
    state
        .iter()
        .find(|(name, _)| bare_identifier(name) == bare_identifier(key))
        .map(|(_, scalar)| scalar)
}

/// Resolves selector domains with the same property-name contract as state matching.
pub fn property_key(name: &str) -> Option<BlockProperty> {
    BlockProperty::from_name(name).or_else(|| {
        BlockProperty::ALL
            .iter()
            .copied()
            .find(|key| bare_identifier(key.name()) == bare_identifier(name))
    })
}

impl Rule {
    /// Evaluates admission selectors with exclusions and state values as filters.
    pub fn matches(
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
                property(state, key)
                    .and_then(|s| s.value.as_i64())
                    .is_none_or(|value| value < *minimum)
            })
            || self.state_maximum.as_ref().is_some_and(|key| {
                let maximum =
                    property_key(key).and_then(|p| match super::block_property_domain(name, p) {
                        super::PropertyDomain::Int(values) => values.iter().max().copied(),
                        _ => None,
                    });
                property(state, key).and_then(|s| s.value.as_i64()) != maximum.map(i64::from)
            })
            || self
                .state_values
                .iter()
                .any(|(key, value)| property(state, key).is_none_or(|s| &s.value != value))
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
                .is_some_and(|s| property(state, s).is_some())
            || (!self.state_properties.is_empty()
                && self
                    .state_properties
                    .iter()
                    .all(|s| property(state, s).is_some())
                && (!self.exact_properties || state.len() == self.state_properties.len()))
            || self
                .model_family
                .iter()
                .any(|s| ModelFamily::from_name(s) == Some(family))
    }
}

/// Gives state-derived memberships priority over lower-order identifier rules.
fn intrinsic(state: &BTreeMap<String, Scalar>, family: ModelFamily) -> BlockTraits {
    let mut traits = BlockTraits::empty();
    traits.set(
        BlockTraits::TORCH,
        property(state, "torch_facing_direction").is_some(),
    );
    traits.set(
        BlockTraits::MULTIFACE,
        property(state, "multi_face_direction_bits").is_some(),
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
        property(state, "open_bit").is_some() && property(state, "upper_block_bit").is_some(),
    );
    traits.set(
        BlockTraits::LANTERN_LIKE,
        state.len() == 1 && property(state, "hanging").is_some(),
    );
    traits
}

/// Reads the component derivative once for registry compilation and compatibility decoding.
pub fn components() -> &'static BTreeMap<String, BlockTraits> {
    static FACTS: OnceLock<BTreeMap<String, BlockTraits>> = OnceLock::new();
    FACTS.get_or_init(|| {
        let mut facts = BTreeMap::new();
        for row in include_str!("../../data/block-components-v2193.tsv")
            .lines()
            .skip(1)
        {
            let (name, components) = row.split_once('\t').expect("valid block component row");
            let mut traits = BlockTraits::empty();
            for component in components.split(',').filter(|s| !s.is_empty() && *s != "-") {
                traits |= match component {
                    "minecraft:replaceable" => BlockTraits::REPLACEABLE,
                    "minecraft:flammable" => BlockTraits::FIRE_FLAMMABLE,
                    _ => panic!("unknown pinned block component"),
                };
            }
            assert!(
                facts.insert(name.into(), traits).is_none(),
                "duplicate component block"
            );
        }
        facts
    })
}

/// Reads backing membership from the existing block-entity carrier without duplicating its names.
pub fn backings() -> &'static BTreeSet<String> {
    static BACKINGS: OnceLock<BTreeSet<String>> = OnceLock::new();
    BACKINGS.get_or_init(|| {
        let document: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../data/block-entities-v1001.json"))
                .expect("pinned block entities");
        document["entries"]
            .as_array()
            .expect("block entity entries")
            .iter()
            .flat_map(|e| e["backing_blocks"].as_array().expect("backing blocks"))
            .map(|b| b["name"].as_str().expect("backing identifier").to_owned())
            .collect()
    })
}

/// Evaluates the shared first-match trait, shape and survival rules for one canonical state.
pub fn evaluate(
    name: &str,
    state: &BTreeMap<String, Scalar>,
    family: ModelFamily,
    flags: BlockFlags,
    rules: &[Rule],
) -> (BlockTraits, SurvivalRule, ShapeTraits) {
    let mut traits = intrinsic(state, family);
    traits |= components().get(name).copied().unwrap_or_default();
    if backings().contains(name) {
        traits |= BlockTraits::BLOCK_ENTITY_BACKING;
    }
    if flags.contains(BlockFlags::FIRE_TOP_SUPPORT) {
        traits |= BlockTraits::FULL_TOP_SUPPORT;
    }
    let mut decided = traits;
    let mut shapes = ShapeTraits::empty();
    let mut survival = None;
    for rule in rules {
        if !rule.matches(name, state, family) {
            continue;
        }
        if let Some(value) = rule.fact.strip_prefix("shape:") {
            shapes.set(
                ShapeTraits::from_name(value).expect("valid shape rule"),
                rule.value,
            );
        } else if let Some(value) = rule.fact.strip_prefix("survival:") {
            if survival.is_none() {
                survival = SurvivalRule::from_name(value);
            }
        } else if rule.fact.starts_with("render:")
            || rule.fact.starts_with("render_tint:")
            || rule.fact.starts_with("render_flag:")
            || rule.fact.starts_with("legacy_alias:")
            || rule.fact == "model_variant"
        {
            continue;
        } else {
            let fact = BlockTraits::from_name(&rule.fact).expect("valid block trait");
            if !decided.contains(fact) {
                traits.set(fact, rule.value);
                decided |= fact;
            }
        }
    }
    let survival = survival.unwrap_or(SurvivalRule::None);
    traits.set(BlockTraits::SURVIVAL_RULE, survival != SurvivalRule::None);
    (traits, survival, shapes)
}

/// Supplies facts omitted by BREG1003 before any consumer receives its records.
pub(crate) fn restore_historical(record: &mut RegistryRecord) -> Result<(), serde_json::Error> {
    if record.name.as_ref() == super::block::RESERVED_BLOCK_NAME {
        record.survival = SurvivalRule::Unresolved;
        return Ok(());
    }
    let state = serde_json::from_str(&record.canonical_state)?;
    (record.traits, record.survival, _) = evaluate(
        &record.name,
        &state,
        record.model_family,
        record.flags,
        &policy().block_rules,
    );
    Ok(())
}
