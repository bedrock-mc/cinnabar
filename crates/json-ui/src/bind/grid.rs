//! Grid expansion: one `grid_item_template` instance per cell the grid holds,
//! the retained template, and the cell indices of grids listing their cells.
//! A virtualized grid builds only the cells around those in view; the rest are
//! placeholders laid out at the cell size.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use serde_json::Value;

use super::feed::collection_name;
use super::spec::{Binding, Kind};
use super::{Binder, Node, Scope, Src, with_index};
use crate::layout::{GRID_PLACEHOLDER_KEY, GRID_TEMPLATE_KEY};
use crate::tree::{ControlRef, ResolvedControl};

#[cfg(test)]
mod tests;

/// Collection grids with fewer cells always build every cell; a virtualized grid no layout
/// has shown yet builds this many first, as a list opens scrolled to its start.
const VIRTUAL_GRID_CELLS: usize = 64;

impl Binder<'_> {
    /// One `grid_item_template` instance per cell the grid holds (columns × rows,
    /// or `maximum_grid_items` when rescaling), then the template itself, kept
    /// for layout to measure. A grid filling its extent holds one per item.
    pub(super) fn expand_grid(
        &mut self,
        src: &Src,
        template: &ControlRef,
        scope: &Scope,
    ) -> Vec<Node> {
        let control = src.get();
        let collection = collection_name(control);
        let key = collection.map(|name| self.collection_key(name, scope));
        let count = grid_capacity(src)
            .unwrap_or_else(|| key.as_ref().map_or(0, |key| self.data.collection_len(key)))
            .min(super::feed::MAX_FACTORY_ITEMS);
        let Some(inner) = self.expansion_scope(scope, template) else {
            return Vec::new();
        };
        let Some(resolved) = self.resolve_scoped(template, control, &BTreeMap::new()) else {
            self.note(format!(
                "{}: grid template {template} unresolved",
                control.name
            ));
            return Vec::new();
        };
        let window = (self.state.virtualizes(collection) && count >= VIRTUAL_GRID_CELLS)
            .then(|| self.grid_window(scope.parent_key, &resolved))
            .flatten();
        let mut cells = Vec::with_capacity(count);
        for index in 0..count {
            if !self.can_create() {
                break;
            }
            if let Some((built, placeholder)) = &window
                && !built.contains(&index)
            {
                let cell = with_index(Src::root(Arc::clone(placeholder)), index);
                cells.push(self.placeholder(cell, &inner));
                continue;
            }
            let child_scope = match (collection, &key) {
                (Some(name), Some(key)) => inner.enter(name, key.clone(), index),
                _ => inner.clone(),
            };
            let cell = with_index(Src::root(Arc::clone(&resolved)), index).patched(|patch| {
                if let Some(name) = collection {
                    patch.properties.insert(
                        "collection_scope".to_owned(),
                        Value::String(name.to_owned()),
                    );
                }
            });
            cells.push(self.build(cell, &child_scope, 0));
        }
        let template_node = Src::root(resolved).patched(|patch| {
            patch
                .properties
                .insert(GRID_TEMPLATE_KEY.to_owned(), Value::Bool(true));
        });
        if let Some(template) = self.unbound(template_node) {
            cells.push(template);
        }
        cells
    }
}

impl Binder<'_> {
    /// The cells the virtualized grid at bind key `grid` builds and the placeholder standing in
    /// for the others, when the cell template has fixed pixel bounds no binding resizes, so a
    /// placeholder lays out exactly where its cell would.
    fn grid_window(
        &mut self,
        grid: u64,
        template: &Arc<ResolvedControl>,
    ) -> Option<(Range<usize>, Arc<ResolvedControl>)> {
        if binds_size(template) {
            return None;
        }
        let mut geometry = BTreeMap::new();
        for key in ["size", "min_size", "max_size"] {
            match template.properties.get(key) {
                Some(value) if pixel_pair(value).is_some() => {
                    geometry.insert(key.to_owned(), value.clone());
                }
                None if key != "size" => {}
                _ => return None,
            }
        }
        let placeholders = &mut self.state.placeholders;
        let placeholder = match placeholders
            .iter()
            .find(|(key, cell, _)| *key == grid && Arc::ptr_eq(cell, template))
        {
            Some((_, _, placeholder)) => Arc::clone(placeholder),
            None => {
                let placeholder = Arc::new(ResolvedControl {
                    name: template.name.clone(),
                    control_type: Some("panel".to_owned()),
                    base: None,
                    unresolved_base: None,
                    properties: geometry
                        .into_iter()
                        .chain([(GRID_PLACEHOLDER_KEY.to_owned(), Value::from(grid))])
                        .collect::<BTreeMap<_, _>>()
                        .into(),
                    children: Vec::new(),
                    factory: None,
                });
                placeholders.retain(|(key, _, _)| *key != grid);
                placeholders.push((grid, Arc::clone(template), Arc::clone(&placeholder)));
                placeholder
            }
        };
        let built = self
            .state
            .grid_windows
            .entry(grid)
            .or_insert(0..VIRTUAL_GRID_CELLS)
            .clone();
        Some((built, placeholder))
    }

    /// An out-of-view cell's placeholder, under the key its built cell has; one standing
    /// unchanged since the last bind is moved over.
    fn placeholder(&mut self, src: Src, scope: &Scope) -> Node {
        let key = self.control_key(&src, scope.parent_key);
        let prior = match self.pool.remove(&key) {
            Some(previous) if previous.src.same(&src) => return self.reuse(previous),
            Some(previous) => Some(self.replace(previous).1),
            None => None,
        };
        self.created += 1;
        self.state.built.push(key);
        Node {
            src,
            key,
            layout_key: String::new(),
            own: BTreeMap::new(),
            native: Default::default(),
            memory: Default::default(),
            bindings: Arc::default(),
            children: Vec::new(),
            deferred: None,
            retained: false,
            scope: scope.clone(),
            track: super::reuse::Track::built(Default::default(), false, None, prior),
        }
    }

    /// Measures a template without binding while retaining the common creation budget.
    fn unbound(&mut self, src: Src) -> Option<Node> {
        if !self.can_create() {
            return None;
        }
        self.created += 1;
        let mut children = Vec::new();
        for index in 0..src.get().children.len() {
            let Some(child) = self.unbound(src.child(index)) else {
                break;
            };
            children.push(child);
        }
        Some(Node {
            src,
            key: 0,
            layout_key: String::new(),
            own: BTreeMap::new(),
            native: Default::default(),
            memory: Default::default(),
            bindings: Arc::default(),
            children,
            deferred: None,
            retained: false,
            scope: Scope::default(),
            track: super::reuse::Track::built(Default::default(), false, None, None),
        })
    }
}

