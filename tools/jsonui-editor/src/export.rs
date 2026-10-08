//! Exporting edits as a Bedrock resource pack: an overlay of only what
//! changed (partial redefinitions, `a/b` nested overrides and `modifications`
//! that layer over the unedited stack), the full edited files, or a full copy
//! of one layer; packaged with a generated `manifest.json` as `.mcpack`,
//! `.zip` or `.mcaddon`. Only edited or user-authored files ever leave.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::outline::{self, Node, Spanned};
use crate::workspace::Workspace;

const UI_DEFS: &str = "ui/_ui_defs.json";
const GLOBALS: &str = "ui/_global_variables.json";
/// Files the export writes itself.
const GENERATED: &[&str] = &["manifest.json", "pack_icon.png"];
const TARGET: &str = include_str!("../../../assets/bedrock-target.json");

/// The manifest fields an export writes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PackInfo {
    pub name: String,
    pub description: String,
    pub version: [u32; 3],
    pub min_engine_version: [u32; 3],
    pub header_uuid: String,
    pub module_uuid: String,
}

impl PackInfo {
    /// A new pack with fresh `uuids`, version 1.0.0 and the repository's game version.
    pub fn new(name: &str, uuids: [String; 2]) -> Self {
        let [header_uuid, module_uuid] = uuids;
        Self {
            name: name.to_owned(),
            description: String::new(),
            version: [1, 0, 0],
            min_engine_version: default_min_engine_version(),
            header_uuid,
            module_uuid,
        }
    }

    /// The same pack one patch version on, for the next re-export.
    pub fn bumped(&self) -> Self {
        let mut next = self.clone();
        next.version[2] = next.version[2].saturating_add(1);
        next
    }

    /// Resource-pack `manifest.json` (format 2).
    pub fn manifest(&self) -> Result<Value, String> {
        if self.name.trim().is_empty() {
            return Err("the pack needs a name".into());
        }
        for uuid in [&self.header_uuid, &self.module_uuid] {
            if !is_uuid(uuid) {
                return Err(format!("`{uuid}` is not a UUID"));
            }
        }
        if self.header_uuid.eq_ignore_ascii_case(&self.module_uuid) {
            return Err("the header and module UUIDs must differ".into());
        }
        Ok(json!({
            "format_version": 2,
            "header": {
                "name": self.name,
                "description": self.description,
                "uuid": self.header_uuid.to_ascii_lowercase(),
                "version": self.version,
                "min_engine_version": self.min_engine_version,
            },
            "modules": [{
                "type": "resources",
                "description": self.description,
                "uuid": self.module_uuid.to_ascii_lowercase(),
                "version": self.version,
            }],
        }))
    }
}

/// `min_engine_version` from the game version `assets/bedrock-target.json` pins.
pub fn default_min_engine_version() -> [u32; 3] {
    let version = serde_json::from_str::<Value>(TARGET)
        .ok()
        .and_then(|target| target.get("game_version")?.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut parts = version.split('.').map(|part| part.parse().unwrap_or(0));
    [(); 3].map(|_| parts.next().unwrap_or(0))
}

fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.chars().all(|c| c.is_ascii_hexdigit()))
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Only the changed controls, layered over the unedited stack.
    Overlay,
    /// Every edited file in full.
    Changed,
    /// One layer with its edits applied.
    Full,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Mcpack,
    Zip,
    Mcaddon,
}

/// One export as the front ends request it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub mode: Mode,
    /// The layer [`Mode::Full`] copies.
    #[serde(default)]
    pub layer: Option<usize>,
    /// The user confirmed that layer is their own pack.
    #[serde(default)]
    pub own_layer: bool,
    pub format: Format,
    pub pack: PackInfo,
}

/// What an export will contain, before packaging.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Plan {
    /// Pack-relative path -> contents (text for ui json).
    #[serde(serialize_with = "texts")]
    pub files: BTreeMap<String, Vec<u8>>,
    /// Changes an overlay cannot express, and other caveats.
    pub notes: Vec<String>,
    /// Files left out because they are not the user's.
    pub skipped: Vec<String>,
}

fn texts<S: serde::Serializer>(files: &BTreeMap<String, Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = s.serialize_map(Some(files.len()))?;
    for (path, bytes) in files {
        match std::str::from_utf8(bytes) {
            Ok(text) => map.serialize_entry(path, text)?,
            Err(_) => map.serialize_entry(path, &format!("({} bytes)", bytes.len()))?,
        }
    }
    map.end()
}

