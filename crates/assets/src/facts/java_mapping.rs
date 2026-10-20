//! Java Edition identities of the pinned Bedrock registries, for Java shader packs: every block
//! state of the block registry to one Java block state, Java block tags, the Java biome registry
//! (climate values in network registry order), Java biome tags, Bedrock biome ids to Java
//! biomes, and Bedrock entity and item identifiers to Java entity types and items.
//! `registryc --kind java-mapping` generates the carrier from the pinned GeyserMC mappings and
//! Geyser sources named by [`SOURCE_MANIFEST`].
//!
//! The carrier is optional: consumers treat a carrier that fails validation as "nothing
//! mapped" and keep running.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, sync::LazyLock};
use thiserror::Error;

pub const FILENAME: &str = "java-mapping-v2193.bin";
pub const SOURCE_PATH: &str = "crates/assets/data/java-mapping-v2193.bin";
/// The pinned GeyserMC source the generator reads.
pub const SOURCE_MANIFEST: &str = "assets/geyser-mappings-source.json";
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"JAVAMAP4";
const SOURCE_BYTES: &[u8] = include_bytes!("../../data/java-mapping-v2193.bin");
const SOURCE_SHA256: &str = include_str!("../../data/java-mapping-v2193.sha256");
const BIOME_REGISTRY_BYTES: &[u8] = include_bytes!("../../data/biome-registry-v2193.bin");
const SOURCE_MANIFEST_BYTES: &[u8] =
    include_bytes!("../../../../assets/geyser-mappings-source.json");

/// The Java Edition release the pinned mappings describe: `java_edition` of
/// [`SOURCE_MANIFEST`], e.g. `26.3`. Java shader packs see it as `MC_VERSION`.
pub fn java_edition() -> &'static str {
    #[derive(Deserialize)]
    struct Manifest {
        java_edition: Box<str>,
    }
    static EDITION: LazyLock<Box<str>> = LazyLock::new(|| {
        serde_json::from_slice::<Manifest>(SOURCE_MANIFEST_BYTES)
            .map(|m| m.java_edition)
            .expect("the checked-in GeyserMC source manifest names its Java edition")
    });
    &EDITION
}

/// A carrier error always names the failed validation.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("invalid Java mapping carrier ({0}); rebuild with registryc --kind java-mapping")]
pub struct JavaMappingError(String);

/// The serialized carrier content, exactly as the generator writes it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JavaMappingPayload {
    pub protocol: u32,
    /// SHA-256 of the block registry whose sequential ids index `states`.
    pub block_registry_sha256: [u8; 32],
    /// SHA-256 of the biome registry whose ids `biomes` maps.
    pub biome_registry_sha256: [u8; 32],
    /// SHA-256 of the source manifest the carrier was generated from.
    pub source_manifest_sha256: [u8; 32],
    /// Java block ids (`namespace:path`), sorted and unique.
    pub blocks: Box<[Box<str>]>,
    /// Java property lists in block-state syntax without brackets (`axis=x,waterlogged=false`),
    /// keys sorted within each list; sorted and unique, the empty list included when used.
    pub property_sets: Box<[Box<str>]>,
    /// Per block-registry sequential id: the Java block index and property-list index, or
    /// `None` when the state has no Java counterpart.
    pub states: Box<[Option<(u16, u32)>]>,
    /// Sequential ids, ascending, of mapped states Geyser never produces: they carry the Java
    /// state of the produced state of the same Bedrock block sharing the most properties.
    pub approximate: Box<[u32]>,
    /// Java block tags (`namespace:path`, sorted, unique) with their member block indices,
    /// ascending; only blocks some state maps to are listed.
    pub tags: Box<[JavaBlockTag]>,
    /// The Java biome registry in network registry order: an entry's index is its Java registry
    /// id. Names are unique.
    pub java_biomes: Box<[JavaBiomeRecord]>,
    /// Java biome tags (`namespace:path`, sorted, unique) with their member Java biome indices,
    /// ascending.
    pub biome_tags: Box<[JavaBiomeTag]>,
    /// The colours each [`JavaDimension`] type sets, in [`JavaDimension::ALL`] order: what a
    /// biome that leaves a colour unset shows in that dimension.
    pub dimension_colours: [JavaSkyColours; JavaDimension::ALL.len()],
    /// Bedrock biome id to Java biome index, sorted by Bedrock id.
    pub biomes: Box<[JavaBiome]>,
    /// Every Java entity type Geyser registers with the Bedrock entity id it spawns, sorted by
    /// Java id, Java ids unique.
    pub entities: Box<[JavaIdentity]>,
    /// One index into `entities` per distinct Bedrock entity id, ascending by that id: the
    /// Java type the Bedrock entity stands for (the same-named one, else the first Geyser
    /// registers).
    pub bedrock_entities: Box<[u16]>,
    /// Every Java item with its Bedrock item identifier, sorted by Java id, Java ids unique.
    pub items: Box<[JavaIdentity]>,
    /// One index into `items` per distinct Bedrock item identifier, ascending by that id: the
    /// Java item it stands for (the same-named one, else the first by Java id).
    pub bedrock_items: Box<[u16]>,
}

