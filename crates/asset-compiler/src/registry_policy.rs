//! Client admission, movement rules and production registry carriers.

use assets::facts::block::RESERVED_BLOCK_NAME;
use assets::facts::rules::{Rule, Scalar};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::File,
    io::Read,
    path::Path,
};

mod biomes;
pub mod block_entities;
mod block_rules;
mod carriers;
pub mod entities;
pub mod events;
mod format_generated;
pub mod identities;
pub mod items;
pub mod java_mapping;
mod light;
mod manifest;
mod movement;
mod placement;
pub mod presentation;
mod projection;

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    game_version: String,
    protocol: u32,
    source_lock_sha256: String,
    cloudburst_ref: String,
    blocks: Vec<BlockFact>,
    biomes: Vec<BiomeFact>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockFact {
    sequential_id: u32,
    network_hash: u32,
    name: String,
    state: String,
    shape_id: u16,
    boxes: Option<Vec<[f64; 6]>>,
    friction: f64,
    emission: u8,
    filter: u8,
    can_contain_liquid_source: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BiomeFact {
    id: i32,
    has_id: bool,
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    runtime_identifiers: BTreeMap<String, BTreeMap<String, String>>,
    presentation: presentation::Rules,
    entity_compatibility: Vec<String>,
    entity_rules: Vec<entities::Rule>,
    model_family_rules: Vec<Rule>,
    block_rules: Vec<Rule>,
    placement_rules: Vec<Rule>,
    block_entity_rules: Vec<block_entities::Rule>,
    item_rules: Vec<items::ItemRule>,
    #[serde(default)]
    visual_events: events::Rules,
    pack_rules: assets::facts::pack::PackRules,
    twins: BTreeMap<String, Twin>,
    unresolved_retail: Vec<String>,
    movement: Vec<Movement>,
    movement_census: MovementCensus,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Twin {
    twin: String,
    item: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Movement {
    name: String,
    behavior: String,
    states: usize,
}
/// Reviewed classification of the identifiers that no movement rule declares.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MovementCensus {
    /// SHA-256 of the reviewed identifiers, sorted and each followed by a newline.
    identifiers_sha256: String,
    /// Identifiers with vanilla movement behaviour the carrier does not model.
    unmodeled: Vec<String>,
    /// Identifiers whose movement classification is not established.
    unresolved: Vec<String>,
}
const POLICY: &[u8] = assets::facts::rules::BLOCK_POLICY_BYTES;
const MAX_INPUT_BYTES: u64 = 128 * 1024 * 1024;

/// Resolves geometry memberships while retaining the decoded block traits.
pub(crate) fn block_shape_facts(
    record: &assets::RegistryRecord,
) -> (
    assets::registry::BlockTraits,
    assets::facts::shape::ShapeTraits,
) {
    static RULES: std::sync::OnceLock<Policy> = std::sync::OnceLock::new();
    let policy =
        RULES.get_or_init(|| serde_json::from_slice(POLICY).expect("valid registry policy"));
    block_rules::for_record(record, &policy.block_rules)
}

/// Reads a bounded source export without fetching or changing its catalog.
pub fn read_facts(path: &Path) -> Result<Facts> {
    Ok(serde_json::from_slice(&read(path)?)?)
}

/// Reads a compiler input within the existing registry allocation ceiling.
fn read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    check(
        bytes.len() as u64 <= MAX_INPUT_BYTES,
        "registry input exceeds 128 MiB",
    )?;
    Ok(bytes)
}

/// Returns a useful input error when a compiler invariant fails.
fn check(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into().into())
    }
}

/// Names immutable bytes with their lowercase SHA-256 digest.
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Validates the exported library target and source lock before applying product policy.
fn validate(facts: &Facts, manifest: &manifest::BlockManifest) -> Result<()> {
    check(
        facts.protocol == manifest.protocol && facts.game_version == manifest.game_version,
        "source catalog target mismatch",
    )?;
    check(
        facts.source_lock_sha256 == manifest.source.source_lock_sha256
            && facts.cloudburst_ref == manifest.source.cloudburst_ref,
        "source catalog lock mismatch",
    )?;
    check(
        facts.blocks.len() == manifest.projection.states,
        "source palette count mismatch",
    )?;
    let mut hashes = BTreeSet::new();
    for (index, block) in facts.blocks.iter().enumerate() {
        check(
            block.sequential_id as usize == index && hashes.insert(block.network_hash),
            "source palette identity mismatch",
        )?;
    }
    Ok(())
}