/// Plan an export. `layer` picks the layer for [`Mode::Full`]; `own_layer`
/// confirms a non-scratch layer's contents are the user's to redistribute.
pub fn plan(workspace: &mut Workspace, mode: Mode, layer: Option<usize>, own_layer: bool) -> Plan {
    let mut plan = Plan::default();
    match mode {
        Mode::Overlay => overlay(workspace, &mut plan),
        Mode::Changed => changed(workspace, &mut plan),
        Mode::Full => full(workspace, layer, own_layer, &mut plan),
    }
    let new_files = new_ui_files(workspace, &plan);
    if !new_files.is_empty() || plan.files.contains_key(UI_DEFS) {
        let mut listed: Vec<String> = plan
            .files
            .get(UI_DEFS)
            .and_then(|bytes| parse(&String::from_utf8_lossy(bytes)))
            .and_then(|defs| defs.get("ui_defs")?.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        for path in new_files {
            if !listed.contains(&path) {
                listed.push(path);
            }
        }
        plan.files
            .insert(UI_DEFS.to_owned(), pretty(&json!({ "ui_defs": listed })));
    }
    plan
}

/// Every edited `(layer, path)`, bottom layer first.
fn edits(workspace: &Workspace) -> Vec<(usize, String)> {
    workspace
        .layers()
        .iter()
        .enumerate()
        .flat_map(|(index, layer)| layer.edited_paths().map(move |p| (index, p.to_owned())))
        .collect()
}

/// Edited ui files no unedited layer has, which a pack must list in `_ui_defs.json`.
fn new_ui_files(workspace: &Workspace, plan: &Plan) -> Vec<String> {
    plan.files
        .keys()
        .filter(|path| {
            path.starts_with("ui/") && path.ends_with(".json") && !path.starts_with("ui/_")
        })
        .filter(|path| !workspace.layers().iter().any(|layer| layer.had(path)))
        .cloned()
        .collect()
}

fn changed(workspace: &Workspace, plan: &mut Plan) {
    for (layer, path) in edits(workspace) {
        if let Some(bytes) = workspace.layers()[layer].file(&path) {
            plan.files.insert(path, bytes.to_vec());
        }
    }
}

fn full(workspace: &mut Workspace, layer: Option<usize>, own: bool, plan: &mut Plan) {
    let Some(index) = layer.filter(|index| *index < workspace.layers().len()) else {
        plan.notes.push("full copy needs a layer".into());
        return;
    };
    let owned = own || workspace.layers()[index].is_scratch();
    let paths = workspace.layers()[index].all_paths();
    for path in paths {
        if GENERATED.contains(&path.as_str()) {
            continue;
        }
        if !owned && !workspace.layers()[index].is_edited(&path) {
            plan.skipped.push(path);
            continue;
        }
        match workspace.read_file(index, &path) {
            Some(bytes) => {
                plan.files.insert(path, bytes.to_vec());
            }
            None => plan.skipped.push(path),
        }
    }
    let name = &workspace.layers()[index].name;
    match (owned, plan.skipped.len()) {
        (_, 0) => {}
        (true, count) => plan.notes.push(format!("{count} files of `{name}` were not loaded and are left out")),
        (false, count) => plan.notes.push(format!(
            "{count} unedited files of `{name}` left out: confirm the layer is your own pack to copy it whole"
        )),
    }
}

fn overlay(workspace: &Workspace, plan: &mut Plan) {
    let mut merged: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for (layer, path) in edits(workspace) {
        if path == UI_DEFS || !path.starts_with("ui/") || !path.ends_with(".json") {
            continue;
        }
        let target = &workspace.layers()[layer];
        let Some(edited) = target.file(&path) else {
            continue;
        };
        let edited_text = String::from_utf8_lossy(edited).into_owned();
        let original = match target.original(&path) {
            Some(Some(bytes)) => bytes.clone(),
            // A file the user made: it ships as written.
            _ => {
                if path == GLOBALS && parse(&edited_text).is_some_and(|v| v == json!({})) {
                    continue;
                }
                plan.files.insert(path, edited.to_vec());
                continue;
            }
        };
        let (Some(before), Some(after)) = (
            parse(&String::from_utf8_lossy(&original)),
            parse(&edited_text),
        ) else {
            plan.notes
                .push(format!("{path}: not valid JSON, shipped in full"));
            plan.files.insert(path, edited.to_vec());
            continue;
        };
        let diff = if path == GLOBALS {
            diff_globals(&before, &after, &path, &mut plan.notes)
        } else {
            diff_file(&before, &after, &path, &mut plan.notes)
        };
        if let Some(diff) = diff {
            merged.entry(path).or_default().extend(diff);
        }
    }
    for (path, mut object) in merged {
        // `namespace` leads, as authors write it.
        let text = match object.remove("namespace") {
            Some(namespace) if !object.is_empty() => {
                let rest = String::from_utf8(pretty(&Value::Object(object))).unwrap_or_default();
                format!("{{\n  \"namespace\": {namespace},{}", &rest[1..]).into_bytes()
            }
            Some(namespace) => pretty(&json!({ "namespace": namespace })),
            None => pretty(&Value::Object(object)),
        };
        plan.files.insert(path, text);
    }
}

fn diff_globals(
    before: &Value,
    after: &Value,
    path: &str,
    notes: &mut Vec<String>,
) -> Option<Map<String, Value>> {
    let (before, after) = (before.as_object()?, after.as_object()?);
    let changed: Map<String, Value> = after
        .iter()
        .filter(|(key, value)| before.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        notes.push(format!(
            "{path}: `{key}` was deleted; an overlay cannot remove a variable"
        ));
    }
    (!changed.is_empty()).then_some(changed)
}

/// The overlay entries that turn `before` into `after`; `None` when equal.
fn diff_file(
    before: &Value,
    after: &Value,
    path: &str,
    notes: &mut Vec<String>,
) -> Option<Map<String, Value>> {
    let (before, after) = (before.as_object()?, after.as_object()?);
    let mut out = Map::new();
    let old: BTreeMap<&str, (Option<&str>, &Value)> = before
        .iter()
        .filter(|(key, _)| *key != "namespace")
        .map(|(key, body)| {
            let (name, base) = split_key(key);
            (name, (base, body))
        })
        .collect();
    let mut seen = BTreeSet::new();
    for (key, body) in after.iter().filter(|(key, _)| *key != "namespace") {
        let (name, base) = split_key(key);
        seen.insert(name);
        let Some((old_base, old_body)) = old.get(name) else {
            out.insert(key.clone(), body.clone());
            continue;
        };
        let partial = diff_control(old_body, body, name, &mut out, path, notes);
        let rebased = base != *old_base;
        if rebased && base.is_none() {
            notes.push(format!(
                "{path}: `{name}` dropped its @base; an overlay cannot remove one"
            ));
        }
        if !partial.is_empty() || (rebased && base.is_some()) {
            let key = if rebased && base.is_some() {
                key.clone()
            } else {
                name.to_owned()
            };
            out.insert(key, Value::Object(partial));
        }
    }
    for name in old.keys().filter(|name| !seen.contains(*name)) {
        notes.push(format!(
            "{path}: `{name}` was deleted; an overlay cannot remove a definition"
        ));
    }
    if out.is_empty() {
        return None;
    }
    if let Some(namespace) = after.get("namespace").or_else(|| before.get("namespace")) {
        out.insert("namespace".into(), namespace.clone());
    }
    Some(out)
}

/// The changed properties of one control; child changes become `modifications`
/// here and `address/child` entries in `out`.
fn diff_control(
    before: &Value,
    after: &Value,
    address: &str,
    out: &mut Map<String, Value>,
    path: &str,
    notes: &mut Vec<String>,
) -> Map<String, Value> {
    let empty = Map::new();
    let before = before.as_object().unwrap_or(&empty);
    let after = after.as_object().unwrap_or(&empty);
    if before.contains_key("modifications")
        && (before.get("modifications") != after.get("modifications")
            || before.get("controls") != after.get("controls"))
    {
        return after.clone();
    }
    let mut partial = Map::new();
    let mut modifications = Vec::new();
    for (key, value) in after
        .iter()
        .filter(|(key, _)| *key != "controls" && *key != "modifications")
    {
        match (before.get(key), value) {
            (Some(old), _) if old == value => {}
            (Some(Value::Array(old)), Value::Array(new))
                if new.len() > old.len()
                    && new.starts_with(old)
                    && old.iter().all(Value::is_object) =>
            {
                modifications.push(json!({
                    "array_name": key, "operation": "insert_back", "value": new[old.len()..],
                }));
            }
            _ => {
                partial.insert(key.clone(), value.clone());
            }
        }
    }
    for key in before
        .keys()
        .filter(|key| *key != "controls" && !after.contains_key(*key))
    {
        notes.push(format!(
            "{path}: `{address}` lost `{key}`; an overlay cannot remove a property"
        ));
    }
    match (before.get("controls"), after.get("controls")) {
        (old, new) if old == new => {}
        (Some(Value::Array(old)), Some(Value::Array(new))) => {
            match child_changes(old, new, address, out, path, notes) {
                Some(ops) => modifications.extend(ops),
                None => {
                    partial.insert("controls".into(), Value::Array(new.clone()));
                }
            }
        }
        (_, Some(new)) => {
            partial.insert("controls".into(), new.clone());
        }
        (Some(_), None) => notes.push(format!(
            "{path}: `{address}` lost `controls`; an overlay cannot remove them"
        )),
        (None, None) => {}
    }
    if !modifications.is_empty() {
        if let Some(Value::Array(authored)) = after.get("modifications") {
            modifications.extend(authored.iter().cloned());
        }
        partial.insert("modifications".into(), Value::Array(modifications));
    } else if before.get("modifications") != after.get("modifications")
        && let Some(authored) = after.get("modifications")
    {
        partial.insert("modifications".into(), authored.clone());
    }
    partial
}

/// `modifications` turning child list `old` into `new`, with changed kept
/// children recursed into `out`; `None` when a full list states it more
/// safely (reordered or duplicate names).
fn child_changes(
    old: &[Value],
    new: &[Value],
    address: &str,
    out: &mut Map<String, Value>,
    path: &str,
    notes: &mut Vec<String>,
) -> Option<Vec<Value>> {
    let (old, new) = (Child::list(old)?, Child::list(new)?);
    let names = |list: &[Child]| -> Vec<String> { list.iter().map(|c| c.name.clone()).collect() };
    let (old_names, new_names) = (names(&old), names(&new));
    let unique = |names: &[String]| names.iter().collect::<BTreeSet<_>>().len() == names.len();
    if !unique(&old_names) || !unique(&new_names) {
        return None;
    }
    let kept_old: Vec<&String> = old_names.iter().filter(|n| new_names.contains(n)).collect();
    let kept_new: Vec<&String> = new_names.iter().filter(|n| old_names.contains(n)).collect();
    if kept_old != kept_new {
        return None;
    }
    let mut ops = Vec::new();
    for name in old_names.iter().filter(|n| !new_names.contains(n)) {
        ops.push(json!({ "array_name": "controls", "operation": "remove", "control_name": name }));
    }
    let mut anchor: Option<String> = None;
    let mut pending: Vec<Value> = Vec::new();
    let flush = |anchor: &Option<String>, pending: &mut Vec<Value>, ops: &mut Vec<Value>| {
        if pending.is_empty() {
            return;
        }
        let value = Value::Array(std::mem::take(pending));
        ops.push(match anchor {
            Some(name) => json!({ "array_name": "controls", "operation": "insert_after", "control_name": name, "value": value }),
            None => json!({ "array_name": "controls", "operation": "insert_front", "value": value }),
        });
    };
    for Child {
        name,
        base,
        body,
        entry,
    } in &new
    {
        let Some(Child {
            base: old_base,
            body: old_body,
            ..
        }) = old.iter().find(|o| &o.name == name)
        else {
            pending.push(entry.clone());
            continue;
        };
        flush(&anchor, &mut pending, &mut ops);
        if base != old_base {
            ops.push(json!({ "array_name": "controls", "operation": "replace", "control_name": name, "value": [entry] }));
        } else if body != old_body {
            let child = format!("{address}/{name}");
            let partial = diff_control(old_body, body, &child, out, path, notes);
            if !partial.is_empty() {
                out.insert(child, Value::Object(partial));
            }
        }
        anchor = Some(name.clone());
    }
    flush(&anchor, &mut pending, &mut ops);
    Some(ops)
}

/// One `controls` entry: its instance name, `@base`, body and the entry itself.
struct Child {
    name: String,
    base: Option<String>,
    body: Value,
    entry: Value,
}

impl Child {
    fn list(entries: &[Value]) -> Option<Vec<Child>> {
        entries
            .iter()
            .map(|entry| {
                let (key, body) = entry.as_object()?.iter().next()?;
                let (name, base) = split_key(key);
                Some(Child {
                    name: name.to_owned(),
                    base: base.map(str::to_owned),
                    body: body.clone(),
                    entry: entry.clone(),
                })
            })
            .collect()
    }
}

fn split_key(key: &str) -> (&str, Option<&str>) {
    match key.split_once('@') {
        Some((name, base)) => (name, Some(base)),
        None => (key, None),
    }
}

/// Tolerant JSON (comments, trailing commas) to a value.
pub fn parse(text: &str) -> Option<Value> {
    let root = outline::parse(text).ok()?;
    Some(to_value(&root, text))
}

fn to_value(node: &Spanned, text: &str) -> Value {
    match &node.node {
        Node::Object(members) => Value::Object(
            members
                .iter()
                .map(|member| (member.key.clone(), to_value(&member.value, text)))
                .collect(),
        ),
        Node::Array(items) => Value::Array(items.iter().map(|item| to_value(item, text)).collect()),
        Node::String(value) => Value::String(value.clone()),
        Node::Other => serde_json::from_str(&text[node.start..node.end]).unwrap_or(Value::Null),
    }
}

fn pretty(value: &Value) -> Vec<u8> {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text.into_bytes()
}

/// Checks a `pack_icon.png`: it must be a PNG; a non-square one only warns.
pub fn check_icon(bytes: &[u8]) -> Result<Vec<String>, String> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return Err("pack_icon.png must be a PNG image".into());
    }
    let dimension =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let (width, height) = (dimension(16), dimension(20));
    let mut warnings = Vec::new();
    if width != height {
        warnings.push(format!(
            "pack_icon.png is {width}x{height}; Bedrock expects a square icon"
        ));
    }
    Ok(warnings)
}