/// A Java id (`namespace:path`) and the Bedrock identifier Geyser maps it to.
pub type JavaIdentity = (Box<str>, Box<str>);

/// A Java block tag id (`namespace:path`) and its member block indices, ascending.
pub type JavaBlockTag = (Box<str>, Box<[u16]>);

/// A Java biome tag id (`namespace:path`) and its member Java biome indices, ascending.
pub type JavaBiomeTag = (Box<str>, Box<[u16]>);

/// A Bedrock biome id and the index of its Java biome in [`JavaMappingPayload::java_biomes`].
pub type JavaBiome = (u32, u16);

/// The sky, fog and water fog colours (`0xRRGGBB`) a Java biome or dimension type sets
/// through its `minecraft:visual/*_color` environment attributes; `None` leaves the colour
/// to the layer below (biome over dimension type over the attribute's default).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JavaSkyColours {
    pub sky: Option<u32>,
    pub fog: Option<u32>,
    pub water_fog: Option<u32>,
}

impl JavaSkyColours {
    /// Each colour of `self`, else of `below`.
    pub fn over(self, below: Self) -> Self {
        Self {
            sky: self.sky.or(below.sky),
            fog: self.fog.or(below.fog),
            water_fog: self.water_fog.or(below.water_fog),
        }
    }

    fn valid(self) -> bool {
        [self.sky, self.fog, self.water_fog]
            .into_iter()
            .flatten()
            .all(|colour| colour <= 0xFF_FF_FF)
    }
}

/// Declares dimension variants, carrier order and registry identifiers together.
macro_rules! java_dimensions {
    ($($variant:ident => $identifier:literal),+ $(,)?) => {
        /// The Java dimension types whose colours the carrier keeps.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum JavaDimension {
            $($variant),+
        }

        impl JavaDimension {
            /// Every kept dimension type, in [`JavaMappingPayload::dimension_colours`] order.
            pub const ALL: [Self; [$(stringify!($variant)),+].len()] = [$(Self::$variant),+];

            /// The dimension type's registry id.
            pub const fn identifier(self) -> &'static str {
                match self {
                    $(Self::$variant => $identifier),+
                }
            }
        }
    };
}

java_dimensions! {
    Overworld => "minecraft:overworld",
    Nether => "minecraft:the_nether",
    End => "minecraft:the_end",
}

/// One serialized Java biome registry entry; [`JavaBiomeEntry`] is its borrowed view.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JavaBiomeRecord {
    /// The biome id, e.g. `minecraft:plains`.
    pub name: Box<str>,
    /// The base temperature, before any temperature modifier.
    pub temperature: f32,
    pub downfall: f32,
    pub has_precipitation: bool,
    /// Whether the biome's temperature modifier is `frozen`.
    pub frozen: bool,
    /// The colours the biome sets over its dimension type's.
    pub colours: JavaSkyColours,
}

/// One Java biome of the pinned registry; its index is Java's network registry id (the
/// `minecraft:worldgen/biome` entry order of the generator's `networkCodec.nbt`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JavaBiomeEntry<'a> {
    /// The biome id, e.g. `minecraft:plains`.
    pub name: &'a str,
    /// The base temperature, before any temperature modifier.
    pub temperature: f32,
    pub downfall: f32,
    pub has_precipitation: bool,
    /// Whether the biome's temperature modifier is `frozen`.
    pub frozen: bool,
    /// The colours the biome sets over its dimension type's.
    pub colours: JavaSkyColours,
}

