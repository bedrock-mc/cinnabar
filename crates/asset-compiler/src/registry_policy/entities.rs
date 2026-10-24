//! Compiles entity components and reviewed native rules against the pinned inventory.

use super::{Policy, Result, check, read};
use assets::facts::entity::{EntityFact, EntityFactRecord, EntityFacts, NativeEntityClass};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const COMPONENTS_PATH: &str = "crates/asset-compiler/data/entity-components-1.26.50.json";
const ACTORS_PATH: &str = "crates/protocol/data/retail_actors_1_26_50.txt";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rule {
    id: String,
    status: Status,
    members: Vec<String>,
    facts: BTreeMap<String, serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Verified,
    Provisional,
}

impl Rule {
    /// Applies declared fields through the canonical record and rejects overlapping assignments.
    fn apply(&self, fact: &mut EntityFact, assigned: &mut BTreeSet<String>) -> Result<()> {
        check(!self.facts.is_empty(), "entity rule has no facts")?;
        let mut value = serde_json::to_value(*fact)?;
        let fields = value
            .as_object_mut()
            .ok_or("entity fact must be an object")?;
        for (field, value) in &self.facts {
            check(
                fields.contains_key(field),
                format!("unknown entity field {field} in {}", self.id),
            )?;
            check(
                !matches!(
                    field.as_str(),
                    "class_key" | "collision" | "collidable" | "provisional"
                ),
                "native rule cannot override a derived field",
            )?;
            check(
                assigned.insert(field.clone()),
                format!("overlapping entity field {field} in {}", self.id),
            )?;
            fields.insert(field.clone(), value.clone());
        }
        *fact = serde_json::from_value(value)?;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ComponentSource {
    source_manifest_sha256: [u8; 32],
    entries: BTreeMap<String, Components>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Components {
    projectile: bool,
    collidable: bool,
    collision: Option<[f32; 2]>,
}

/// Verifies and unpacks the pinned archive before deriving its entity components.
pub fn components_from_archive(archive: &Path) -> Result<Vec<u8>> {
    let source = assets::vanilla_source();
    let scratch = tempfile::tempdir()?;
    let paths = source.local_paths(scratch.path())?;
    std::fs::create_dir_all(paths.archive.parent().ok_or("archive parent missing")?)?;
    std::fs::copy(archive, &paths.archive)?;
    check(
        assets::vanilla_pack::sha256_file(&paths.archive)? == source.sha256.as_ref(),
        "entity component archive hash mismatch",
    )?;
    assets::vanilla_pack::unpack(&paths, &assets::vanilla_pack::UnpackLimits::PINNED, &|| {
        false
    })?;
    components(&paths.cache)
}

/// Extracts only component facts from the pinned behavior definitions.
pub fn components(samples: &Path) -> Result<Vec<u8>> {
    let mut entries = BTreeMap::new();
    for path in super::super::vocabulary::json_files(&samples.join("behavior_pack/entities"))? {
        let document = super::super::vocabulary::read(&path)?;
        let entity = &document["minecraft:entity"];
        let identifier = entity["description"]["identifier"]
            .as_str()
            .ok_or("entity identifier missing")?;
        let mut projectile = false;
        let mut inspect = |components: &serde_json::Value| {
            projectile |= components.get("minecraft:projectile").is_some()
                || components["minecraft:type_family"]["family"]
                    .as_array()
                    .is_some_and(|family| family.iter().any(|v| v.as_str() == Some("projectile")));
        };
        inspect(&entity["components"]);
        if let Some(groups) = entity["component_groups"].as_object() {
            for group in groups.values() {
                inspect(group);
            }
        }
        let collision = &entity["components"]["minecraft:collision_box"];
        let collision = collision["width"]
            .as_f64()
            .zip(collision["height"].as_f64())
            .map(|(w, h)| [w as f32, h as f32])
            .filter(|v| v.iter().all(|n| n.is_finite() && *n > 0.0));
        check(
            entries
                .insert(
                    identifier.to_owned(),
                    Components {
                        projectile,
                        collidable: entity["components"]
                            .get("minecraft:is_collidable")
                            .is_some(),
                        collision,
                    },
                )
                .is_none(),
            "duplicate component entity",
        )?;
    }
    let source = ComponentSource {
        source_manifest_sha256: assets::canonical_source_manifest_sha256(
            assets::VANILLA_SOURCE_MANIFEST.as_bytes(),
        ),
        entries,
    };
    Ok(serde_json::to_vec_pretty(&source)?
        .into_iter()
        .chain(*b"\n")
        .collect())
}

/// Writes deterministic entity facts, their checksum and the expanded native-rule membership.
pub fn compile(root: &Path, out: &Path, policy_bytes: &[u8]) -> Result<()> {
    let policy: Policy = serde_json::from_slice(policy_bytes)?;
    let actors = read(&root.join(ACTORS_PATH))?;
    let mut names: BTreeSet<String> = std::str::from_utf8(&actors)?
        .lines()
        .map(str::to_owned)
        .collect();
    check(
        names.len() == std::str::from_utf8(&actors)?.lines().count(),
        "duplicate actor inventory",
    )?;
    for name in &policy.entity_compatibility {
        check(
            names.insert(name.clone()),
            "duplicate entity compatibility member",
        )?;
    }
    let components: ComponentSource = serde_json::from_slice(&read(&root.join(COMPONENTS_PATH))?)?;
    check(
        components.source_manifest_sha256
            == assets::canonical_source_manifest_sha256(assets::VANILLA_SOURCE_MANIFEST.as_bytes()),
        "entity component source mismatch",
    )?;
    check(
        components.entries.keys().all(|name| names.contains(name)),
        "unknown behavior entity",
    )?;
    let mut facts = BTreeMap::new();
    for (i, name) in names.iter().enumerate() {
        let component = components.entries.get(name);
        facts.insert(
            name.clone(),
            EntityFact {
                native_class: NativeEntityClass::Ordinary,
                class_key: u16::try_from(i + 1)?,
                projectile: component.is_some_and(|v| v.projectile),
                collidable: component.is_some_and(|v| v.collidable),
                collision: component.and_then(|v| v.collision),
                ..EntityFact::default()
            },
        );
    }
    let mut ids = BTreeSet::new();
    let mut assigned: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut memberships: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rule in &policy.entity_rules {
        check(
            ids.insert(&rule.id) && !rule.members.is_empty(),
            "duplicate or empty entity rule",
        )?;
        let mut members = BTreeSet::new();
        for name in &rule.members {
            check(members.insert(name), "duplicate entity rule member")?;
            let fact = facts
                .get_mut(name)
                .ok_or_else(|| format!("unknown entity {name} in {}", rule.id))?;
            check(
                !(components.entries.get(name).is_some_and(|v| v.projectile)
                    && rule.facts.get("projectile") == Some(&serde_json::Value::Bool(false))),
                "native rule cannot erase a projectile component",
            )?;
            rule.apply(fact, assigned.entry(name.clone()).or_default())?;
            fact.provisional |= matches!(rule.status, Status::Provisional);
            memberships
                .entry(name.clone())
                .or_default()
                .push(rule.id.clone());
        }
    }
    check(
        names.iter().all(|name| {
            assigned.get(name).is_some_and(|fields| {
                fields.contains("interactions") && fields.contains("actor_use_resolver")
            })
        }),
        "every entity needs explicit targeting and use resolver dispositions",
    )?;
    let mut expansion = String::from(
        "identifier\tclass_key\tnative_class\tprojectile\tprovisional\trules\tfacts\n",
    );
    for (name, fact) in &facts {
        expansion.push_str(&format!(
            "{name}\t{}\t{:?}\t{}\t{}\t{}\t{}\n",
            fact.class_key,
            fact.native_class,
            fact.projectile,
            fact.provisional,
            memberships.get(name).map_or(String::new(), |v| v.join(",")),
            serde_json::to_string(fact)?
        ));
    }
    let facts = EntityFacts {
        protocol: assets::active_content_registry_protocol(),
        actors_sha256: <sha2::Sha256 as sha2::Digest>::digest(&actors).into(),
        records: facts
            .into_iter()
            .map(|(identifier, fact)| EntityFactRecord {
                identifier: identifier.into(),
                fact,
            })
            .collect(),
    };
    let bytes = facts.encode()?;
    super::manifest::write_blob(&out.join(assets::facts::entity::SOURCE_PATH), &bytes, false)?;
    super::manifest::write(
        &out.join(assets::facts::entity::EXPANSION_PATH),
        expansion.as_bytes(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
