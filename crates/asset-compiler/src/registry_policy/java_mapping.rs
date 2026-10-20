//! Generates the Java mapping carrier (`assets::facts::java_mapping`) from the pinned GeyserMC
//! mappings: Geyser maps every Java block state to one Bedrock state, so each Bedrock state
//! of the block registry takes one Java state from the inverse, chosen by [`preference`]. The
//! Java biome registry and the block and biome tags come from the generator's MCProtocolLib
//! network exports in the same archive. Java items come from the archive's `items.json`, and
//! Java entity types from the pinned Geyser sources ([`entities`]).

mod entities;
mod nbt;
mod source;

use super::Result;
use assets::facts::java_mapping::{
    self, JavaBiome, JavaBiomeRecord, JavaBiomeTag, JavaBlockMapping, JavaBlockTag, JavaDimension,
    JavaIdentity, JavaMappingPayload, JavaSkyColours,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::Path,
};

pub use source::SourceFiles;

/// The target manifest naming the registries the carrier binds to.
const TARGET_MANIFEST: &str = "assets/bedrock-target.json";

/// How many registry states the generator mapped, for the command's report.
#[derive(Debug)]
pub struct Summary {
    pub states: usize,
    pub mapped_states: usize,
    /// Mapped states that took the Java state of a similar state of the same block.
    pub approximate_states: usize,
    pub biomes: usize,
    pub mapped_biomes: usize,
    /// Entries of the Java biome registry.
    pub java_biomes: usize,
    /// Java entity types and the distinct Bedrock entity ids they map to.
    pub entities: usize,
    pub bedrock_entities: usize,
    /// Java items and the distinct Bedrock item identifiers they map to.
    pub items: usize,
    pub bedrock_items: usize,
}

/// Writes the carrier to `out` and its digest beside it (`.sha256`), reading the archive at
/// `archive`, or the manifest's cache (downloaded and verified when missing).
pub fn write(root: &Path, archive: Option<&Path>, out: &Path) -> Result<Summary> {
    let manifest_bytes = std::fs::read(root.join(java_mapping::SOURCE_MANIFEST))?;
    let files = source::load(root, &manifest_bytes, archive)?;
    let target: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(TARGET_MANIFEST))?)?;
    let artifact = |name: &str| -> Result<Vec<u8>> {
        let path = target["artifacts"][name]
            .as_str()
            .ok_or_else(|| format!("{TARGET_MANIFEST} names no {name}"))?;
        Ok(std::fs::read(root.join(path))?)
    };
    let block_registry = artifact("block_registry")?;
    let biome_registry = artifact("biome_registry")?;
    let (payload, summary) = compile(&files, &block_registry, &biome_registry, &manifest_bytes)?;
    let bytes = JavaBlockMapping::encode(&payload)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, &bytes)?;
    std::fs::write(
        out.with_extension("sha256"),
        format!("{:x}\n", Sha256::digest(&bytes)),
    )?;
    Ok(summary)
}

/// One `blocks_debug.json` value: the Bedrock block (when its name differs from the Java
/// block's) and its complete Bedrock state.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeyserBlock {
    bedrock_identifier: Option<String>,
    #[serde(default)]
    state: BTreeMap<String, Scalar>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(untagged)]
