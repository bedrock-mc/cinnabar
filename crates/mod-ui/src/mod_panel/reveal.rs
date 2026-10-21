//! One-time collection scrolling over cached, admitted native grid geometry.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{Catalog, LayoutEnv, ResolvedControl, ScrollMetrics, ViewState};
use ui::mod_panel::{CollectionReveal, Panel};

use super::{SCREEN, motion::PanelMotion};

#[derive(Default)]
pub(super) struct CollectionReveals {
    pending: BTreeMap<String, Arc<CollectionReveal>>,
    catalog: Option<Arc<Catalog>>,
    root: Option<ResolvedControl>,
    geometry: BTreeMap<String, Geometry>,
    pub(super) ignored: u64,
}

struct Geometry {
    key: String,
    height: f64,
    columns: usize,
}

impl CollectionReveals {
    /// Queues newly admitted identities, including repeated requests for the same item.
    pub(super) fn publish(&mut self, previous: Option<&Panel>, next: &Panel) {
        let Some(surface) = &next.surface else {
            self.pending.clear();
            return;
        };
        self.pending.retain(|name, request| {
            surface
                .collections
                .get(name)
                .and_then(|collection| collection.reveal.as_ref())
                .is_some_and(|next| Arc::ptr_eq(request, next))
        });
        for (name, collection) in &surface.collections {
            let Some(request) = &collection.reveal else {
                continue;
            };
            let previous = previous
                .and_then(|panel| panel.surface.as_ref())
                .and_then(|surface| surface.collections.get(name))
                .and_then(|collection| collection.reveal.as_ref());
            if previous.is_none_or(|previous| !Arc::ptr_eq(previous, request)) {
                self.pending.insert(name.clone(), Arc::clone(request));
            }
        }
    }

    /// Consumes requests before binding the visible grid window; a complete cell includes
    /// the guest's card padding. Reveals snap and cancel pending wheel easing.
    pub(super) fn apply(
        &mut self,
        panel: &Panel,
        catalog: &Arc<Catalog>,
        view: &mut ViewState,
        motion: &mut PanelMotion,
        env: &LayoutEnv<'_>,
    ) {
        if self.pending.is_empty() {
            return;
        }
        if self
            .catalog
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, catalog))
        {
            self.root = json_ui::resolve(catalog, SCREEN, &crate::personal_context()).control;
            self.catalog = Some(Arc::clone(catalog));
            self.geometry.clear();
        }
        while let Some((name, request)) = self.pending.pop_first() {
            let Some(collection) = panel
                .surface
                .as_ref()
                .and_then(|s| s.collections.get(&name))
            else {
                continue;
            };
            let Some(index) = collection
                .order
                .iter()
                .position(|item| *item == request.item as usize)
            else {
                self.ignored = self.ignored.saturating_add(1);
                tracing::warn!(collection = %name, item = request.item, ignored = self.ignored,
                    "personal panel reveal ignored because its item is filtered out");
                continue;
            };
            if !self.geometry.contains_key(&name) {
                let Some(root) = &self.root else { continue };
                let Some((key, grid)) = grid(root, "", "", &name, 0) else {
                    continue;
                };
                let Some(template) = grid
                    .properties
                    .get("grid_item_template")
                    .and_then(|v| v.as_str())
                    .and_then(|reference| {
                        json_ui::resolve(catalog, reference, &crate::personal_context()).control
                    })
                else {
                    continue;
                };
                let Some(width) = grid
                    .properties
                    .get("size")
                    .and_then(|v| v.as_array())
                    .and_then(|size| size.first())
                    .and_then(|v| v.as_f64())
                else {
                    continue;
                };
                let cell =
                    json_ui::layout(&template, [width, collection.definition.viewport[1]], env)
                        .rect;
                self.geometry.insert(
                    name.clone(),
                    Geometry {
                        key,
                        height: cell.h,
                        columns: json_ui::grid_template_columns(width, cell.w),
                    },
                );
            }
            let geometry = &self.geometry[&name];
            let rows = collection
                .order
                .len()
                .div_ceil(collection.definition.columns);
            let metrics = ScrollMetrics {
                offset: view.scroll_offset(&geometry.key),
                content: rows as f64 * geometry.height,
                viewport: collection.definition.viewport[1],
                ..Default::default()
            };
            let top = (index / geometry.columns) as f64 * geometry.height - metrics.offset;
            let offset = metrics.offset_revealing(top, top + geometry.height);
            motion.cancel_scroll(&geometry.key);
            if offset != metrics.offset {
                view.scroll.insert(geometry.key.clone(), offset);
            }
        }
    }
}

/// Finds the native grid and its enclosing scroll key using layout's instance identities.
fn grid<'a>(
    control: &'a ResolvedControl,
    parent: &str,
    scroll: &str,
    name: &str,
    repeat: usize,
) -> Option<(String, &'a ResolvedControl)> {
    let key = json_ui::child_key(parent, control, repeat);
    let scroll = if control.control_type.as_deref() == Some("scroll_view") {
        &key
    } else {
        scroll
    };
    if control.control_type.as_deref() == Some("grid")
        && control
            .properties
            .get("collection_name")
            .and_then(|v| v.as_str())
            == Some(name)
    {
        return Some((scroll.to_owned(), control));
    }
    for (index, child) in control.children.iter().enumerate() {
        let repeat = control.children[..index]
            .iter()
            .filter(|sibling| sibling.name == child.name)
            .count();
        if let Some(found) = grid(child, &key, scroll, name, repeat) {
            return Some(found);
        }
    }
    None
}
