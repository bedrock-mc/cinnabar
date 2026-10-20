//! block.properties: block-state ids (`block.<int>`) and render-layer overrides (`layer.*`).

use std::borrow::Cow;
use std::collections::HashMap;

use super::{BLOCK_FILE, diagnostic, namespaced, parse_id};
use crate::properties::PropertiesDiagnostic;
use crate::properties::reader::RawEntry;

/// A render layer a block can be moved to with `layer.<layer>=<blocks>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderLayer {
    /// `layer.solid`: opaque geometry.
    Solid,
    /// `layer.cutout`: alpha-tested geometry without mipmaps.
    Cutout,
    /// `layer.cutout_mipped`: alpha-tested geometry with mipmaps.
    CutoutMipped,
    /// `layer.translucent`: blended geometry.
    Translucent,
}

impl RenderLayer {
    /// The layer named by the part of a `layer.` key after the prefix.
    fn from_key(name: &str) -> Option<RenderLayer> {
        match name {
            "solid" => Some(RenderLayer::Solid),
            "cutout" => Some(RenderLayer::Cutout),
            "cutout_mipped" => Some(RenderLayer::CutoutMipped),
            "translucent" => Some(RenderLayer::Translucent),
            _ => None,
        }
    }
}

/// Block-state ids of a pack.
///
/// Entries are `[namespace:]name[:key=v1[,v2...]]...` or the tag form `%[namespace:]tag[...]`;
/// the namespace defaults to `minecraft`. A state matches an entry when its block name is
/// the entry's and, for every filter whose key the state has, the state's value is one of
/// the listed alternatives; filters on properties the block lacks are ignored. The id of a
/// state is that of the first matching plain entry in file order; only when none matches,
/// that of the first matching tag entry in file order (Iris/OptiFine precedence).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockIdMap {
    /// Plain entries by full block name, in precedence order.
    by_name: HashMap<String, Vec<Entry>>,
    /// Tag entries in precedence order.
    tag_entries: Vec<TagEntry>,
    /// Distinct lowercase tag ids, indexed by [`TagEntry::tag`], in first-use order.
    tags: Vec<String>,
    /// `layer.*` overrides by full block name.
    layers: HashMap<String, RenderLayer>,
}

/// One entry's id and state filters.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: i32,
    filters: Vec<Filter>,
}

/// `key=v1,v2,...`: the state's `key` must have one of `values`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Filter {
    key: String,
    values: Vec<String>,
}

/// An entry applying to every block of a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TagEntry {
    /// Index into [`BlockIdMap::tags`].
    tag: usize,
    entry: Entry,
}

/// A parsed entry: plain or tag, its full (for tags lowercase) id and its filters.
struct Parsed {
    is_tag: bool,
    id: String,
    filters: Vec<Filter>,
}

impl BlockIdMap {
    /// Builds the map from deduplicated block.properties entries in file order.
    pub(super) fn parse(
        entries: &[RawEntry],
        diagnostics: &mut Vec<PropertiesDiagnostic>,
    ) -> BlockIdMap {
        let mut map = BlockIdMap::default();
        // Iris keeps one entry list per id: a later key naming the same id (`block.7` and
        // `block.07`) replaces the earlier list but keeps its position.
        let mut groups: Vec<(i32, Vec<Parsed>)> = Vec::new();
        let mut positions: HashMap<i32, usize> = HashMap::new();
        for entry in entries {
            if let Some(layer) = entry.key.strip_prefix("layer.") {
                map.add_layer(layer, entry, diagnostics);
                continue;
            }
            let Some(id) = parse_id(BLOCK_FILE, "block.", entry, diagnostics) else {
                continue;
            };
            let parsed: Vec<Parsed> = entry
                .value
                .split_whitespace()
                .map(|part| parse_entry(part, entry, diagnostics))
                .collect();
            match positions.get(&id) {
                Some(&index) => {
                    if let Some(group) = groups.get_mut(index) {
                        group.1 = parsed;
                    }
                }
                None => {
                    positions.insert(id, groups.len());
                    groups.push((id, parsed));
                }
            }
        }
        for (id, parsed) in groups {
            for Parsed {
                is_tag,
                id: name,
                filters,
            } in parsed
            {
                let entry = Entry { id, filters };
                if is_tag {
                    let tag = match map.tags.iter().position(|t| *t == name) {
                        Some(tag) => tag,
                        None => {
                            map.tags.push(name);
                            map.tags.len() - 1
                        }
                    };
                    map.tag_entries.push(TagEntry { tag, entry });
                } else {
                    map.by_name.entry(name).or_default().push(entry);
                }
            }
        }
        map
    }