enum Scalar {
    Int(i64),
    Text(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeyserBiome {
    bedrock_id: u32,
}

/// One `items.json` value; only the Bedrock identifier is read.
#[derive(Deserialize)]
struct GeyserItem {
    bedrock_identifier: String,
}

/// The registry-typed Bedrock state of a canonical-state property (`{"type":..,"value":..}`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedScalar {
    r#type: String,
    value: serde_json::Value,
}

/// A Bedrock block state as both sources can spell it: full id and sorted properties.
type BedrockKey = (String, Vec<(String, Scalar)>);

/// Builds the payload from the extracted source files and the bound registries.
pub fn compile(
    files: &SourceFiles,
    block_registry: &[u8],
    biome_registry: &[u8],
    manifest: &[u8],
) -> Result<(JavaMappingPayload, Summary)> {
    let geyser: BTreeMap<String, GeyserBlock> = serde_json::from_slice(&files.blocks)?;
    let mut inverse: HashMap<BedrockKey, Vec<&str>> = HashMap::new();
    for (java, block) in &geyser {
        let (java_name, _) = split_java_state(java)?;
        let bedrock = match &block.bedrock_identifier {
            Some(name) if name.contains(':') => name.clone(),
            Some(name) => format!("minecraft:{name}"),
            None => java_name.to_owned(),
        };
        let state = block
            .state
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        inverse.entry((bedrock, state)).or_default().push(java);
    }

    let protocol = assets::registry_header_protocol(block_registry)?;
    let records = assets::read_registry_for_protocol(block_registry, protocol)?;
    let bedrock = records
        .iter()
        .map(|record| bedrock_state(&record.canonical_state))
        .collect::<Result<Vec<_>>>()?;
    let mut chosen = Vec::with_capacity(records.len());
    for (record, state) in records.iter().zip(&bedrock) {
        let key = (record.name.to_string(), state.clone());
        chosen.push(
            inverse
                .get(&key)
                .map(|candidates| preference(&record.name, candidates))
                .transpose()?,
        );
    }
    let approximate = nearest_states(&records, &bedrock, &mut chosen);

    let mut blocks = BTreeSet::new();
    let mut property_sets = BTreeSet::new();
    for (name, properties) in chosen.iter().flatten() {
        blocks.insert(name.clone());
        property_sets.insert(properties.clone());
    }
    let blocks: Vec<String> = blocks.into_iter().collect();
    let property_sets: Vec<String> = property_sets.into_iter().collect();
    let block_index = |name: &str| {
        blocks
            .binary_search_by(|block| block.as_str().cmp(name))
            .ok()
    };
    let states = chosen
        .iter()
        .map(|state| {
            state.as_ref().map(|(name, properties)| {
                let block = block_index(name).expect("chosen block is interned");
                let set = property_sets
                    .binary_search(properties)
                    .expect("chosen properties are interned");
                (block as u16, set as u32)
            })
        })
        .collect::<Box<[_]>>();

    let tags = block_tags(files, &geyser, &block_index)?;
    let geyser_biomes: BTreeMap<String, GeyserBiome> = serde_json::from_slice(&files.biomes)?;
    let java_biomes = java_biomes(files, &geyser_biomes)?;
    let biome_tags = biome_tags(files, java_biomes.len())?;
    let dimension_colours = dimension_colours(files)?;
    let (biomes, biome_count) = biomes(&geyser_biomes, &java_biomes, biome_registry)?;
    let entities =
        entities::entity_pairs(&files.vanilla_entities, &files.bedrock_entity_definitions)?;
    let (entities, bedrock_entities) = identities(entities)?;
    let items: BTreeMap<String, GeyserItem> = serde_json::from_slice(&files.items)?;
    let (items, bedrock_items) = identities(
        items
            .into_iter()
            .map(|(java, item)| (java, item.bedrock_identifier))
            .collect(),
    )?;
    let summary = Summary {
        states: records.len(),
        mapped_states: states.iter().flatten().count(),
        approximate_states: approximate.len(),
        biomes: biome_count,
        mapped_biomes: biomes.len(),
        java_biomes: java_biomes.len(),
        entities: entities.len(),
        bedrock_entities: bedrock_entities.len(),
        items: items.len(),
        bedrock_items: bedrock_items.len(),
    };
    let payload = JavaMappingPayload {
        protocol,
        block_registry_sha256: Sha256::digest(block_registry).into(),
        biome_registry_sha256: Sha256::digest(biome_registry).into(),
        source_manifest_sha256: Sha256::digest(manifest).into(),
        blocks: blocks.into_iter().map(String::into_boxed_str).collect(),
        property_sets: property_sets
            .into_iter()
            .map(String::into_boxed_str)
            .collect(),
        states,
        approximate,
        tags,
        java_biomes,
        biome_tags,
        dimension_colours,
        biomes,
        entities,
        bedrock_entities,
        items,
        bedrock_items,
    };
    Ok((payload, summary))
}

/// An identity table sorted by Java id and its inverse's indices into it.
type Identities = (Box<[JavaIdentity]>, Box<[u16]>);

/// The identity table of `(Java id, Bedrock id)` pairs given in preference order, and its
/// inverse: each distinct Bedrock id, ascending, takes the pair whose Java id equals it, else
/// the first pair naming it.
fn identities(pairs: Vec<(String, String)>) -> Result<Identities> {
    let mut chosen: BTreeMap<&str, &str> = BTreeMap::new();
    for (java, bedrock) in &pairs {
        let entry = chosen.entry(bedrock.as_str()).or_insert(java.as_str());
        if java == bedrock {
            *entry = java.as_str();
        }
    }
    let mut sorted: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(java, bedrock)| (java.as_str(), bedrock.as_str()))
        .collect();
    sorted.sort_unstable();
    if let Some(pair) = sorted.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(format!("{} is mapped twice", pair[0].0).into());
    }
    let index = |java: &str| -> Result<u16> {
        let found = sorted
            .binary_search_by(|(id, _)| (*id).cmp(java))
            .expect("chosen Java ids are listed");
        Ok(u16::try_from(found)?)
    };
    let inverse = chosen
        .values()
        .map(|java| index(java))
        .collect::<Result<Box<[u16]>>>()?;
    let table = sorted
        .into_iter()
        .map(|(java, bedrock)| (Box::from(java), Box::from(bedrock)))
        .collect();
    Ok((table, inverse))
}