/// Package `plan` with a manifest (and icon) in `format`.
pub fn package(
    plan: &Plan,
    pack: &PackInfo,
    icon: Option<&[u8]>,
    format: Format,
) -> Result<Vec<u8>, String> {
    let manifest = pack.manifest()?;
    if let Some(icon) = icon {
        check_icon(icon)?;
    }
    let mut files: Vec<(String, Vec<u8>)> = vec![("manifest.json".into(), pretty(&manifest))];
    if let Some(icon) = icon {
        files.push(("pack_icon.png".into(), icon.to_vec()));
    }
    files.extend(
        plan.files
            .iter()
            .map(|(path, bytes)| (path.clone(), bytes.clone())),
    );
    let pack_zip = zip_files(&files)?;
    match format {
        Format::Mcpack | Format::Zip => Ok(pack_zip),
        Format::Mcaddon => zip_files(&[(format!("{}.mcpack", file_stem(&pack.name)), pack_zip)]),
    }
}

/// A filesystem-safe stem for `name`.
pub fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if stem.trim_matches('_').is_empty() {
        "pack".into()
    } else {
        stem
    }
}

fn zip_files(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, bytes) in files {
        writer
            .start_file(path.as_str(), options)
            .map_err(|e| e.to_string())?;
        writer.write_all(bytes).map_err(|e| e.to_string())?;
    }
    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|e| e.to_string())
}

