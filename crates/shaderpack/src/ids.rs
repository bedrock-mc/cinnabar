//! `block.properties`, `item.properties` and `entity.properties`: the OptiFine/Iris ID maps
//! that give block states, items and entities the numbers packs read (`mc_Entity`,
//! `currentRenderedItemId`, `entityId`).
//!
//! Each file goes through the same conditional preprocessor as `shaders.properties` and is
//! then read as java.util.Properties: a repeated key takes the later value but keeps the
//! position of its first occurrence, as Iris's order-backed properties do. Order matters
//! because the first matching block entry wins. Malformed entries never fail a file; they
//! become [`PropertiesDiagnostic`]s whose message starts with the file name.

mod bedrock;
mod block;
mod legacy;
#[cfg(test)]
mod tests;

pub use bedrock::UNMAPPED_BLOCK_ID;
pub use block::{BlockIdMap, RenderLayer};

use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;

use crate::properties::reader::{RawEntry, read_entries};
use crate::properties::{PropertiesDiagnostic, last_by_key};

/// `shaders/`-relative name of the block id map.
pub(crate) const BLOCK_FILE: &str = "block.properties";
/// `shaders/`-relative name of the item id map.
pub(crate) const ITEM_FILE: &str = "item.properties";
/// `shaders/`-relative name of the entity id map.
pub(crate) const ENTITY_FILE: &str = "entity.properties";

/// Parsed block.properties / item.properties / entity.properties of a pack.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq)]
pub struct IdMaps {
    /// Block-state ids and render-layer overrides from block.properties; without the file,
    /// Iris's legacy numeric ids (`LegacyIdMap`).
    pub blocks: BlockIdMap,
    /// Item ids from item.properties (empty without the file).
    pub items: NamedIdMap,
    /// Entity ids from entity.properties (empty without the file).
    pub entities: NamedIdMap,
    /// Problems found in any of the three files, each message prefixed with its file name.
    pub diagnostics: Vec<PropertiesDiagnostic>,
}

impl IdMaps {
    /// Parses the raw texts of the three files (`None` = the pack lacks that file) with
    /// `defines` visible to `#if`/`#ifdef` (see [`crate::ShaderProperties::parse`]).
    pub fn parse(
        block: Option<&str>,
        item: Option<&str>,
        entity: Option<&str>,
        defines: &BTreeMap<String, String>,
    ) -> IdMaps {
        let mut diagnostics = Vec::new();
        let blocks = {
            let text = block.unwrap_or(legacy::BLOCK_PROPERTIES);
            let entries = load(BLOCK_FILE, text, defines, &mut diagnostics);
            BlockIdMap::parse(&entries, &mut diagnostics)
        };
        let mut named = |file: &str, prefix: &str, text: Option<&str>| {
            text.map_or_else(NamedIdMap::default, |text| {
                let entries = load(file, text, defines, &mut diagnostics);
                NamedIdMap::parse(file, prefix, &entries, &mut diagnostics)
            })
        };
        let items = named(ITEM_FILE, "item.", item);
        let entities = named(ENTITY_FILE, "entity.", entity);
        IdMaps {
            blocks,
            items,
            entities,
            diagnostics,
        }
    }
}

/// Item or entity ids: `<prefix><int>=<name> <name> ...` with names `[namespace:]path`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct NamedIdMap {
    /// Full `namespace:path` name → id.
    ids: HashMap<String, i32>,
}

impl NamedIdMap {
    /// Reads the `prefix`ed keys of `entries`; a name assigned again later takes the later id
    /// (Iris uses a plain map put). Entries with state filters (`=`) are not supported for
    /// items and entities and are skipped with a diagnostic.
    fn parse(
        file: &str,
        prefix: &str,
        entries: &[RawEntry],
        diagnostics: &mut Vec<PropertiesDiagnostic>,
    ) -> NamedIdMap {
        let mut ids = HashMap::new();
        for entry in entries {
            let Some(id) = parse_id(file, prefix, entry, diagnostics) else {
                continue;
            };
            for part in entry.value.split_whitespace() {
                if part.contains('=') {
                    diagnostics.push(diagnostic(
                        file,
                        entry.line,
                        format_args!(
                            "`{part}` in `{}`: state properties are not supported here",
                            entry.key
                        ),
                    ));
                    continue;
                }
                ids.insert(namespaced(part), id);
            }
        }
        NamedIdMap { ids }
    }

    /// The pack id of `name`, a full `namespace:path` id such as "minecraft:diamond_sword".
    pub fn id(&self, name: &str) -> Option<i32> {
        self.ids.get(name).copied()
    }

    /// Whether no name has an id.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Preprocesses `text` and reads it as java.util.Properties: a repeated key replaces the
/// value (and line) of its first occurrence, which keeps its position.
fn load(
    file: &str,
    text: &str,
    defines: &BTreeMap<String, String>,
    diagnostics: &mut Vec<PropertiesDiagnostic>,
) -> Vec<RawEntry> {
    let mut own = Vec::new();
    let entries = read_entries(text, defines, &mut own);
    diagnostics.extend(own.into_iter().map(|d| diagnostic(file, d.line, d.message)));
    last_by_key(&entries, |entry| &entry.key)
        .into_iter()
        .cloned()
        .collect()
}

/// The integer id of a `prefix<int>` key; `None` for keys without `prefix` (ignored, as in
/// Iris) and, with a diagnostic, for a suffix that is not an `i32`. Ids outside the 16-bit
/// range Iris stores are kept but reported.
fn parse_id(
    file: &str,
    prefix: &str,
    entry: &RawEntry,
    diagnostics: &mut Vec<PropertiesDiagnostic>,
) -> Option<i32> {
    let suffix = entry.key.strip_prefix(prefix)?;
    let Ok(id) = suffix.parse::<i32>() else {
        diagnostics.push(diagnostic(
            file,
            entry.line,
            format_args!(
                "invalid key `{}`: `{suffix}` is not an integer id",
                entry.key
            ),
        ));
        return None;
    };
    if i16::try_from(id).is_err() {
        diagnostics.push(diagnostic(
            file,
            entry.line,
            format_args!(
                "id {id} of `{}` is outside the 16-bit range Iris stores; kept as is",
                entry.key
            ),
        ));
    }
    Some(id)
}

/// `name` with the `minecraft` namespace when it has none; anything before the first ':'
/// is the namespace.
fn namespaced(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

/// A diagnostic at `line` of `file`, the file name leading the message.
fn diagnostic(file: &str, line: u32, message: impl Display) -> PropertiesDiagnostic {
    PropertiesDiagnostic {
        line,
        message: format!("{file}: {message}"),
    }
}