/// Gives each Bedrock state Geyser never produces (such as leaves with `update_bit`, or
/// pressure plates above signal 1) the Java state of the produced state of the same Bedrock
/// block sharing the most property values, the lowest sequential id on ties. Returns the
/// sequential ids filled this way, ascending.
fn nearest_states(
    records: &[assets::RegistryRecord],
    bedrock: &[Vec<(String, Scalar)>],
    chosen: &mut [Option<(String, String)>],
) -> Box<[u32]> {
    let mut exact_by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        if chosen[index].is_some() {
            exact_by_name.entry(&record.name).or_default().push(index);
        }
    }
    let mut approximate = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if chosen[index].is_some() {
            continue;
        }
        let Some(exact) = exact_by_name.get(record.name.as_ref()) else {
            continue;
        };
        let shared = |other: usize| {
            bedrock[index]
                .iter()
                .filter(|property| bedrock[other].contains(property))
                .count()
        };
        let mut best = exact[0];
        for &other in &exact[1..] {
            if shared(other) > shared(best) {
                best = other;
            }
        }
        chosen[index] = chosen[best].clone();
        approximate.push(record.sequential_id);
    }
    approximate.into_boxed_slice()
}

/// The Java state among those Geyser maps to one Bedrock state that the inverse uses:
/// a Java block named like the Bedrock block (`air` over `cave_air`), then a dry state
/// (`waterlogged=false`; Bedrock keeps water in a second layer), then the first in
/// Java block-state text order. Returns the Java id and its bracket-free property list.
fn preference(bedrock: &str, candidates: &[&str]) -> Result<(String, String)> {
    /// Renamed block, wet state, then text: lower ranks win.
    type Rank<'a> = (bool, bool, &'a str);
    let mut best: Option<(Rank, &str, &str)> = None;
    for &candidate in candidates {
        let (name, properties) = split_java_state(candidate)?;
        let wet = properties
            .split(',')
            .any(|property| property == "waterlogged=true");
        let rank = (name != bedrock, wet, candidate);
        if best.is_none_or(|(current, _, _)| rank < current) {
            best = Some((rank, name, properties));
        }
    }
    let (_, name, properties) = best.ok_or("empty Geyser candidate list")?;
    let mut pairs: Vec<&str> = properties.split(',').filter(|p| !p.is_empty()).collect();
    pairs.sort_unstable_by_key(|pair| pair.split_once('=').map_or(*pair, |(key, _)| key));
    Ok((name.to_owned(), pairs.join(",")))
}

