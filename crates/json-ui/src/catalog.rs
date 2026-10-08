//! Raw, pre-resolution model: every `ui/*.json` control kept as authored, keyed
//! by `namespace` then local name, with `$global` variables loaded alongside.
//! Property maps are preserved verbatim (minus `controls`, which becomes the child
//! list) so later stages consume unknown keys unchanged.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::json5;

/// A control exactly as authored, with its `@base` recorded from the key.
#[derive(Clone, Debug)]
pub struct RawControl {
    /// Namespace of the file this control was authored in; qualifies bare refs.
    pub owner_ns: String,
    /// Local instance name (the part before `@`).
    pub name: String,
    /// The raw `@base` reference (after `@`), possibly a bare name or a `$var`.
    pub base: Option<String>,
    /// All properties except `controls`.
    pub props: Map<String, Value>,
    /// Nested controls, in document order.
    pub children: Vec<RawControl>,
    /// Whether the body names `controls` (an array, or a `$var` kept in `props`),
    /// which then replaces any inherited child list.
    pub has_controls: bool,
}

impl RawControl {
    pub(crate) fn from_entry(
        owner_ns: &str,
        key: &str,
        value: &Value,
        diagnostics: &mut Vec<String>,
    ) -> Self {
        let (name, base) = split_key(key);
        let mut props = Map::new();
        let mut children = Vec::new();
        let mut has_controls = false;
        match value {
            Value::Object(object) => {
                has_controls = object.contains_key("controls");
                for (property, item) in object {
                    match (property.as_str(), item) {
                        // A `$var` child list resolves against the scope later.
                        ("controls", Value::String(_)) => {
                            props.insert(property.clone(), item.clone());
                        }
                        ("controls", _) => children = child_controls(owner_ns, item, diagnostics),
                        _ => {
                            props.insert(property.clone(), item.clone());
                        }
                    }
                }
            }
            // Packs write an empty body as `[]`.
            Value::Array(items) if items.is_empty() => {}
            _ => diagnostics.push(format!("{owner_ns}.{name}: control body is not an object")),
        }
        Self {
            owner_ns: owner_ns.to_owned(),
            name,
            base,
            props,
            children,
            has_controls,
        }
    }
}

impl RawControl {
    /// The authored `{ "name@base": body }` entry this control was read from.
    pub(crate) fn to_entry(&self) -> Value {
        let key = match &self.base {
            Some(base) => format!("{}@{base}", self.name),
            None => self.name.clone(),
        };
        let mut body = self.props.clone();
        if self.has_controls && !body.contains_key("controls") {
            let children = self.children.iter().map(Self::to_entry).collect();
            body.insert("controls".to_owned(), Value::Array(children));
        }
        Value::Object(Map::from_iter([(key, Value::Object(body))]))
    }
}

pub(crate) fn child_controls(
    owner_ns: &str,
    value: &Value,
    diagnostics: &mut Vec<String>,
) -> Vec<RawControl> {
    let Value::Array(entries) = value else {
        diagnostics.push(format!("{owner_ns}: `controls` is not an array"));
        return Vec::new();
    };
    let mut children = Vec::new();
    for entry in entries {
        let Value::Object(object) = entry else {
            diagnostics.push(format!("{owner_ns}: `controls` entry is not an object"));
            continue;
        };
        // Each entry must be a single-member object `{ "name@base": { .. } }`.
        let mut members = object.iter();
        match (members.next(), members.next()) {
            (Some((key, body)), None) => {
                children.push(RawControl::from_entry(owner_ns, key, body, diagnostics));
            }
            _ => diagnostics.push(format!(
                "{owner_ns}: `controls` entry has {} members, not one",
                object.len()
            )),
        }
    }
    children
}

pub(crate) fn split_key(key: &str) -> (String, Option<String>) {
    match key.split_once('@') {
        Some((name, base)) => (name.to_owned(), Some(base.to_owned())),
        None => (key.to_owned(), None),
    }
}

/// The name after a `ns.` qualifier, or the whole name: without `@`, the vanilla
/// name parser reads `spacer_9.5` as namespace `spacer_9`, name `5`.
pub(crate) fn unqualified(name: &str) -> &str {
    name.split_once('.').map_or(name, |(_, local)| local)
}

