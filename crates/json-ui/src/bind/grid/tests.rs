//! A virtualized grid draws and takes input exactly as one built in full, and only grids
//! opted in virtualize.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    BindState, CollectionItem, ControlLibrary, ControlRef, DataSource, LayoutEnv, MeasureCache,
    ResolvedControl, Scalar, TextMeasure, TextureMeta, TextureSource, ViewState,
};

const CELLS: usize = 300;

fn node(
    name: &str,
    kind: &str,
    properties: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    let Value::Object(properties) = properties else {
        unreachable!("control properties are an object");
    };
    ResolvedControl {
        name: name.into(),
        control_type: Some(kind.into()),
        base: None,
        unresolved_base: None,
        properties: properties.into_iter().collect::<BTreeMap<_, _>>().into(),
        children,
        factory: None,
    }
}

/// A 50×40 scroll view over a five-column grid of 10×10 labelled cells.
fn screen() -> Arc<ResolvedControl> {
    let grid = node(
        "content",
        "grid",
        json!({
            "size": ["100%", "default"], "grid_dimensions": [5, 60],
            "collection_name": "items", "grid_item_template": "test.cell"
        }),
        Vec::new(),
    );
    let viewport = node("viewport", "panel", json!({ "size": [50, 40] }), vec![grid]);
    let view = node(
        "view",
        "scroll_view",
        json!({ "size": [50, 40], "scroll_content": "content", "scroll_view_port": "viewport" }),
        vec![viewport],
    );
    Arc::new(node(
        "root",
        "panel",
        json!({ "size": [50, 40] }),
        vec![view],
    ))
}

/// Hands out one shared cell template across binds, as a caching library does.
struct Cells(Arc<ResolvedControl>);

impl Cells {
    fn new() -> Self {
        Self::with(json!({ "size": [10, 10] }), json!({}))
    }

    /// A `cell` panel of these properties over a label bound to the item's text, with `label`'s.
    fn with(cell: Value, label: Value) -> Self {
        let mut properties = json!({
            "text": "#text",
            "bindings": [{
                "binding_type": "collection", "binding_collection_name": "items",
                "binding_name": "#text"
            }]
        });
        if let (Value::Object(properties), Value::Object(extra)) = (&mut properties, label) {
            properties.extend(extra);
        }
        let label = node("label", "label", properties, Vec::new());
        Self(Arc::new(node("cell", "panel", cell, vec![label])))
    }
}

impl ControlLibrary for Cells {
    fn resolve(&self, _: &ControlRef) -> Option<ResolvedControl> {
        Some((*self.0).clone())
    }

    fn resolve_shared(
        &self,
        _: &ControlRef,
        _: &str,
        _: &dyn Fn() -> BTreeMap<String, Value>,
    ) -> Option<Arc<ResolvedControl>> {
        Some(Arc::clone(&self.0))
    }
}

struct Fixed;

impl TextMeasure for Fixed {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 2.0, 8.0]
    }
}

impl TextureSource for Fixed {
    /// The fixture draws no images.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

fn items(prefix: &str) -> Arc<DataSource> {
    let mut data = DataSource::new();
    data.set_collection(
        "items",
        (0..CELLS)
            .map(|index| {
                CollectionItem::default()
                    .with("#text", Scalar::Text(format!("{prefix}{index}")))
                    .with("#cell_size", Scalar::Num(20.0))
            })
            .collect(),
    );
    Arc::new(data)
}

/// One screen's live binding, refreshed and laid out as the container screens drive it.
struct Session {
    state: BindState,
    measures: MeasureCache,
    tree: Option<ResolvedControl>,
}

impl Session {
    /// A session virtualizing grids that list `opted`.
    fn new(opted: Option<&str>) -> Self {
        let mut state = BindState::new();
        if let Some(collection) = opted {
            state.virtualize_grid(collection);
        }
        Self {
            state,
            measures: MeasureCache::default(),
            tree: None,
        }
    }

