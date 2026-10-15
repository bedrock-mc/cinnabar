//! The operations the editor and the MCP server share, over a [`Session`]:
//! screen listing, resolved trees with provenance, validation, layout boxes,
//! picking and inspection. Both front ends only translate these to their wire.

use std::collections::BTreeSet;

use json_ui::{Catalog, ControlRef, ResolvedControl};
use serde::Serialize;
use serde_json::{Value, json};

use crate::diagnose::{self, Diagnostic};
use crate::provenance::{Inspection, Tracer};
use crate::scene::{Frame, LaidBox, Session, node_at};

/// A top-level control the picker offers.
#[derive(Clone, Debug, Serialize)]
pub struct ScreenEntry {
    pub reference: String,
    /// Its resolved `type` is `screen`.
    pub screen: bool,
    /// The client renders it through the engine today.
    pub engine: bool,
}

/// Every top-level control, screens first.
pub fn screens(session: &mut Session) -> Vec<ScreenEntry> {
    let catalog = session.catalog();
    let index = session.index();
    let mut out: Vec<ScreenEntry> = index
        .top_level()
        .into_iter()
        .map(|reference| ScreenEntry {
            screen: resolved_type(&catalog, &reference).as_deref() == Some("screen"),
            engine: json_ui::is_engine_screen(&reference),
            reference,
        })
        .collect();
    out.sort_by(|a, b| {
        b.screen
            .cmp(&a.screen)
            .then_with(|| a.reference.cmp(&b.reference))
    });
    out
}

/// Errors in the files an export would ship, shown before exporting.
pub fn export_warnings(session: &mut Session) -> Vec<Diagnostic> {
    let edited: Vec<String> = session
        .workspace
        .layers()
        .iter()
        .flat_map(|layer| {
            layer
                .ui_paths()
                .filter(|path| layer.is_edited(path))
                .map(str::to_owned)
        })
        .filter(|path| path != json_ui::UI_DEFS_PATH && path != json_ui::GLOBAL_VARIABLES_PATH)
        .collect();
    if edited.is_empty() {
        return Vec::new();
    }
    let mut out = validate(session, &edited, &json_ui::Context::empty());
    out.retain(|d| d.severity == diagnose::Severity::Error);
    out
}

/// Whether `reference` names a top-level control the catalog holds.
pub fn has_control(session: &mut Session, reference: &str) -> bool {
    let catalog = session.catalog();
    reference
        .split_once('.')
        .is_some_and(|(namespace, name)| catalog.lookup(namespace, name).is_some())
}

/// The control to preview for a file: its first top-level control whose type
/// resolves to `screen`, else its first top-level control. Nested overlays
/// (`a/b`) and animations are not previewable.
pub fn pick_screen(session: &mut Session, layer: usize, path: &str) -> Option<String> {
    let catalog = session.catalog();
    let text = session.workspace.layer(layer)?.text(path)?;
    let root = crate::outline::parse(&text).ok()?;
    let namespace = match root
        .get("namespace")
        .and_then(crate::outline::Spanned::as_str)
    {
        Some(namespace) => namespace.to_owned(),
        None => session
            .index()
            .file_namespace(layer, path)
            .map(str::to_owned)?,
    };
    let candidates: Vec<String> = root
        .members()
        .iter()
        .filter(|member| member.key != "namespace" && !member.key.contains('/'))
        .filter(|member| member.value.get("anim_type").is_none())
        .map(|member| {
            let name = member
                .key
                .split_once('@')
                .map_or(member.key.as_str(), |(name, _)| name);
            format!("{namespace}.{name}")
        })
        .filter(|reference| {
            let (namespace, name) = reference.split_once('.').unwrap_or_default();
            catalog.lookup(namespace, name).is_some()
        })
        .collect();
    candidates
        .iter()
        .find(|reference| resolved_type(&catalog, reference).as_deref() == Some("screen"))
        .or_else(|| candidates.first())
        .cloned()
}

/// The first `type` along `reference`'s literal base chain.
fn resolved_type(catalog: &Catalog, reference: &str) -> Option<String> {
    let (namespace, name) = reference.split_once('.')?;
    let mut current = ControlRef::new(namespace, name);
    for _ in 0..64 {
        let raw = catalog.lookup(&current.namespace, &current.name)?;
        if let Some(kind) = raw.props.get("type").and_then(Value::as_str) {
            return Some(kind.to_owned());
        }
        let base = raw.base.as_deref().filter(|base| !base.starts_with('$'))?;
        current = ControlRef::parse(base, &raw.owner_ns);
    }
    None
}