/// A `[w, h]` of plain numbers.
fn pixel_pair(value: &Value) -> Option<[f64; 2]> {
    match value.as_array()?.as_slice() {
        [w, h] => Some([w.as_f64()?, h.as_f64()?]),
        _ => None,
    }
}

/// Whether one of `control`'s own bindings writes its size.
fn binds_size(control: &ResolvedControl) -> bool {
    let Some(Value::Array(bindings)) = control.properties.get("bindings") else {
        return false;
    };
    bindings.iter().any(|binding| {
        [
            "binding_name",
            "binding_name_override",
            "target_property_name",
        ]
        .iter()
        .filter_map(|key| binding.get(*key)?.as_str())
        .any(|name| name.starts_with("#size_binding"))
    })
}

/// A templated grid's cell count, or `None` when a filling grid's count waits
/// for layout: `maximum_grid_items` (a bound value already patched in, only an
/// integer counting) when rescaling, else columns × rows.
pub(super) fn grid_capacity(src: &Src) -> Option<usize> {
    if src.get().control_type.as_deref() != Some("grid") {
        return None;
    }
    let direction = |key: &str| {
        src.prop(key)
            .and_then(Value::as_str)
            .is_some_and(|value| value == "horizontal" || value == "vertical")
    };
    if direction("grid_rescaling_type") {
        let max = src
            .prop("maximum_grid_items")
            .and_then(Value::as_f64)
            .filter(|max| max.fract() == 0.0 && *max >= 0.0);
        return Some(max.map_or(0, |max| max as usize));
    }
    if direction("grid_fill_direction") {
        return None;
    }
    let dims = src.prop("grid_dimensions").and_then(Value::as_array);
    let int = |index: usize| {
        dims.and_then(|dims| dims.get(index)?.as_i64())
            .map_or(0, |value| value.max(0) as usize)
    };
    Some(int(0).saturating_mul(int(1)))
}

/// Whether a templated grid's cell count waits on views, which settle only after
/// the tree is built.
pub(super) fn grid_awaits_views(control: &ResolvedControl, bindings: &[Binding]) -> bool {
    grid_template(control).is_some()
        && bindings
            .iter()
            .any(|binding| matches!(binding.kind, Kind::View { .. }))
}

/// The column count of a collection grid that lists its cells as children.
pub(super) fn static_grid_columns(src: &Src) -> Option<u64> {
    let control = src.get();
    if control.control_type.as_deref() != Some("grid")
        || !control.properties.contains_key("collection_name")
    {
        return None;
    }
    src.prop("grid_dimensions")?
        .as_array()?
        .first()?
        .as_u64()
        .filter(|columns| *columns > 0)
}

/// A static grid cell's collection index, row-major from its `grid_position`.
pub(super) fn grid_cell_index(control: &ResolvedControl, columns: u64) -> Option<usize> {
    if control.properties.contains_key("collection_index") {
        return None;
    }
    let position = control.properties.get("grid_position")?.as_array()?;
    let (column, row) = (position.first()?.as_u64()?, position.get(1)?.as_u64()?);
    usize::try_from(row.checked_mul(columns)?.checked_add(column)?).ok()
}

/// The `grid_item_template` of a `grid`.
pub(super) fn grid_template(control: &ResolvedControl) -> Option<ControlRef> {
    if control.control_type.as_deref() != Some("grid") {
        return None;
    }
    let template = control
        .properties
        .get("grid_item_template")?
        .as_str()
        .filter(|template| !template.is_empty())?;
    let owner = control
        .base
        .as_ref()
        .map_or("", |base| base.namespace.as_str());
    Some(ControlRef::parse(template, owner))
}