    /// Binds `data`, lays it out at `scroll`, and binds again while the layout shows unbuilt
    /// cells; keeps the bound tree in place, as layout memos address its controls.
    fn draw(
        &mut self,
        root: &Arc<ResolvedControl>,
        library: &Cells,
        data: &Arc<DataSource>,
        scroll: f64,
    ) -> Drawn {
        let mut bound = match self.tree.take() {
            Some(mut bound) => {
                crate::rebind(
                    root,
                    data,
                    library,
                    &mut self.state,
                    &mut bound,
                    &mut self.measures,
                );
                bound
            }
            None => crate::bind_incremental(root, data, library, &mut self.state),
        };
        let view = ViewState {
            scroll: BTreeMap::from([("/root/view".to_owned(), scroll)]),
            ..ViewState::default()
        };
        let env = LayoutEnv {
            text: &Fixed,
            textures: &Fixed,
        };
        let mut passes = 0;
        loop {
            let render =
                crate::render_bound_gated(bound, [50.0, 40.0], &env, &view, &mut self.measures);
            if !self.state.show_grid_cells(&render.report) {
                let drawn = Drawn {
                    built: render
                        .bound
                        .find(&|control| control.name == "view")
                        .map_or(0, |view| count(view, "label")),
                    nodes: render.nodes,
                    hits: render.hits,
                    scrolls: render.report.scrolls,
                };
                self.tree = Some(render.bound);
                return drawn;
            }
            passes += 1;
            assert!(passes <= 2, "the shown cells settle within two binds");
            bound = render.bound;
            crate::rebind(
                root,
                data,
                library,
                &mut self.state,
                &mut bound,
                &mut self.measures,
            );
        }
    }
}

/// What a refresh drew, and how many cells it had built.
struct Drawn {
    nodes: Vec<crate::DrawNode>,
    hits: Arc<[crate::HitRegion]>,
    scrolls: BTreeMap<String, crate::ScrollMetrics>,
    built: usize,
}

fn count(control: &ResolvedControl, name: &str) -> usize {
    usize::from(control.name == name)
        + control
            .children
            .iter()
            .map(|child| count(child, name))
            .sum::<usize>()
}

#[test]
fn a_virtualized_grid_draws_as_one_built_in_full() {
    draws_as_built_in_full(&Cells::new(), "items", true);
}

#[test]
fn placeholders_take_the_cell_size_its_bounds_clamp() {
    draws_as_built_in_full(
        &Cells::with(json!({ "size": [10, 10], "min_size": [20, 20] }), json!({})),
        "items",
        true,
    );
}

/// A selection frame past the cell, as on a recipe book slot, or a label shifted rows up, as
/// a server pack's cell may draw, still shows from cells just outside the view.
#[test]
fn cells_drawing_past_their_rect_virtualize() {
    for label in [
        json!({ "offset": [0, -4], "size": ["100% + 5px", "100% + 5px"] }),
        json!({ "offset": [0, -30] }),
    ] {
        draws_as_built_in_full(
            &Cells::with(json!({ "size": [10, 10] }), label),
            "items",
            true,
        );
    }
}

/// Cells a late build could not reproduce exactly stay fully built unless their grid opts in.
#[test]
fn grids_not_opted_in_build_every_cell() {
    let once = json!({
        "bindings": [{
            "binding_type": "collection", "binding_collection_name": "items",
            "binding_name": "#text", "binding_condition": "once"
        }]
    });
    let bound_size = json!({
        "size": [10, 10],
        "bindings": [{
            "binding_type": "collection", "binding_collection_name": "items",
            "binding_name": "#cell_size", "binding_name_override": "#size_binding_x_absolute"
        }]
    });
    let wide = json!({ "size": ["300%", "100%"] });
    for library in [
        Cells::with(json!({ "size": [10, 10] }), once),
        Cells::with(bound_size, json!({})),
        Cells::with(json!({ "size": [10, 10] }), wide),
    ] {
        draws_as_built_in_full(&library, "recipe_book", false);
    }
}

/// A cell a binding resizes builds every cell even in an opted-in grid, as placeholders would
/// keep its literal size.
#[test]
fn cells_a_binding_resizes_are_all_built() {
    let bound_size = json!({
        "size": [10, 10],
        "bindings": [{
            "binding_type": "collection", "binding_collection_name": "items",
            "binding_name": "#cell_size", "binding_name_override": "#size_binding_x_absolute"
        }]
    });
    draws_as_built_in_full(&Cells::with(bound_size, json!({})), "items", false);
}

/// Draws `library`'s cells built in full and with grids listing `opted` virtualized, across
/// scrolls and data changes, asserting they match and that few cells build when it `virtualizes`.
fn draws_as_built_in_full(library: &Cells, opted: &str, virtualizes: bool) {
    let root = screen();
    let (mut full, mut virtualized) = (Session::new(None), Session::new(Some(opted)));
    for (prefix, scroll) in [
        ("a", 0.0),
        ("a", 250.0),
        ("b", 250.0),
        ("b", 560.0),
        ("c", 0.0),
    ] {
        let data = items(prefix);
        let expected = full.draw(&root, library, &data, scroll);
        let drawn = virtualized.draw(&root, library, &data, scroll);
        // Every cell, and the template the grid keeps to measure.
        assert_eq!(expected.built, CELLS + 1);
        if virtualizes {
            assert!(
                drawn.built < CELLS / 2,
                "{} cells built at {scroll}",
                drawn.built
            );
        } else {
            assert_eq!(drawn.built, CELLS + 1);
        }
        assert!(!drawn.nodes.is_empty());
        assert_eq!(drawn.nodes, expected.nodes, "{prefix} at {scroll}");
        assert_eq!(drawn.hits, expected.hits, "{prefix} at {scroll}");
        assert_eq!(drawn.scrolls, expected.scrolls);
    }
}