impl<'a> From<&'a JavaBiomeRecord> for JavaBiomeEntry<'a> {
    fn from(record: &'a JavaBiomeRecord) -> Self {
        Self {
            name: &record.name,
            temperature: record.temperature,
            downfall: record.downfall,
            has_precipitation: record.has_precipitation,
            frozen: record.frozen,
            colours: record.colours,
        }
    }
}

/// One Java property list as `(key, value)` pairs sorted by key.
type PropertyList = Box<[(Box<str>, Box<str>)]>;

/// Decoded Java identities with per-block tag lookup.
#[derive(Clone, Debug)]
pub struct JavaBlockMapping {
    payload: JavaMappingPayload,
    properties: Box<[PropertyList]>,
    block_tags: Box<[Box<[u32]>]>,
}

/// One Java block state: a block id and its complete property list.
#[derive(Clone, Copy)]
pub struct JavaBlockState<'a> {
    mapping: &'a JavaBlockMapping,
    block: u16,
    properties: u32,
}

impl<'a> JavaBlockState<'a> {
    /// The Java block id, e.g. `minecraft:oak_leaves`.
    pub fn name(self) -> &'a str {
        &self.mapping.payload.blocks[usize::from(self.block)]
    }

    /// Every Java property as `(key, value)`, sorted by key.
    pub fn properties(self) -> impl ExactSizeIterator<Item = (&'a str, &'a str)> + 'a {
        self.mapping.properties[self.properties as usize]
            .iter()
            .map(|(key, value)| (key.as_ref(), value.as_ref()))
    }

    /// The value of one Java property, if the block has it.
    pub fn property(self, key: &str) -> Option<&'a str> {
        self.properties()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value)
    }

    /// The Java block tags (`namespace:path`) containing this block.
    pub fn tags(self) -> impl ExactSizeIterator<Item = &'a str> + 'a {
        let tags = &self.mapping.payload.tags;
        self.mapping.block_tags[usize::from(self.block)]
            .iter()
            .map(move |&tag| tags[tag as usize].0.as_ref())
    }

    /// Whether the block belongs to the Java block tag `tag` (`namespace:path`).
    pub fn in_tag(self, tag: &str) -> bool {
        self.tags().any(|name| name == tag)
    }
}

impl fmt::Display for JavaBlockState<'_> {
    /// Java block-state syntax: `minecraft:oak_log[axis=y]`, or the bare id without properties.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let properties = &self.mapping.payload.property_sets[self.properties as usize];
        if properties.is_empty() {
            f.write_str(self.name())
        } else {
            write!(f, "{}[{properties}]", self.name())
        }
    }
}

impl fmt::Debug for JavaBlockState<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JavaBlockState({self})")
    }
}

impl JavaBlockMapping {
    /// The checked-in carrier, validated once; an error means "nothing mapped".
    pub fn pinned() -> Result<&'static Self, &'static JavaMappingError> {
        static MAPPING: LazyLock<Result<JavaBlockMapping, JavaMappingError>> =
            LazyLock::new(|| {
                let mapping = JavaBlockMapping::validate_pinned(SOURCE_BYTES).and_then(|()| {
                    JavaBlockMapping::decode(
                        SOURCE_BYTES,
                        crate::active_content_registry_protocol(),
                    )
                });
                if let Err(error) = &mapping {
                    tracing::warn!(%error, "Java block and biome mapping unavailable");
                }
                mapping
            });
        MAPPING.as_ref()
    }

    /// Checks bytes against the reviewed carrier's external fingerprint.
    pub fn validate_pinned(bytes: &[u8]) -> Result<(), JavaMappingError> {
        ensure(bytes.len() <= MAX_BYTES, "carrier size")?;
        ensure(
            format!("{:x}", Sha256::digest(bytes)) == SOURCE_SHA256.trim(),
            "pinned carrier hash",
        )
    }