    /// Pack id for one Java block state. `name` is the full `namespace:path` id (e.g.
    /// "minecraft:oak_leaves"), `properties` the state's complete Java property list as
    /// (key, value) strings, `in_tag(tag)` reports whether the block belongs to the Java
    /// block tag `tag` given as lowercase "namespace:path". `None` when no entry matches.
    pub fn block_id(
        &self,
        name: &str,
        properties: &[(&str, &str)],
        in_tag: &dyn Fn(&str) -> bool,
    ) -> Option<i32> {
        if let Some(entry) = self
            .by_name
            .get(name)
            .and_then(|entries| entries.iter().find(|e| e.matches(properties)))
        {
            return Some(entry.id);
        }
        self.tag_entries
            .iter()
            .find(|t| {
                t.entry.matches(properties)
                    && self.tags.get(t.tag).is_some_and(|tag| in_tag(tag.as_str()))
            })
            .map(|t| t.entry.id)
    }

    /// Every tag id ("namespace:path", lowercase) the map references, each once, so hosts
    /// can precompute membership.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(String::as_str)
    }

    /// Whether no block or tag entry assigns an id (render-layer overrides do not count).
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.tag_entries.is_empty()
    }

    /// `layer.solid|cutout|cutout_mipped|translucent` render-layer overrides by block id:
    /// the layer the full `namespace:path` block `name` is moved to, if any.
    pub fn render_layer(&self, name: &str) -> Option<RenderLayer> {
        self.layers.get(name).copied()
    }

    /// Applies `layer.<layer>=<blocks>`; later assignments of a block win. Tags are not
    /// allowed here (as in Iris).
    fn add_layer(
        &mut self,
        layer: &str,
        entry: &RawEntry,
        diagnostics: &mut Vec<PropertiesDiagnostic>,
    ) {
        let Some(layer) = RenderLayer::from_key(layer) else {
            diagnostics.push(diagnostic(
                BLOCK_FILE,
                entry.line,
                format_args!("invalid render layer in key `{}`", entry.key),
            ));
            return;
        };
        for part in entry.value.split_whitespace() {
            if part.starts_with('%') {
                diagnostics.push(diagnostic(
                    BLOCK_FILE,
                    entry.line,
                    format_args!("tag `{part}` cannot be used in `{}`", entry.key),
                ));
                continue;
            }
            self.layers.insert(namespaced(part), layer);
        }
    }
}

impl Entry {
    /// Whether a state with `properties` passes every filter whose key it has.
    fn matches(&self, properties: &[(&str, &str)]) -> bool {
        self.filters.iter().all(|filter| {
            properties
                .iter()
                .find(|&&(key, _)| key == filter.key)
                .is_none_or(|&(_, value)| filter.values.iter().any(|v| v.as_str() == value))
        })
    }
}

/// Parses one whitespace-separated entry of `raw` (Iris `BlockEntry.parse` grammar, with
/// OptiFine's comma-separated value alternatives). Malformed filters are reported and
/// dropped; the rest of the entry is kept.
fn parse_entry(part: &str, raw: &RawEntry, diagnostics: &mut Vec<PropertiesDiagnostic>) -> Parsed {
    let is_tag = part.starts_with('%');
    let body: Cow<'_, str> = if is_tag {
        Cow::Owned(part.replace('%', ""))
    } else {
        Cow::Borrowed(part)
    };
    let pieces: Vec<&str> = java_split(&body, ':');
    let (id, start) = match pieces.as_slice() {
        [_] => (format!("minecraft:{body}"), 1),
        [name, second, ..] if second.contains('=') => (format!("minecraft:{name}"), 1),
        [namespace, name, ..] => (format!("{namespace}:{name}"), 2),
        [] => (String::from("minecraft:"), 0),
    };
    let id = if is_tag { id.to_lowercase() } else { id };
    let mut filters: Vec<Filter> = Vec::new();
    for piece in pieces.into_iter().skip(start) {
        let (key, values) = match java_split(piece, '=').as_slice() {
            [key, values] => (*key, *values),
            _ => {
                diagnostics.push(diagnostic(
                    BLOCK_FILE,
                    raw.line,
                    format_args!(
                        "entry `{part}` of `{}`: state filter `{piece}` is not of the form key=value; ignored",
                        raw.key
                    ),
                ));
                continue;
            }
        };
        let values: Vec<String> = values
            .split(',')
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect();
        match filters.iter_mut().find(|f| f.key == key) {
            Some(existing) => existing.values = values,
            None => filters.push(Filter {
                key: key.to_owned(),
                values,
            }),
        }
    }
    Parsed {
        is_tag,
        id,
        filters,
    }
}

/// `text` split at `separator` without trailing empty pieces, like Java's `String.split`
/// (an empty `text` yields one empty piece).
fn java_split(text: &str, separator: char) -> Vec<&str> {
    let mut pieces: Vec<&str> = text.split(separator).collect();
    while pieces.len() > 1 && pieces.last().is_some_and(|p| p.is_empty()) {
        pieces.pop();
    }
    if pieces.len() == 1 && text.ends_with(separator) {
        // Only separators: Java yields an empty array.
        if pieces.first().is_some_and(|p| p.is_empty()) {
            pieces.clear();
        }
    }
    pieces
}