/// The pack info of an exported `.mcpack`/`.zip`/`.mcaddon`, so a re-export
/// keeps its UUIDs.
pub fn read_pack_info(bytes: &[u8]) -> Option<PackInfo> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let read = |archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str| -> Option<Vec<u8>> {
        let mut out = Vec::new();
        archive
            .by_name(name)
            .ok()?
            .take(16 * 1024 * 1024)
            .read_to_end(&mut out)
            .ok()?;
        Some(out)
    };
    if let Some(manifest) = read(&mut archive, "manifest.json") {
        let manifest = parse(&String::from_utf8_lossy(&manifest))?;
        let header = manifest.get("header")?;
        let module = manifest.get("modules")?.get(0)?;
        let triple = |value: &Value| serde_json::from_value::<[u32; 3]>(value.clone()).ok();
        return Some(PackInfo {
            name: header.get("name")?.as_str()?.to_owned(),
            description: header
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            version: triple(header.get("version")?)?,
            min_engine_version: triple(header.get("min_engine_version")?)?,
            header_uuid: header.get("uuid")?.as_str()?.to_owned(),
            module_uuid: module.get("uuid")?.as_str()?.to_owned(),
        });
    }
    let inner = archive
        .file_names()
        .find(|name| name.ends_with(".mcpack"))?
        .to_owned();
    read_pack_info(&read(&mut archive, &inner)?)
}