/// A document without a string `namespace` registers under this one.
pub(crate) const ROOT_NAMESPACE: &str = "_root";

#[derive(Clone, Debug, Default)]
struct FileRead {
    namespace: Option<String>,
    failed: bool,
}

/// The whole pack: variable globals plus every control keyed by namespace/name.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    globals: BTreeMap<String, Value>,
    /// Every `_ui_defs` path loaded so far; a pack file loads only when listed.
    ui_defs: std::collections::BTreeSet<String>,
    defs: BTreeMap<String, BTreeMap<String, RawControl>>,
    /// First-file parse state and namespace inherited by later layers at that path.
    files: BTreeMap<String, FileRead>,
    diagnostics: Vec<String>,
}

impl Catalog {
    /// Load `_ui_defs.json`, `_global_variables.json`, and every listed `ui/*.json`
    /// in declared load order. `ui_dir` is the pack's `ui/` directory. Missing or
    /// malformed individual files are skipped and recorded; the two index files
    /// are required.
    pub fn load_dir(ui_dir: &Path) -> Result<Self, LoadError> {
        let pack_root = ui_dir.parent().unwrap_or(ui_dir);
        let mut catalog = Catalog::default();
        catalog.load_globals(&ui_dir.join("_global_variables.json"))?;
        let order = read_ui_defs(&ui_dir.join("_ui_defs.json"))?;
        catalog.ui_defs.extend(order.iter().cloned());
        for entry in order {
            let path = pack_root.join(&entry);
            catalog.load_file(&entry, &path);
        }
        Ok(catalog)
    }

    fn load_globals(&mut self, path: &Path) -> Result<(), LoadError> {
        let text = read(path)?;
        self.load_globals_text(path, &text)
    }

    pub(crate) fn load_globals_text(&mut self, path: &Path, text: &str) -> Result<(), LoadError> {
        let value = json5::parse(text).map_err(|source| LoadError::Parse {
            path: path.to_path_buf(),
            message: source.to_string(),
        })?;
        let Value::Object(object) = value else {
            return Err(LoadError::Shape {
                path: path.to_path_buf(),
            });
        };
        self.merge_globals(object);
        Ok(())
    }

    /// Layer global variables; object values merge member by member, as the
    /// vanilla client's JSON merge does, and anything else replaces.
    fn merge_globals(&mut self, object: Map<String, Value>) {
        for (key, item) in object {
            let Some(name) = key.strip_prefix('$') else {
                continue;
            };
            match (self.globals.get_mut(name), item) {
                (Some(Value::Object(old)), Value::Object(new)) => {
                    crate::pack::merge_objects(old, &new);
                }
                (_, item) => {
                    self.globals.insert(name.to_owned(), item);
                }
            }
        }
    }

