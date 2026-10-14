//! Compile block-entity routing against the active registry, never fixed runtime ids.

use super::{Result, check, digest, manifest, read};
use assets::{RegistryRecord, facts::*, registry::ModelFamily};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rule {
    entry: BlockEntityEntry,
    status: assets::facts::rules::Status,
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    suffixes: Vec<String>,
    family: Option<String>,
    state_property: Option<String>,
    exclude_property: Option<String>,
    #[serde(default)]
    exclude_members: Vec<String>,
}

impl Rule {
    /// Evaluates reviewed identity selectors and property filters only during compilation.
    fn matches(&self, record: &RegistryRecord) -> bool {
        !self
            .exclude_members
            .iter()
            .any(|name| name == record.name.as_ref())
            && self.state_property.as_ref().is_none_or(|p| {
                record
                    .property(BlockProperty::from_name(p).unwrap())
                    .is_some()
            })
            && self.exclude_property.as_ref().is_none_or(|p| {
                record
                    .property(BlockProperty::from_name(p).unwrap())
                    .is_none()
            })
            && (self.members.iter().any(|name| name == record.name.as_ref())
                || self
                    .suffixes
                    .iter()
                    .any(|suffix| record.name.ends_with(suffix))
                || self.family.as_ref().is_some_and(|family| {
                    ModelFamily::from_name(family) == Some(record.model_family)
                }))
    }

    /// Rejects misspelled identities, property keys and empty selectors before expansion.
    fn validate(&self, records: &[RegistryRecord]) -> Result<()> {
        check(
            self.entry.backing_blocks.is_empty() && !self.entry.provisional,
            "routing rules cannot supply generated backing facts",
        )?;
        for name in self.members.iter().chain(&self.exclude_members) {
            check(
                records.iter().any(|r| r.name.as_ref() == name),
                format!("{}: unknown backing {name}", self.entry.nbt_id),
            )?;
        }
        for property in self.state_property.iter().chain(&self.exclude_property) {
            check(
                BlockProperty::from_name(property).is_some(),
                format!("unknown block-entity property {property}"),
            )?;
        }
        if let Some(family) = &self.family {
            check(
                ModelFamily::from_name(family).is_some(),
                format!("unknown block-entity family {family}"),
            )?;
        }
        check(!self.entry.nbt_id.is_empty(), "empty block-entity id")?;
        for suffix in &self.suffixes {
            check(
                !suffix.is_empty() && records.iter().any(|r| r.name.ends_with(suffix)),
                format!("{}: empty suffix {suffix}", self.entry.nbt_id),
            )?;
        }
        Ok(())
    }
}

/// Derives sign and banner mounts from actual state properties instead of block-name suffixes.
fn mount(record: &RegistryRecord, renderer: BlockEntityRenderer) -> BlockEntityMount {
    if !matches!(
        renderer,
        BlockEntityRenderer::Sign | BlockEntityRenderer::Banner
    ) {
        return BlockEntityMount::None;
    }
    if record.property(BlockProperty::AttachedBit).is_some() {
        BlockEntityMount::Hanging
    } else if record
        .property(BlockProperty::GroundSignDirection)
        .is_some()
    {
        BlockEntityMount::Standing
    } else {
        BlockEntityMount::Wall
    }
}

/// Expands every route once and rejects overlapping backing-state ownership.
fn compile(
    records: &[RegistryRecord],
    rules: &[Rule],
    registry: &[u8],
    protocol: u32,
) -> Result<BlockEntityFactsPayload> {
    let mut entries = Vec::new();
    let mut states = Vec::new();
    let mut claimed = BTreeSet::new();
    let mut rules: Vec<_> = rules.iter().collect();
    rules.sort_by(|a, b| a.entry.nbt_id.cmp(&b.entry.nbt_id));
    for rule in rules {
        rule.validate(records)?;
        check(
            entries
                .last()
                .is_none_or(|e: &BlockEntityEntry| e.nbt_id != rule.entry.nbt_id),
            "duplicate block-entity id",
        )?;
        let selected: Vec<_> = records
            .iter()
            .filter(|record| rule.matches(record))
            .collect();
        check(
            !selected.is_empty(),
            format!("{} has no backing states", rule.entry.nbt_id),
        )?;
        let names: BTreeSet<_> = selected.iter().map(|r| r.name.to_string()).collect();
        for record in selected {
            check(
                claimed.insert(record.sequential_id),
                format!("{} has overlapping block-entity routes", record.name),
            )?;
            states.push(BlockEntityState {
                sequential_id: record.sequential_id,
                network_hash: record.network_hash,
                entry: entries.len(),
                mount: mount(record, rule.entry.renderer),
            });
        }
        let mut entry = rule.entry.clone();
        entry.provisional = matches!(rule.status, assets::facts::rules::Status::Provisional);
        entry.backing_blocks = names.into_iter().collect();
        entries.push(entry);
    }
    states.sort_by_key(|state| state.sequential_id);
    Ok(BlockEntityFactsPayload {
        schema: 1,
        protocol,
        registry_sha256: digest(registry),
        entries,
        states,
    })
}

/// Writes one table bound to the supplied active registry and the shared rules file.
pub fn write(root: &Path, registry: &Path, out: &Path) -> Result<()> {
    let bytes = read(registry)?;
    let protocol = assets::registry_header_protocol(&bytes)?;
    let records = assets::read_registry_for_protocol(&bytes, protocol)?;
    let policy: super::Policy = serde_json::from_slice(&read(
        &root.join("crates/asset-compiler/data/registry-policy.json"),
    )?)?;
    let payload = compile(&records, &policy.block_entity_rules, &bytes, protocol)?;
    let encoded = serde_json::to_vec(&payload)?;
    BlockEntityFacts::decode(&encoded, &bytes)?;
    let mut encoded = encoded;
    encoded.push(b'\n');
    manifest::write(out, &encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses the real pinned registry and reviewed rules for each compiler mutation.
    fn fixture() -> (Vec<RegistryRecord>, super::super::Policy) {
        let bytes = assets::pinned_block_registry_bytes();
        let records =
            assets::read_registry_for_protocol(bytes, assets::active_content_registry_protocol())
                .unwrap();
        (
            records.into_vec(),
            serde_json::from_slice(super::super::POLICY).unwrap(),
        )
    }

    #[test]
    fn block_entity_compiler_rejects_overlap_and_missing_members() {
        let (records, mut policy) = fixture();
        let registry = assets::pinned_block_registry_bytes();
        let protocol = assets::active_content_registry_protocol();
        policy.block_entity_rules[0]
            .members
            .push("minecraft:barrel".into());
        assert!(
            compile(&records, &policy.block_entity_rules, registry, protocol)
                .unwrap_err()
                .to_string()
                .contains("overlapping")
        );
        let (_, mut policy) = fixture();
        policy.block_entity_rules[0].members.clear();
        assert!(
            compile(&records, &policy.block_entity_rules, registry, protocol)
                .unwrap_err()
                .to_string()
                .contains("no backing states")
        );
    }

    #[test]
    fn block_entity_compiler_uses_registry_identities_after_reordering() {
        let (mut records, policy) = fixture();
        records.reverse();
        for (id, record) in records.iter_mut().enumerate() {
            record.sequential_id = id as u32;
        }
        let output = compile(
            &records,
            &policy.block_entity_rules,
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        for state in output.states {
            let record = &records[state.sequential_id as usize];
            assert_eq!(record.network_hash, state.network_hash);
            assert!(
                output.entries[state.entry]
                    .backing_blocks
                    .iter()
                    .any(|name| name == record.name.as_ref())
            );
        }
    }
}