    /// Decodes a hash-sealed carrier bound to `protocol` and this checkout's registries.
    pub fn decode(bytes: &[u8], protocol: u32) -> Result<Self, JavaMappingError> {
        ensure(
            bytes.len() >= MAGIC.len() + 32 && bytes.len() <= MAX_BYTES,
            "carrier size",
        )?;
        let (content, checksum) = bytes.split_at(bytes.len() - 32);
        ensure(content.starts_with(MAGIC), "carrier format")?;
        ensure(
            Sha256::digest(content).as_slice() == checksum,
            "carrier checksum",
        )?;
        let payload: JavaMappingPayload = serde_json::from_slice(&content[MAGIC.len()..])
            .map_err(|error| JavaMappingError(error.to_string()))?;
        ensure(payload.protocol == protocol, "registry protocol")?;
        ensure(
            payload.block_registry_sha256
                == <[u8; 32]>::from(Sha256::digest(
                    crate::pinned_content::pinned_block_registry_bytes(),
                )),
            "block registry binding",
        )?;
        ensure(
            payload.biome_registry_sha256 == <[u8; 32]>::from(Sha256::digest(BIOME_REGISTRY_BYTES)),
            "biome registry binding",
        )?;
        ensure(
            payload.source_manifest_sha256
                == <[u8; 32]>::from(Sha256::digest(SOURCE_MANIFEST_BYTES)),
            "source manifest binding",
        )?;
        Self::from_payload(payload)
    }

    /// Encodes a payload exactly as [`Self::decode`] accepts it, with a trailing SHA-256 seal.
    pub fn encode(payload: &JavaMappingPayload) -> Result<Vec<u8>, JavaMappingError> {
        Self::from_payload(payload.clone())?;
        let mut bytes = MAGIC.to_vec();
        bytes.extend(
            serde_json::to_vec(payload).map_err(|error| JavaMappingError(error.to_string()))?,
        );
        bytes.extend_from_slice(&Sha256::digest(&bytes));
        ensure(bytes.len() <= MAX_BYTES, "carrier size")?;
        Ok(bytes)
    }

    /// Validates table ordering and every index, then builds the lookups.
    pub fn from_payload(payload: JavaMappingPayload) -> Result<Self, JavaMappingError> {
        ensure(
            !payload.blocks.is_empty() && payload.blocks.len() <= usize::from(u16::MAX),
            "block count",
        )?;
        ensure(strictly_sorted(&payload.blocks), "block order")?;
        ensure(
            payload.blocks.iter().all(|name| namespaced(name)),
            "block id syntax",
        )?;
        ensure(strictly_sorted(&payload.property_sets), "property order")?;
        let properties = payload
            .property_sets
            .iter()
            .map(|set| parse_properties(set))
            .collect::<Option<Box<[_]>>>()
            .ok_or_else(|| JavaMappingError("property syntax".into()))?;
        ensure(
            payload.states.iter().flatten().all(|&(block, set)| {
                usize::from(block) < payload.blocks.len() && (set as usize) < properties.len()
            }),
            "state index",
        )?;
        ensure(
            strictly_sorted(&payload.approximate)
                && payload
                    .approximate
                    .iter()
                    .all(|&id| payload.states.get(id as usize).is_some_and(Option::is_some)),
            "approximate states",
        )?;
        ensure(
            payload.tags.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "tag order",
        )?;
        let mut block_tags = vec![Vec::new(); payload.blocks.len()];
        for (index, (name, members)) in payload.tags.iter().enumerate() {
            ensure(namespaced(name), "tag id syntax")?;
            ensure(strictly_sorted(members), "tag member order")?;
            for &member in members.iter() {
                let tags = block_tags
                    .get_mut(usize::from(member))
                    .ok_or_else(|| JavaMappingError("tag member index".into()))?;
                tags.push(index as u32);
            }
        }
        let java_biomes = &payload.java_biomes;
        ensure(
            java_biomes.len() <= usize::from(u16::MAX),
            "Java biome count",
        )?;
        ensure(
            java_biomes.iter().all(|biome| {
                namespaced(&biome.name)
                    && biome.temperature.is_finite()
                    && biome.downfall.is_finite()
                    && biome.colours.valid()
            }) && payload
                .dimension_colours
                .iter()
                .all(|colours| colours.valid()),
            "Java biome entry",
        )?;
        let mut names: Vec<&str> = java_biomes
            .iter()
            .map(|biome| biome.name.as_ref())
            .collect();
        names.sort_unstable();
        ensure(strictly_sorted(&names), "Java biome names unique")?;
        ensure(
            payload
                .biome_tags
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
                && payload.biome_tags.iter().all(|(name, members)| {
                    namespaced(name)
                        && strictly_sorted(members)
                        && members
                            .last()
                            .is_none_or(|&last| usize::from(last) < java_biomes.len())
                }),
            "biome tags",
        )?;
        ensure(
            payload.biomes.windows(2).all(|pair| pair[0].0 < pair[1].0)
                && payload
                    .biomes
                    .iter()
                    .all(|&(_, index)| usize::from(index) < java_biomes.len()),
            "biome table",
        )?;
        validate_identities(
            &payload.entities,
            &payload.bedrock_entities,
            "entity identities",
        )?;
        validate_identities(&payload.items, &payload.bedrock_items, "item identities")?;
        Ok(Self {
            payload,
            properties,
            block_tags: block_tags.into_iter().map(Vec::into_boxed_slice).collect(),
        })
    }

