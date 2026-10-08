//! Laid-out engine screens reused while their inputs are unchanged, so a static
//! menu only repaints each frame; hover, press and focus are gated in the
//! layout and only repaint. Resolved trees are reused across value changes,
//! which only re-bind and re-lay out.

use std::sync::{Arc, Mutex};

use json_ui::{Catalog, Context, DataSource, FormRender, ResolvedControl, ViewState};

/// Screens kept at once: a menu, its overlay and a dialog popup.
const SLOTS: usize = 4;
/// Resolved trees of `cache_screen` screens kept beyond [`SLOTS`].
const CACHED_SLOTS: usize = 8;

/// Everything a screen's layout depends on besides the catalog's contents.
pub(super) struct ScreenKey<'a> {
    pub(super) reference: &'a str,
    pub(super) catalog: &'a Arc<Catalog>,
    pub(super) context: &'a Context,
    pub(super) data: &'a DataSource,
    /// Only its scroll offsets key the layout.
    pub(super) view: &'a ViewState,
    pub(super) root: [f64; 2],
    pub(super) px: f32,
    /// The language tables text measures with.
    pub(super) text: [usize; 3],
}

struct Entry {
    reference: String,
    catalog: Arc<Catalog>,
    context: Context,
    data: DataSource,
    view: ViewState,
    root: [f64; 2],
    px: f32,
    text: [usize; 3],
    render: Arc<FormRender>,
}

impl Entry {
    fn matches(&self, key: &ScreenKey<'_>) -> bool {
        self.reference == key.reference
            && Arc::ptr_eq(&self.catalog, key.catalog)
            && self.root == key.root
            && self.px == key.px
            && self.text == key.text
            && self.view.same_layout(key.view)
            && self.context == *key.context
            && self.data == *key.data
    }
}

/// A screen's resolved tree, which depends only on the catalog and context.
struct Resolved {
    reference: String,
    catalog: Arc<Catalog>,
    context: Context,
    root: Arc<ResolvedControl>,
    /// The screen asks to stay cached once closed (`cache_screen`).
    cached: bool,
}

/// Makes room for one more tree: the oldest whose screen does not ask to stay
/// cached leaves first, as vanilla retains `cache_screen` visual trees.
fn make_room(entries: &mut Vec<Resolved>) {
    let uncached = entries.iter().filter(|entry| !entry.cached).count();
    if uncached >= SLOTS || entries.len() >= SLOTS + CACHED_SLOTS {
        let index = entries.iter().position(|entry| !entry.cached).unwrap_or(0);
        entries.remove(index);
    }
}

#[derive(Default)]
pub(super) struct ScreenCache {
    laid: Mutex<Vec<Entry>>,
    resolved: Mutex<Vec<Resolved>>,
    /// Each screen's live bindings across data refreshes.
    bindings: Mutex<Vec<Bound>>,
}

/// A screen's binding state, which lives as long as its resolved tree.
struct Bound {
    reference: String,
    catalog: Arc<Catalog>,
    context: Context,
    state: json_ui::BindState,
}

impl ScreenCache {
    /// The cached render for `key`, else `render()`'s, remembered in place of
    /// the oldest entry.
    pub(super) fn get_or_render(
        &self,
        key: ScreenKey<'_>,
        render: impl FnOnce() -> Option<FormRender>,
    ) -> Option<Arc<FormRender>> {
        let mut entries = lock(&self.laid);
        if let Some(index) = entries.iter().position(|entry| entry.matches(&key)) {
            let entry = entries.remove(index);
            let render = Arc::clone(&entry.render);
            entries.push(entry);
            return Some(render);
        }
        drop(entries);
        let rendered = Arc::new(render()?);
        let mut entries = lock(&self.laid);
        if entries.len() >= SLOTS {
            entries.remove(0);
        }
        entries.push(Entry {
            reference: key.reference.to_owned(),
            catalog: Arc::clone(key.catalog),
            context: key.context.clone(),
            data: key.data.clone(),
            view: key.view.layout_part(),
            root: key.root,
            px: key.px,
            text: key.text,
            render: Arc::clone(&rendered),
        });
        Some(rendered)
    }

    /// An allow-listed screen's render for `key`: cached, else bound and laid
    /// out over its cached resolved tree.
    pub(super) fn render(
        &self,
        key: ScreenKey<'_>,
        env: &json_ui::LayoutEnv,
    ) -> Option<Arc<FormRender>> {
        let (reference, catalog, context, data, view, root) = (
            key.reference,
            key.catalog,
            key.context,
            key.data,
            key.view,
            key.root,
        );
        self.get_or_render(key, || {
            if !json_ui::is_engine_screen(reference) {
                return None;
            }
            let tree = self.resolved(reference, catalog, context, || {
                json_ui::resolve(catalog, reference, context).control
            })?;
            let library = json_ui::CatalogLibrary { catalog, context };
            let bound = self.with_binding(reference, catalog, context, |state| {
                json_ui::bind_stateful(&tree, data, &library, state).0
            });
            let measures = &mut json_ui::MeasureCache::default();
            Some(json_ui::render_bound_gated(
                bound, root, env, view, measures,
            ))
        })
    }