/// Options bounding a resolved-tree dump.
#[derive(Clone, Copy, Debug)]
pub struct TreeLimits {
    pub max_nodes: usize,
    pub properties: bool,
}

/// The bound tree of `frame` as JSON, each property with its source.
pub fn tree_json(
    session: &mut Session,
    reference: &str,
    frame: &Frame,
    limits: TreeLimits,
) -> Value {
    let catalog = session.catalog();
    let index = session.index();
    let tracer = Tracer {
        catalog: &catalog,
        index: &index,
        workspace: &session.workspace,
    };
    let mut budget = limits.max_nodes;
    let mut path = Vec::new();
    node_json(
        &tracer,
        reference,
        &frame.bound,
        &frame.bound,
        &mut path,
        &mut budget,
        limits,
    )
}

fn node_json(
    tracer: &Tracer,
    reference: &str,
    root: &ResolvedControl,
    node: &ResolvedControl,
    path: &mut Vec<usize>,
    budget: &mut usize,
    limits: TreeLimits,
) -> Value {
    if *budget == 0 {
        return json!({ "name": node.name, "truncated": true });
    }
    *budget -= 1;
    let mut out = json!({
        "name": node.name,
        "type": node.control_type,
        "base": node.base.as_ref().map(ToString::to_string),
    });
    if limits.properties
        && let Some(inspection) = tracer.inspect(reference, root, path)
    {
        out["definition"] = json!(inspection.definition);
        out["properties"] = json!(inspection.properties);
    }
    let children: Vec<Value> = node
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            path.push(index);
            let value = node_json(tracer, reference, root, child, path, budget, limits);
            path.pop();
            value
        })
        .collect();
    if !children.is_empty() {
        out["children"] = Value::Array(children);
    }
    out
}

/// Inspect the control behind `frame.boxes[index]`.
pub fn inspect(
    session: &mut Session,
    reference: &str,
    frame: &Frame,
    index: usize,
) -> Option<Inspection> {
    let laid = frame.boxes.get(index)?;
    node_at(&frame.bound, &laid.path)?;
    let catalog = session.catalog();
    let index = session.index();
    let tracer = Tracer {
        catalog: &catalog,
        index: &index,
        workspace: &session.workspace,
    };
    tracer.inspect(reference, &frame.bound, &laid.path)
}

/// The topmost visible box under virtual point `point`, by layer then order.
pub fn pick(frame: &Frame, point: [f64; 2]) -> Option<usize> {
    let visible = visible_boxes(&frame.boxes);
    frame
        .boxes
        .iter()
        .enumerate()
        .filter(|(index, laid)| {
            visible[*index]
                && laid.rect[2] > 0.0
                && laid.rect[3] > 0.0
                && point[0] >= laid.rect[0]
                && point[1] >= laid.rect[1]
                && point[0] < laid.rect[0] + laid.rect[2]
                && point[1] < laid.rect[1] + laid.rect[3]
        })
        .max_by_key(|(index, laid)| (laid.layer, *index))
        .map(|(index, _)| index)
}

/// Whether each box and all its ancestors are visible.
pub fn visible_boxes(boxes: &[LaidBox]) -> Vec<bool> {
    let mut out = Vec::with_capacity(boxes.len());
    for laid in boxes {
        let parent = laid.parent.is_none_or(|parent| out[parent]);
        out.push(parent && laid.visible);
    }
    out
}

/// Diagnostics for the named files (all files when empty): their load and
/// syntax problems plus a resolve of every control they define at top level.
pub fn validate(
    session: &mut Session,
    files: &[String],
    context: &json_ui::Context,
) -> Vec<Diagnostic> {
    let catalog = session.catalog();
    let index = session.index();
    let wanted: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let in_scope = |path: &str| wanted.is_empty() || wanted.contains(path);
    let mut messages = Vec::new();
    let mut lint = Vec::new();
    for reference in index.top_level() {
        if !index.defs(&reference).iter().any(|def| in_scope(&def.path)) {
            continue;
        }
        let resolution = json_ui::resolve(&catalog, &reference, context);
        messages.extend(resolution.diagnostics);
        if let Some(root) = &resolution.control {
            lint.extend(diagnose::lint_bindings(root));
        }
    }
    let mut out = diagnose::collect(&catalog, &index, &messages, &session.workspace);
    out.extend(lint);
    out.retain(|d| d.location.as_ref().is_none_or(|l| in_scope(&l.path)));
    let mut seen = BTreeSet::new();
    out.retain(|d| seen.insert(d.message.clone()));
    out
}
