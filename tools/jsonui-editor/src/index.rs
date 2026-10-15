//! Where each control is written: every top-level, nested, `a/b` overlay and
//! `modifications`-inserted definition of every file, keyed by its site
//! (`namespace.top/child/...`) with the key ranges of the properties it sets.
//! Provenance and jump-to-definition read this; resolution never does.

use std::collections::HashMap;

use serde::Serialize;

use crate::outline::{self, Spanned};
use crate::workspace::Workspace;

/// One file's contribution to a site.
#[derive(Clone, Debug, Serialize)]
pub struct Def {
    pub layer: usize,
    pub path: String,
    /// Load position within the layer; higher applies later.
    pub order: usize,
    pub start: usize,
    pub end: usize,
    /// The `@base` written on this definition's key, if any.
    pub base: Option<String>,
    /// Property key -> the byte range of its key and of its value.
    #[serde(skip)]
    pub props: HashMap<String, [usize; 4]>,
}

/// Every definition site in the workspace, plus each file's namespace.
#[derive(Default)]
pub struct Index {
    sites: HashMap<String, Vec<Def>>,
    /// Top-level control name -> the namespaces defining it.
    tops: HashMap<String, Vec<String>>,
    /// Each indexed file's namespace, by `(layer, path)`.
    files: HashMap<(usize, String), String>,
    /// Syntax errors per `(layer, path)`.
    pub syntax: Vec<(usize, String, outline::SyntaxError)>,
}

impl Index {
    pub fn build(workspace: &Workspace) -> Self {
        let mut index = Index::default();
        let mut namespaces: HashMap<String, String> = HashMap::new();
        for (layer_index, layer) in workspace.layers().iter().enumerate() {
            for (order, path) in layer
                .ui_paths()
                .filter(|path| {
                    *path != json_ui::UI_DEFS_PATH && *path != json_ui::GLOBAL_VARIABLES_PATH
                })
                .enumerate()
            {
                let Some(text) = layer.text(path) else {
                    continue;
                };
                let root = match outline::parse(&text) {
                    Ok(root) => root,
                    Err(error) => {
                        index.syntax.push((layer_index, path.to_owned(), error));
                        continue;
                    }
                };
                let namespace = match root.get("namespace").and_then(Spanned::as_str) {
                    Some(namespace) => namespace.to_owned(),
                    None => match namespaces.get(path) {
                        Some(namespace) => namespace.clone(),
                        None => continue,
                    },
                };
                namespaces.insert(path.to_owned(), namespace.clone());
                index
                    .files
                    .insert((layer_index, path.to_owned()), namespace.clone());
                let file = FileCtx {
                    layer: layer_index,
                    path,
                    order,
                };
                for member in root.members().iter().filter(|m| m.key != "namespace") {
                    let (name, base) = split_key(&member.key);
                    let site = format!("{namespace}.{name}");
                    if !name.contains('/') {
                        index
                            .tops
                            .entry(name.to_owned())
                            .or_default()
                            .push(namespace.clone());
                    }
                    let span = [member.key_start, member.key_end];
                    index.walk(&file, site, base, span, &member.value);
                }
            }
        }
        for namespaces in index.tops.values_mut() {
            namespaces.sort();
            namespaces.dedup();
        }
        index
    }

    fn walk(
        &mut self,
        file: &FileCtx,
        site: String,
        base: Option<&str>,
        span: [usize; 2],
        body: &Spanned,
    ) {
        let mut props = HashMap::new();
        for member in body.members() {
            props.insert(
                member.key.clone(),
                [
                    member.key_start,
                    member.key_end,
                    member.value.start,
                    member.value.end,
                ],
            );
            match member.key.as_str() {
                "controls" => self.children(file, &site, &member.value),
                "modifications" => {
                    for modification in member.value.items() {
                        let is_controls = modification
                            .get("array_name")
                            .and_then(Spanned::as_str)
                            .is_none_or(|name| name == "controls");
                        if let (true, Some(value)) = (is_controls, modification.get("value")) {
                            match value.items() {
                                [] => self.children_entry(file, &site, value),
                                _ => self.children(file, &site, value),
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.sites.entry(site).or_default().push(Def {
            layer: file.layer,
            path: file.path.to_owned(),
            order: file.order,
            start: span[0],
            end: span[1],
            base: base.map(str::to_owned),
            props,
        });
    }

    fn children(&mut self, file: &FileCtx, site: &str, array: &Spanned) {
        for entry in array.items() {
            self.children_entry(file, site, entry);
        }
    }

    fn children_entry(&mut self, file: &FileCtx, site: &str, entry: &Spanned) {
        for member in entry.members() {
            let (name, base) = split_key(&member.key);
            let child = format!("{site}/{name}");
            let span = [member.key_start, member.key_end];
            self.walk(file, child, base, span, &member.value);
        }
    }

    /// Definitions of `site`, latest-applied first.
    pub fn defs(&self, site: &str) -> Vec<&Def> {
        let mut defs: Vec<&Def> = self.sites.get(site).into_iter().flatten().collect();
        defs.sort_by_key(|def| std::cmp::Reverse((def.layer, def.order)));
        defs
    }

    /// Definitions of `namespace.name`, or of the first nested site in that
    /// namespace whose last instance is `name` (engine messages name nested
    /// controls by instance only).
    pub fn defs_named(&self, reference: &str) -> Vec<&Def> {
        let direct = self.defs(reference);
        if !direct.is_empty() {
            return direct;
        }
        let Some((namespace, name)) = reference.split_once('.') else {
            return Vec::new();
        };
        let prefix = format!("{namespace}.");
        let suffix = format!("/{name}");
        let mut nested: Vec<&String> = self
            .sites
            .keys()
            .filter(|site| site.starts_with(&prefix) && site.ends_with(&suffix))
            .collect();
        nested.sort();
        nested.first().map_or_else(Vec::new, |site| self.defs(site))
    }

    /// The namespace `path` in `layer` defines or extends.
    pub fn file_namespace(&self, layer: usize, path: &str) -> Option<&str> {
        self.files
            .get(&(layer, path.to_owned()))
            .map(String::as_str)
    }

    pub fn has_site(&self, site: &str) -> bool {
        self.sites.contains_key(site)
    }

    /// Namespaces with a top-level control named `name`.
    pub fn namespaces_of(&self, name: &str) -> &[String] {
        self.tops.get(name).map_or(&[], Vec::as_slice)
    }

    /// Every top-level `namespace.name`, sorted.
    pub fn top_level(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .tops
            .iter()
            .flat_map(|(name, namespaces)| namespaces.iter().map(move |ns| format!("{ns}.{name}")))
            .collect();
        all.sort();
        all
    }
}

struct FileCtx<'a> {
    layer: usize,
    path: &'a str,
    order: usize,
}

fn split_key(key: &str) -> (&str, Option<&str>) {
    match key.split_once('@') {
        Some((name, base)) => (name, Some(base)),
        None => (key, None),
    }
}