/// Compiles the active block projection and its exact light binding into the selected paths.
pub fn blocks(
    root: &Path,
    facts: &Facts,
    legacy: &Path,
    allowlist: &Path,
    out: &Path,
    light_out: &Path,
    report: &Path,
) -> Result<()> {
    let mut manifest: manifest::BlockManifest =
        serde_json::from_slice(&read(&root.join("assets/block-projection-v2193.json"))?)?;
    validate(facts, &manifest)?;
    let allow = read(allowlist)?;
    check(
        digest(&allow) == manifest.allowlist.sha256,
        "retail item allowlist hash mismatch",
    )?;
    let mut allowed = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for line in std::str::from_utf8(&allow)?.lines() {
        let (id, name) = line.split_once('\t').ok_or("invalid retail item row")?;
        check(
            ids.insert(id.parse::<i32>()?)
                && name.starts_with("minecraft:")
                && allowed.insert(name.to_owned()),
            "duplicate or invalid retail item",
        )?;
    }
    let legacy = assets::read_registry(&read(legacy)?)?;
    let policy: Policy = serde_json::from_slice(POLICY)?;
    let (mut records, stats) = projection::project(&facts.blocks, &legacy, &allowed, &policy)?;
    let expansion = block_rules::compile(&facts.blocks, &mut records, &policy, &legacy)?;
    let encoded = carriers::blocks(facts.protocol, &records)?;
    let lights = light::compile(&records, &facts.blocks)?;
    let light_bytes = carriers::lights(
        facts.protocol,
        &encoded,
        &records,
        &lights,
        manifest.light.format.as_bytes(),
    )?;
    manifest.projection = stats;
    manifest.output.format = String::from_utf8(format_generated::REGISTRY_MAGIC.to_vec())?;
    manifest.output.sha256 = digest(&encoded);
    manifest.light.sha256 = digest(&light_bytes);
    manifest::write_blob(out, &encoded, true)?;
    manifest::write_blob(light_out, &light_bytes, true)?;
    let data = out.parent().ok_or("block output has no parent")?;
    manifest::write(&data.join("facts-expansion-v2193.tsv"), &expansion)?;
    manifest::write(
        &data.join("block-placement-v2193.tsv"),
        &placement::compile(&facts.blocks, &records, &policy)?,
    )?;
    manifest::write(
        &data.join("vocabulary-v2193.txt"),
        &block_rules::vocabulary(root, &facts.blocks)?,
    )?;
    manifest::write_json(report, &manifest)
}

/// Compiles the movement seed bound to a hash-checked active block carrier.
pub fn physics(
    root: &Path,
    facts: &Facts,
    breg: &Path,
    out: &Path,
    sha: Option<&Path>,
    report: Option<&Path>,
) -> Result<()> {
    let template: manifest::BlockManifest =
        serde_json::from_slice(&read(&root.join("assets/block-projection-v2193.json"))?)?;
    validate(facts, &template)?;
    let bytes = read(breg)?;
    let target: serde_json::Value =
        serde_json::from_slice(&read(&root.join("assets/bedrock-target.json"))?)?;
    let expected = if let Some(path) = report {
        let manifest: manifest::BlockManifest = serde_json::from_slice(&read(path)?)?;
        manifest.output.sha256
    } else {
        target["hashes"]["block_registry"]
            .as_str()
            .ok_or("missing block registry hash")?
            .to_owned()
    };
    check(
        expected == digest(&bytes),
        "pinned protocol-2193 block registry SHA-256 does not match",
    )?;
    if let Some(path) = report {
        let report: manifest::BlockManifest = serde_json::from_slice(&read(path)?)?;
        check(
            report.projection.denied_count == template.projection.denied_count
                && report.projection.education_states == template.projection.education_states
                && report.output.sha256 == digest(&bytes),
            "physics projection manifest mismatch",
        )?;
    }
    let records = assets::read_registry_for_protocol(&bytes, facts.protocol)?;
    let policy: Policy = serde_json::from_slice(POLICY)?;
    let physics = movement::compile(&records, &facts.blocks, &policy)?;
    let encoded = carriers::physics(facts.protocol, &bytes, &records, &physics)?;
    manifest::write_blob(out, &encoded, false)?;
    if let Some(path) = sha {
        manifest::write(path, format!("{}\n", digest(&encoded)).as_bytes())?;
    }
    Ok(())
}

/// Projects source biomes into the retail name set and writes their deterministic manifest.
pub fn biomes(
    root: &Path,
    facts: &Facts,
    allowlist: &Path,
    out: &Path,
    report: &Path,
) -> Result<()> {
    biomes::compile(root, facts, allowlist, out, report)
}