    /// The Java block state of a block-registry sequential id.
    pub fn java_block_state(&self, bedrock_state: u32) -> Option<JavaBlockState<'_>> {
        let (block, properties) = (*self.payload.states.get(bedrock_state as usize)?)?;
        Some(JavaBlockState {
            mapping: self,
            block,
            properties,
        })
    }

    /// Every block-registry state in sequential-id order, `None` where unmapped.
    pub fn block_states(&self) -> impl ExactSizeIterator<Item = Option<JavaBlockState<'_>>> {
        self.payload.states.iter().map(move |state| {
            state.map(|(block, properties)| JavaBlockState {
                mapping: self,
                block,
                properties,
            })
        })
    }

    /// Whether a mapped state took the Java state of a similar state of its Bedrock block
    /// because Geyser maps no Java state to it exactly.
    pub fn is_approximate(&self, bedrock_state: u32) -> bool {
        self.payload
            .approximate
            .binary_search(&bedrock_state)
            .is_ok()
    }

    /// The Java biome id of a Bedrock biome id.
    pub fn java_biome(&self, bedrock_biome: u32) -> Option<&str> {
        self.java_biome_index(bedrock_biome)
            .and_then(|index| self.java_biome_entry(index))
            .map(|entry| entry.name)
    }

    /// The Java registry index of the Java biome a Bedrock biome id maps to.
    pub fn java_biome_index(&self, bedrock_biome: u32) -> Option<u16> {
        let biomes = &self.payload.biomes;
        biomes
            .binary_search_by_key(&bedrock_biome, |&(id, _)| id)
            .ok()
            .map(|index| biomes[index].1)
    }

    /// Every Java biome in network registry order, so the position is the registry index.
    pub fn java_biomes(&self) -> impl ExactSizeIterator<Item = JavaBiomeEntry<'_>> + '_ {
        self.payload.java_biomes.iter().map(JavaBiomeEntry::from)
    }

    /// The Java biome at registry index `index`.
    pub fn java_biome_entry(&self, index: u16) -> Option<JavaBiomeEntry<'_>> {
        self.payload
            .java_biomes
            .get(usize::from(index))
            .map(JavaBiomeEntry::from)
    }

    /// The colours the dimension type `dimension` sets.
    pub fn java_dimension_colours(&self, dimension: JavaDimension) -> JavaSkyColours {
        self.payload.dimension_colours[dimension as usize]
    }

    /// Whether the Java biome at registry index `index` belongs to the Java biome tag `tag`
    /// (`namespace:path`, e.g. `minecraft:is_ocean`); false for unknown tags and indices.
    pub fn java_biome_has_tag(&self, index: u16, tag: &str) -> bool {
        let tags = &self.payload.biome_tags;
        tags.binary_search_by(|(name, _)| name.as_ref().cmp(tag))
            .is_ok_and(|found| tags[found].1.binary_search(&index).is_ok())
    }

    /// The Java entity type (`namespace:path`) a Bedrock entity identifier stands for, e.g.
    /// `minecraft:end_crystal` for `minecraft:ender_crystal`; `None` when Geyser spawns no
    /// Bedrock entity of that identifier.
    pub fn java_entity(&self, bedrock_identifier: &str) -> Option<&str> {
        let payload = &self.payload;
        preferred(
            &payload.entities,
            &payload.bedrock_entities,
            bedrock_identifier,
        )
    }

    /// Every Java entity type Geyser spawns as `bedrock_identifier`, by Java id.
    pub fn java_entities<'a>(
        &'a self,
        bedrock_identifier: &'a str,
    ) -> impl Iterator<Item = &'a str> {
        java_ids(&self.payload.entities, bedrock_identifier)
    }

    /// The Java item (`namespace:path`) a Bedrock item identifier stands for, e.g.
    /// `minecraft:cobweb` for `minecraft:web`; `None` when no Java item maps to it.
    pub fn java_item(&self, bedrock_identifier: &str) -> Option<&str> {
        let payload = &self.payload;
        preferred(&payload.items, &payload.bedrock_items, bedrock_identifier)
    }

    /// Every Java item Geyser maps to `bedrock_identifier`, by Java id.
    pub fn java_items<'a>(&'a self, bedrock_identifier: &'a str) -> impl Iterator<Item = &'a str> {
        java_ids(&self.payload.items, bedrock_identifier)
    }

    /// The decoded payload.
    pub fn payload(&self) -> &JavaMappingPayload {
        &self.payload
    }
}