    fn load_file(&mut self, entry: &str, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                self.diagnostics
                    .push(format!("{entry}: unreadable ({error})"));
                return;
            }
        };
        self.load_text(entry, &text);
    }

    /// Layers one pack document over the catalog with first-file retention and
    /// later-file merge semantics. Syntax errors remain in diagnostics.
    pub fn overlay_text(&mut self, entry: &str, text: &str) {
        self.merge_overlay_file(entry, text);
    }

    /// Layers a pack's `_global_variables.json` over the earlier ones.
    pub fn overlay_globals_text(&mut self, text: &str) {
        match json5::parse(text) {
            Ok(Value::Object(object)) => self.merge_globals(object),
            _ => self
                .diagnostics
                .push("_global_variables.json: overlay is not an object".to_owned()),
        }
    }

    /// Add every control of one `ui/*.json` document; a redefinition replaces.
    pub(crate) fn load_text(&mut self, entry: &str, text: &str) {
        let Some(value) = self.read_document(entry, text) else {
            return;
        };
        let Value::Object(object) = value else {
            self.diagnostics
                .push(format!("{entry}: top level is not an object"));
            return;
        };
        let namespace = match object.get("namespace") {
            Some(Value::String(namespace)) => namespace.clone(),
            _ => ROOT_NAMESPACE.to_owned(),
        };
        self.remember_file_namespace(entry, &namespace);
        for (key, body) in &object {
            if key == "namespace" {
                continue;
            }
            let control = RawControl::from_entry(&namespace, key, body, &mut self.diagnostics);
            let table = self.defs.entry(namespace.clone()).or_default();
            if let Some(existing) = table.insert(control.name.clone(), control) {
                self.diagnostics.push(format!(
                    "{namespace}.{}: redefined; last wins",
                    existing.name
                ));
            }
        }
    }

    pub fn lookup(&self, namespace: &str, name: &str) -> Option<&RawControl> {
        self.defs.get(namespace)?.get(name)
    }

    /// Every top-level definition; nested instances belong to its child list.
    pub fn controls(&self) -> impl Iterator<Item = &RawControl> {
        self.defs.values().flat_map(|table| table.values())
    }

    /// Mutable definition bodies for an application-owned presentation policy.
    pub fn controls_mut(&mut self) -> impl Iterator<Item = &mut RawControl> {
        self.defs.values_mut().flat_map(|table| table.values_mut())
    }

    pub(crate) fn lookup_mut(&mut self, namespace: &str, name: &str) -> Option<&mut RawControl> {
        self.defs.get_mut(namespace)?.get_mut(name)
    }

    /// Replace a complete definition, including its inherited template and children.
    pub fn insert(&mut self, control: RawControl) {
        self.defs
            .entry(control.owner_ns.clone())
            .or_default()
            .insert(control.name.clone(), control);
    }

    pub(crate) fn lists(&self, entry: &str) -> bool {
        self.ui_defs.contains(entry)
    }

    pub(crate) fn list(&mut self, entries: impl IntoIterator<Item = String>) {
        self.ui_defs.extend(entries);
    }

    pub(crate) fn file_namespace(&self, entry: &str) -> Option<&str> {
        self.files.get(entry)?.namespace.as_deref()
    }

    /// Records a document's namespace so later pack layers can inherit it.
    pub(crate) fn remember_file_namespace(&mut self, entry: &str, namespace: &str) {
        self.files.entry(entry.to_owned()).or_default().namespace = Some(namespace.to_owned());
    }

    pub(crate) fn file_failed(&self, entry: &str) -> bool {
        self.files.get(entry).is_some_and(|file| file.failed)
    }

    pub(crate) fn has_file(&self, entry: &str) -> bool {
        self.files.contains_key(entry)
    }

    /// The first resource keeps its partial value; a failed first read prevents
    /// later merges, while an invalid later resource leaves the earlier value intact.
    pub(crate) fn read_document(&mut self, entry: &str, text: &str) -> Option<Value> {
        if self.file_failed(entry) {
            return None;
        }
        let first = !self.has_file(entry);
        let (value, error) = json5::parse_partial(text);
        if first {
            self.files.insert(
                entry.to_owned(),
                FileRead {
                    namespace: None,
                    failed: error.is_some(),
                },
            );
        }
        if let Some(error) = error {
            self.note(format!("{entry}: parse error ({error})"));
            if !first {
                return None;
            }
        }
        Some(value)
    }

    pub(crate) fn note(&mut self, message: String) {
        self.diagnostics.push(message);
    }

    pub fn global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    pub fn globals(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.globals.iter()
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn namespace_count(&self) -> usize {
        self.defs.len()
    }
}

fn read_ui_defs(path: &Path) -> Result<Vec<String>, LoadError> {
    let text = read(path)?;
    parse_ui_defs(path, &text)
}

/// The `_ui_defs` paths sorted and deduplicated, the order the vanilla client loads them.
pub(crate) fn parse_ui_defs(path: &Path, text: &str) -> Result<Vec<String>, LoadError> {
    let value = json5::parse(text).map_err(|source| LoadError::Parse {
        path: path.to_path_buf(),
        message: source.to_string(),
    })?;
    let entries = value
        .get("ui_defs")
        .and_then(Value::as_array)
        .ok_or_else(|| LoadError::Shape {
            path: path.to_path_buf(),
        })?;
    let mut paths: Vec<String> = entries
        .iter()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn read(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("unexpected shape in {path}")]
    Shape { path: PathBuf },
}

