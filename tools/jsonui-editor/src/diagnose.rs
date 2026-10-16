//! Diagnostics for the panel and the MCP `validate` tool: the engine's own load,
//! resolve and bind messages located in the files they came from, syntax errors
//! with exact positions, a binding lint, and textures no layer provides.

use std::collections::BTreeSet;

use json_ui::{Catalog, ResolvedControl};
use serde::Serialize;
use serde_json::Value;

use crate::index::Index;
use crate::outline::line_col;
use crate::workspace::Workspace;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// `load`, `syntax`, `resolve`, `bind`, `binding` or `texture`.
    pub stage: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Location {
    pub layer: usize,
    pub path: String,
    pub start: usize,
    pub end: usize,
    /// Zero-based.
    pub line: usize,
    pub column: usize,
}

impl Location {
    pub fn at(workspace: &Workspace, layer: usize, path: &str, start: usize, end: usize) -> Self {
        let (line, column) = workspace
            .layer(layer)
            .and_then(|l| l.text(path))
            .map_or((0, 0), |text| line_col(&text, start));
        Self {
            layer,
            path: path.to_owned(),
            start,
            end,
            line,
            column,
        }
    }
}

/// Load, syntax and resolve/bind diagnostics, each located where possible.
pub fn collect(
    catalog: &Catalog,
    index: &Index,
    messages: &[String],
    workspace: &Workspace,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if let Some(error) = workspace.base_error() {
        out.push(Diagnostic {
            severity: Severity::Warning,
            stage: "load",
            message: format!("bottom layer loaded as an overlay: {error}"),
            location: None,
        });
    }
    let mut syntax_files = BTreeSet::new();
    for (layer, path, error) in &index.syntax {
        syntax_files.insert(path.clone());
        out.push(Diagnostic {
            severity: Severity::Error,
            stage: "syntax",
            message: format!("{path}: {}", error.message),
            location: Some(Location::at(
                workspace,
                *layer,
                path,
                error.offset,
                error.offset + 1,
            )),
        });
    }
    for message in catalog.diagnostics() {
        // A parse failure is already reported with its exact position.
        if message.contains("parse error")
            && syntax_files.iter().any(|p| message.contains(p.as_str()))
        {
            continue;
        }
        out.push(located(message, "load", index, workspace));
    }
    let mut seen = BTreeSet::new();
    for message in messages {
        if seen.insert(message) {
            out.push(located(message, stage_of(message), index, workspace));
        }
    }
    out
}

fn stage_of(message: &str) -> &'static str {
    if message.contains("factory") || message.contains("grid template") {
        "bind"
    } else {
        "resolve"
    }
}

fn severity_of(message: &str) -> Severity {
    if message.contains("not found")
        || message.contains("unknown control")
        || message.contains("parse error")
        || message.contains("cycle")
        || message.contains("unresolved")
    {
        Severity::Error
    } else if message.contains("redefined") {
        Severity::Info
    } else {
        Severity::Warning
    }
}

/// Place an engine message: a `pack <path>:` or `<path>:` prefix names its
/// file, a `namespace.name:` prefix its definition.
fn located(message: &str, stage: &'static str, index: &Index, workspace: &Workspace) -> Diagnostic {
    let head = message.split(": ").next().unwrap_or_default();
    let head = head.strip_prefix("pack ").unwrap_or(head);
    let location = if workspace
        .layers()
        .iter()
        .any(|layer| layer.ui_paths().any(|path| path == head))
    {
        let layer = (0..workspace.layers().len())
            .rev()
            .find(|layer| workspace.layers()[*layer].file(head).is_some());
        layer.map(|layer| Location::at(workspace, layer, head, 0, 0))
    } else {
        index
            .defs_named(head)
            .first()
            .map(|def| Location::at(workspace, def.layer, &def.path, def.start, def.end))
    };
    Diagnostic {
        severity: severity_of(message),
        stage,
        message: message.to_owned(),
        location,
    }
}

const BINDING_TYPES: &[&str] = &["global", "collection", "collection_details", "view", "none"];

/// Bindings the engine would skip silently: malformed entries, collection
/// bindings with no collection, and views naming a missing source control.
pub fn lint_bindings(root: &ResolvedControl) -> Vec<Diagnostic> {
    let mut names = BTreeSet::new();
    collect_names(root, &mut names);
    let mut out = Vec::new();
    lint_node(root, None, &names, &mut out);
    out
}

fn collect_names<'a>(node: &'a ResolvedControl, names: &mut BTreeSet<&'a str>) {
    names.insert(node.name.as_str());
    for child in &node.children {
        collect_names(child, names);
    }
}

fn lint_node(
    node: &ResolvedControl,
    collection: Option<&str>,
    names: &BTreeSet<&str>,
    out: &mut Vec<Diagnostic>,
) {
    let collection = node
        .properties
        .get("collection_name")
        .and_then(Value::as_str)
        .or(collection);
    let label = match &node.base {
        Some(base) => format!("{}@{base}", node.name),
        None => node.name.clone(),
    };
    let mut report = |message: String| {
        out.push(Diagnostic {
            severity: Severity::Warning,
            stage: "binding",
            message: format!("{label}: {message}"),
            location: None,
        });
    };
    if let Some(bindings) = node.properties.get("bindings") {
        let Some(bindings) = bindings.as_array() else {
            report("`bindings` is not an array".into());
            return;
        };
        for binding in bindings {
            let Some(binding) = binding.as_object() else {
                report("binding entry is not an object".into());
                continue;
            };
            let kind = binding
                .get("binding_type")
                .and_then(Value::as_str)
                .unwrap_or("global");
            if !BINDING_TYPES.contains(&kind) {
                report(format!("unknown binding_type `{kind}`"));
            }
            let name = binding.get("binding_name").and_then(Value::as_str);
            if let Some(name) = name
                && !(name.starts_with('#') || name.starts_with('('))
            {
                report(format!("binding_name `{name}` does not start with `#`"));
            }
            match kind {
                "collection" | "collection_details"
                    if collection.is_none() && !binding.contains_key("binding_collection_name") =>
                {
                    report(format!("{kind} binding with no collection_name in scope"));
                }
                "view" => {
                    if !binding.contains_key("source_property_name")
                        || !binding.contains_key("target_property_name")
                    {
                        report(
                            "view binding needs source_property_name and target_property_name"
                                .into(),
                        );
                    }
                    if let Some(source) = binding.get("source_control_name").and_then(Value::as_str)
                        && !names.contains(source)
                    {
                        report(format!(
                            "view source control `{source}` is not in this screen"
                        ));
                    }
                }
                "global" | "collection" | "collection_details" if name.is_none() => {
                    report(format!("{kind} binding has no binding_name"));
                }
                _ => {}
            }
        }
    }
    for child in &node.children {
        lint_node(child, collection, names, out);
    }
}

/// Textures a control names that no layer holds or that failed to decode.
pub fn textures(missing: &[String]) -> Vec<Diagnostic> {
    missing
        .iter()
        .map(|path| Diagnostic {
            severity: Severity::Warning,
            stage: "texture",
            message: format!("texture `{path}` is in no loaded layer (or failed to decode)"),
            location: None,
        })
        .collect()
}