/// The Java block state of a block-registry sequential id, from the pinned carrier.
pub fn java_block_state(bedrock_state: u32) -> Option<JavaBlockState<'static>> {
    JavaBlockMapping::pinned()
        .ok()?
        .java_block_state(bedrock_state)
}

/// The Java biome id of a Bedrock biome id, from the pinned carrier.
pub fn java_biome(bedrock_biome: u32) -> Option<&'static str> {
    JavaBlockMapping::pinned().ok()?.java_biome(bedrock_biome)
}

/// Splits `key=value,...` into pairs; rejects empty parts and unsorted or repeated keys.
fn parse_properties(set: &str) -> Option<PropertyList> {
    if set.is_empty() {
        return Some(Box::default());
    }
    let pairs = set
        .split(',')
        .map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (!key.is_empty() && !value.is_empty() && !value.contains('='))
                .then(|| (Box::from(key), Box::from(value)))
        })
        .collect::<Option<Box<[(Box<str>, Box<str>)]>>>()?;
    pairs
        .windows(2)
        .all(|pair| pair[0].0 < pair[1].0)
        .then_some(pairs)
}

/// Checks an identity table: Java ids namespaced, sorted and unique, Bedrock ids namespaced,
/// and `inverse` naming one pair per distinct Bedrock id in ascending Bedrock order.
fn validate_identities(
    pairs: &[JavaIdentity],
    inverse: &[u16],
    what: &str,
) -> Result<(), JavaMappingError> {
    ensure(
        pairs.len() <= usize::from(u16::MAX)
            && pairs.windows(2).all(|pair| pair[0].0 < pair[1].0)
            && pairs
                .iter()
                .all(|(java, bedrock)| namespaced(java) && namespaced(bedrock)),
        what,
    )?;
    let mut bedrock: Vec<&str> = pairs.iter().map(|(_, bedrock)| bedrock.as_ref()).collect();
    bedrock.sort_unstable();
    bedrock.dedup();
    let named = inverse
        .iter()
        .map(|&index| {
            pairs
                .get(usize::from(index))
                .map(|(_, bedrock)| bedrock.as_ref())
        })
        .collect::<Option<Vec<&str>>>();
    ensure(named.is_some_and(|named| named == bedrock), what)
}

/// The Java id `inverse` chooses for `bedrock`.
fn preferred<'a>(pairs: &'a [JavaIdentity], inverse: &[u16], bedrock: &str) -> Option<&'a str> {
    let found = inverse
        .binary_search_by(|&index| pairs[usize::from(index)].1.as_ref().cmp(bedrock))
        .ok()?;
    Some(&pairs[usize::from(inverse[found])].0)
}

/// Every Java id of `pairs` mapped to `bedrock`, in table order.
fn java_ids<'a>(pairs: &'a [JavaIdentity], bedrock: &'a str) -> impl Iterator<Item = &'a str> {
    pairs
        .iter()
        .filter(move |(_, mapped)| mapped.as_ref() == bedrock)
        .map(|(java, _)| java.as_ref())
}

fn namespaced(name: &str) -> bool {
    name.split_once(':')
        .is_some_and(|(namespace, path)| !namespace.is_empty() && !path.is_empty())
}

fn strictly_sorted<T: Ord>(items: &[T]) -> bool {
    items.windows(2).all(|pair| pair[0] < pair[1])
}

fn ensure(condition: bool, what: &str) -> Result<(), JavaMappingError> {
    if condition {
        Ok(())
    } else {
        Err(JavaMappingError(what.to_owned()))
    }
}

#[cfg(test)]
mod tests;