#[cfg(test)]
mod overlay_tests {
    use super::Catalog;

    // A pack file redefining a control replaces the vanilla one and adds new ones.
    #[test]
    fn overlay_text_replaces_and_adds_controls() {
        let mut catalog = Catalog::default();
        catalog.overlay_text("ui/a.json", r#"{"namespace":"n","box":{"size":[1,1]}}"#);
        catalog.overlay_text(
            "ui/b.json",
            r#"{"namespace":"n","box":{"size":[2,2]},"extra":{}}"#,
        );
        catalog.overlay_globals_text(r#"{"$g": 3}"#);
        assert_eq!(catalog.lookup("n", "box").unwrap().props["size"][0], 2);
        assert!(catalog.lookup("n", "extra").is_some());
        assert_eq!(
            catalog.global("g").and_then(|value| value.as_i64()),
            Some(3)
        );
    }
}

#[cfg(test)]
mod document_tests {
    use super::Catalog;
    use crate::{Context, resolve};

    fn catalog(defs: &str, files: &[(&str, &str)]) -> Catalog {
        let mut all = vec![
            ("ui/_global_variables.json", "{}"),
            ("ui/_ui_defs.json", defs),
        ];
        all.extend_from_slice(files);
        Catalog::from_files(all.iter().map(|(path, text)| (*path, text.as_bytes()))).unwrap()
    }

    // `_ui_defs` paths load sorted and once each, so `ui/z.json` redefines last.
    #[test]
    fn ui_defs_are_sorted_and_deduplicated() {
        let catalog = catalog(
            r#"{"ui_defs":["ui/z.json","ui/a.json","ui/z.json"]}"#,
            &[
                ("ui/a.json", r#"{"namespace":"n","c":{"size":[1,1]}}"#),
                ("ui/z.json", r#"{"namespace":"n","c":{"size":[2,2]}}"#),
            ],
        );
        assert_eq!(catalog.lookup("n", "c").unwrap().props["size"][0], 2);
        assert_eq!(
            catalog.diagnostics().len(),
            1,
            "{:?}",
            catalog.diagnostics()
        );
    }

    // A pack file no `_ui_defs.json` lists is never registered.
    #[test]
    fn unlisted_pack_documents_are_not_loaded() {
        let mut catalog = catalog(
            r#"{"ui_defs":["ui/a.json"]}"#,
            &[("ui/a.json", r#"{"namespace":"a","c":{}}"#)],
        );
        catalog.apply_pack([(
            "ui/extra.json",
            br#"{"namespace":"extra","c":{"type":"panel"}}"#.as_slice(),
        )]);
        assert!(catalog.lookup("extra", "c").is_none());
        catalog.apply_pack([
            (
                "ui/_ui_defs.json",
                br#"{"ui_defs":["ui/extra.json"]}"#.as_slice(),
            ),
            (
                "ui/extra.json",
                br#"{"namespace":"extra","c":{}}"#.as_slice(),
            ),
        ]);
        assert!(catalog.lookup("extra", "c").is_some());
    }

    #[test]
    fn a_document_without_a_namespace_registers_under_root() {
        let catalog = catalog(
            r#"{"ui_defs":["ui/a.json"]}"#,
            &[("ui/a.json", r#"{"c":{"type":"panel"}}"#)],
        );
        assert!(catalog.lookup("_root", "c").is_some());
    }

    // `spacer_9.5` parses as namespace `spacer_9`, name `5`; a multi-member entry creates nothing.
    #[test]
    fn inline_names_follow_the_vanilla_name_parser() {
        let catalog = catalog(
            r#"{"ui_defs":["ui/a.json"]}"#,
            &[(
                "ui/a.json",
                r#"{"namespace":"a","root":{"type":"panel","controls":[
                    {"spacer_9.5":{"type":"panel"}},
                    {"x":{"type":"panel"},"y":{"type":"panel"}},
                    {"a.b@a.base":{}}]},
                  "base":{"type":"panel"}}"#,
            )],
        );
        let root = resolve(&catalog, "a.root", &Context::empty())
            .control
            .unwrap();
        let names: Vec<_> = root
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        assert_eq!(names, ["5", "a.b"]);
    }
}