/// Splits `minecraft:oak_log[axis=y]` into the id and `axis=y`.
fn split_java_state(state: &str) -> Result<(&str, &str)> {
    match state.split_once('[') {
        Some((name, rest)) => {
            let properties = rest
                .strip_suffix(']')
                .ok_or_else(|| format!("unterminated Java block state {state}"))?;
            Ok((name, properties))
        }
        None => Ok((state, "")),
    }
}

/// Reads a registry canonical state into the sorted, untyped form Geyser uses (bytes and
/// ints are both integers).
fn bedrock_state(canonical: &str) -> Result<Vec<(String, Scalar)>> {
    let typed: BTreeMap<String, TypedScalar> = serde_json::from_str(canonical)?;
    typed
        .into_iter()
        .map(|(key, scalar)| {
            let value = match (scalar.r#type.as_str(), scalar.value) {
                ("byte" | "int", serde_json::Value::Number(number)) => Scalar::Int(
                    number
                        .as_i64()
                        .ok_or_else(|| format!("non-integer state value for {key}"))?,
                ),
                ("string", serde_json::Value::String(text)) => Scalar::Text(text),
                (kind, _) => return Err(format!("unexpected {kind} state value for {key}").into()),
            };
            Ok((key, value))
        })
        .collect()
}

/// Java block tags from the generator's network tag export, keyed by Java block registry
/// order (`Blocks.java`), restricted to blocks the carrier lists.
fn block_tags(
    files: &SourceFiles,
    geyser: &BTreeMap<String, GeyserBlock>,
    block_index: &dyn Fn(&str) -> Option<usize>,
) -> Result<Box<[JavaBlockTag]>> {
    let registry = java_block_registry(&files.java_blocks)?;
    let mapped: BTreeSet<&str> = geyser
        .keys()
        .map(|state| split_java_state(state).map(|(name, _)| name))
        .collect::<Result<_>>()?;
    let listed: BTreeSet<&str> = registry.iter().map(String::as_str).collect();
    if mapped != listed || listed.len() != registry.len() {
        return Err("Blocks.java does not list exactly the blocks of blocks_debug.json".into());
    }
    let tags = nbt::tags(&files.tags, nbt::BLOCK_REGISTRY)?;
    let mut result = Vec::new();
    for (tag, members) in tags {
        let mut indices = members
            .into_iter()
            .map(|id| {
                registry
                    .get(usize::try_from(id).ok()?)
                    .map(|name| block_index(name))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("tag {tag} names an unknown Java block id"))?
            .into_iter()
            .flatten()
            .map(|index| index as u16)
            .collect::<Vec<_>>();
        indices.sort_unstable();
        indices.dedup();
        if !indices.is_empty() {
            result.push((tag.into_boxed_str(), indices.into_boxed_slice()));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result.into_boxed_slice())
}

/// Java block ids in registry order, read from `register(new <Class>("<path>", ...` lines.
fn java_block_registry(source: &str) -> Result<Vec<String>> {
    const MARKER: &str = "register(new ";
    source
        .lines()
        .filter_map(|line| line.split_once(MARKER).map(|(_, rest)| rest))
        .map(|rest| {
            let path = rest
                .split_once("(\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(path, _)| path)
                .filter(|path| !path.is_empty())
                .ok_or_else(|| format!("unreadable Blocks.java registration: {rest}"))?;
            Ok(format!("minecraft:{path}"))
        })
        .collect()
}

/// The Java biome registry of the network codec, in network registry order. Its ids must run
/// 0..n in file order and its names must be exactly the Java biomes `biomes.json` maps, so the
/// Bedrock table indexes every Java biome Geyser knows.
fn java_biomes(
    files: &SourceFiles,
    geyser: &BTreeMap<String, GeyserBiome>,
) -> Result<Box<[JavaBiomeRecord]>> {
    let codec = nbt::biomes(&files.codec)?;
    if codec.len() > usize::from(u16::MAX) {
        return Err("the Java biome registry exceeds u16 indices".into());
    }
    if let Some((index, biome)) = codec
        .iter()
        .enumerate()
        .find(|(index, biome)| usize::try_from(biome.id).ok() != Some(*index))
    {
        return Err(format!(
            "Java biome {} has registry id {}, not its position {index}",
            biome.name, biome.id
        )
        .into());
    }
    let names: BTreeSet<&str> = codec.iter().map(|biome| biome.name.as_str()).collect();
    if names.len() != codec.len() || !names.iter().copied().eq(geyser.keys().map(String::as_str)) {
        return Err("networkCodec.nbt does not list exactly the Java biomes of biomes.json".into());
    }
    Ok(codec
        .into_iter()
        .map(|biome| JavaBiomeRecord {
            name: biome.name.into_boxed_str(),
            temperature: biome.temperature,
            downfall: biome.downfall,
            has_precipitation: biome.has_precipitation,
            frozen: biome.frozen,
            colours: biome.colours,
        })
        .collect())
}

/// The colours of every [`JavaDimension`] type in the network codec, in
/// [`JavaDimension::ALL`] order.
fn dimension_colours(files: &SourceFiles) -> Result<[JavaSkyColours; 3]> {
    let codec = nbt::dimensions(&files.codec)?;
    let mut colours = [JavaSkyColours::default(); 3];
    for (slot, dimension) in colours.iter_mut().zip(JavaDimension::ALL) {
        *slot = codec
            .iter()
            .find(|entry| entry.name == dimension.identifier())
            .ok_or_else(|| {
                format!(
                    "networkCodec.nbt has no dimension type {}",
                    dimension.identifier()
                )
            })?
            .colours;
    }
    Ok(colours)
}

/// Java biome tags from the network tag export, members as Java biome registry indices
/// (ascending, unique); tags without members are left out.
fn biome_tags(files: &SourceFiles, java_biomes: usize) -> Result<Box<[JavaBiomeTag]>> {
    let mut result = Vec::new();
    for (tag, members) in nbt::tags(&files.tags, nbt::BIOME_REGISTRY)? {
        let mut indices = members
            .into_iter()
            .map(|id| {
                u16::try_from(id)
                    .ok()
                    .filter(|&index| usize::from(index) < java_biomes)
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("biome tag {tag} names an unknown Java biome id"))?;
        indices.sort_unstable();
        indices.dedup();
        if !indices.is_empty() {
            result.push((tag.into_boxed_str(), indices.into_boxed_slice()));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result.into_boxed_slice())
}

/// Bedrock biome id to Java biome index: Geyser maps Java biomes to Bedrock ids, so a Bedrock
/// id several Java biomes share takes the one named like the Bedrock biome, else the first by
/// name. Returns the table and the Bedrock registry's biome count.
fn biomes(
    geyser: &BTreeMap<String, GeyserBiome>,
    java_biomes: &[JavaBiomeRecord],
    biome_registry: &[u8],
) -> Result<(Box<[JavaBiome]>, usize)> {
    let java_index: HashMap<&str, u16> = java_biomes
        .iter()
        .enumerate()
        .map(|(index, biome)| (biome.name.as_ref(), index as u16))
        .collect();
    let records = assets::read_biome_registry(biome_registry)?;
    let mut table = Vec::new();
    for record in records.iter() {
        let mut candidates = geyser
            .iter()
            .filter(|(_, biome)| biome.bedrock_id == record.id)
            .map(|(name, _)| name.as_str());
        let first = candidates.next();
        let chosen = std::iter::once(first)
            .flatten()
            .chain(candidates)
            .find(|name| *name == record.name.as_ref())
            .or(first);
        if let Some(name) = chosen {
            let index = *java_index
                .get(name)
                .ok_or_else(|| format!("biomes.json maps {name}, absent from the registry"))?;
            table.push((record.id, index));
        }
    }
    table.sort_by_key(|&(id, _)| id);
    Ok((table.into_boxed_slice(), records.len()))
}

#[cfg(test)]
mod tests;