    /// Run `bind` over `reference`'s binding state, created on first use.
    fn with_binding<T>(
        &self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        bind: impl FnOnce(&mut json_ui::BindState) -> T,
    ) -> T {
        let resolved = lock(&self.resolved);
        let mut bindings = lock(&self.bindings);
        bindings.retain(|bound| {
            resolved.iter().any(|tree| {
                bound.reference == tree.reference
                    && Arc::ptr_eq(&bound.catalog, &tree.catalog)
                    && bound.context == tree.context
            })
        });
        drop(resolved);
        let index = match bindings.iter().position(|bound| {
            bound.reference == reference
                && Arc::ptr_eq(&bound.catalog, catalog)
                && bound.context == *context
        }) {
            Some(index) => index,
            None => {
                bindings.push(Bound {
                    reference: reference.to_owned(),
                    catalog: Arc::clone(catalog),
                    context: context.clone(),
                    state: json_ui::BindState::new(),
                });
                bindings.len() - 1
            }
        };
        bind(&mut bindings[index].state)
    }

    /// The resolved tree of `reference` under `context`, else `resolve()`'s.
    pub(super) fn resolved(
        &self,
        reference: &str,
        catalog: &Arc<Catalog>,
        context: &Context,
        resolve: impl FnOnce() -> Option<ResolvedControl>,
    ) -> Option<Arc<ResolvedControl>> {
        let same = |entry: &Resolved| {
            entry.reference == reference
                && Arc::ptr_eq(&entry.catalog, catalog)
                && entry.context == *context
        };
        if let Some(entry) = lock(&self.resolved).iter().find(|entry| same(entry)) {
            return Some(Arc::clone(&entry.root));
        }
        let root = Arc::new(resolve()?);
        let mut entries = lock(&self.resolved);
        if !entries.iter().any(same) {
            make_room(&mut entries);
            entries.push(Resolved {
                reference: reference.to_owned(),
                catalog: Arc::clone(catalog),
                context: context.clone(),
                cached: json_ui::ScreenSettings::from_properties(&root.properties).cache_screen,
                root: Arc::clone(&root),
            });
        }
        Some(root)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render() -> Option<FormRender> {
        Some(FormRender {
            bound: json_ui::ResolvedControl {
                name: "root".to_owned(),
                control_type: None,
                base: None,
                unresolved_base: None,
                properties: Default::default(),
                children: Vec::new(),
                factory: None,
            },
            nodes: Vec::new(),
            hits: Arc::from([]),
            report: Default::default(),
            cancel_target: None,
            root_panel: None,
        })
    }

    // A second frame with the same inputs reuses the render; any change redoes it.
    #[test]
    fn unchanged_inputs_reuse_the_render() {
        let cache = ScreenCache::default();
        let catalog = Arc::new(Catalog::default());
        let (context, data, view) = (Context::desktop(), DataSource::new(), ViewState::default());
        let key = |root: [f64; 2]| ScreenKey {
            reference: "start.start_screen",
            catalog: &catalog,
            context: &context,
            data: &data,
            view: &view,
            root,
            px: 2.0,
            text: [0; 3],
        };
        let first = cache.get_or_render(key([400.0, 300.0]), render).unwrap();
        let again = cache
            .get_or_render(key([400.0, 300.0]), || panic!("rendered twice"))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        let resized = cache.get_or_render(key([500.0, 300.0]), render).unwrap();
        assert!(!Arc::ptr_eq(&first, &resized));
        let tree = || render().map(|render| render.bound);
        let once = cache
            .resolved("start.start_screen", &catalog, &context, tree)
            .unwrap();
        let again = cache
            .resolved("start.start_screen", &catalog, &context, || {
                panic!("resolved twice")
            })
            .unwrap();
        assert!(Arc::ptr_eq(&once, &again));
    }

    // A `cache_screen` tree survives the ordinary trees resolved after it.
    #[test]
    fn cache_screen_trees_outlive_ordinary_slots() {
        let cache = ScreenCache::default();
        let catalog = Arc::new(Catalog::default());
        let context = Context::desktop();
        let tree = |cached: bool| {
            let mut root = render().unwrap().bound;
            root.properties
                .insert("cache_screen".to_owned(), serde_json::Value::Bool(cached));
            Some(root)
        };
        cache.resolved("pause.pause_screen", &catalog, &context, || tree(true));
        for index in 0..SLOTS + 1 {
            let reference = format!("screen.{index}");
            cache.resolved(&reference, &catalog, &context, || tree(false));
        }
        cache
            .resolved("pause.pause_screen", &catalog, &context, || {
                panic!("evicted")
            })
            .unwrap();
    }
    #[test]
    fn review_cached_screen_keeps_bindings_with_its_resolved_tree() {
        let cache = ScreenCache::default();
        let catalog = Arc::new(Catalog::default());
        let context = Context::desktop();
        let mut root = render().unwrap().bound;
        root.properties
            .insert("cache_screen".to_owned(), serde_json::json!(true));
        cache.resolved("pause.pause_screen", &catalog, &context, || Some(root));
        cache.with_binding("pause.pause_screen", &catalog, &context, |state| {
            state.publish("/root", "#remembered", json_ui::Scalar::Num(42.0));
        });
        for index in 0..SLOTS + 1 {
            let reference = format!("screen.{index}");
            cache.resolved(&reference, &catalog, &context, || {
                Some(render().unwrap().bound)
            });
            cache.with_binding(&reference, &catalog, &context, |_| {});
        }
        cache.with_binding("pause.pause_screen", &catalog, &context, |state| {
            assert_eq!(
                state.value("/root", "#remembered"),
                Some(&json_ui::Scalar::Num(42.0))
            );
        });
    }
}
