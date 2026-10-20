use super::*;
use crate::{engine::pixel_snap, text_metrics::TextMetrics};
use json_ui::{Draw, LayoutEnv, Rect, Scalar, TextMeasure, TextureMeta, TextureSource, resolve};
use ui::{DpiScale, FONT_DESIGN_PIXEL_TEXELS};

struct DisplayMeasure {
    metrics: TextMetrics,
    px: f32,
}

impl TextMeasure for DisplayMeasure {
    /// Makes DPI-dependent measurement visible without requiring a font carrier.
    fn extent(&self, _: &str) -> [f64; 2] {
        [f64::from(self.metrics.dpi_scale.get()) * 10.0, 8.0]
    }

    /// Uses the production conversion from physical pixels to virtual layout coordinates.
    fn physical_pixel_size(&self) -> f64 {
        1.0 / f64::from(self.px * self.metrics.dpi_scale.get())
    }

    /// Uses the same snapped destination as the canvas for authored path culling.
    fn paint_rect(&self, rect: Rect) -> Rect {
        let [x0, y0, x1, y1] = pixel_snap::snapped(
            [rect.x, rect.y, rect.w, rect.h],
            self.metrics.gui_scale,
            self.px,
        )
        .map(|edge| f64::from(edge / self.px));
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

impl TextureSource for DisplayMeasure {
    /// This authored fixture has no texture dependencies.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Counts expanded binding nodes rather than just the visible emitted primitives.
fn expanded_nodes(tree: &ResolvedControl) -> usize {
    1 + tree.children.iter().map(expanded_nodes).sum::<usize>()
}

#[test]
fn prepared_screen_node_limit_survives_catalog_replacement_and_factory_refresh() {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/limited.json", r#"{"namespace":"limited",
        "root":{"type":"panel","size":[100,100],"factory":{"name":"rows","control_ids":{"row":"limited.row"}}},
        "row":{"type":"panel","size":[10,10],"controls":[{"label":{"type":"label","text":"row","size":[10,8]}}]}}"#);
    let catalog = Arc::new(catalog);
    let context = Context::default();
    let root = Arc::new(resolve(&catalog, "limited.root", &context).control.unwrap());
    let mut screen =
        CachedScreen::from_resolved("limited.root", Arc::clone(&catalog), context.clone(), root)
            .with_node_limit(7);
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), None);
    let measure = DisplayMeasure { metrics, px: 2.0 };
    let env = LayoutEnv {
        text: &measure,
        textures: &measure,
    };
    for current in [Arc::clone(&catalog), Arc::new((*catalog).clone())] {
        for rows in [20, 1, 20] {
            let mut data = DataSource::new();
            data.set_factory(
                "rows",
                (0..rows)
                    .map(|_| json_ui::FactoryItem::new("row", 0.0))
                    .collect(),
            );
            let rendered = screen
                .render(
                    "limited.root",
                    &current,
                    &context,
                    Arc::new(data),
                    ([100.0; 2], 2.0, [0; 3]),
                    &env,
                )
                .unwrap();
            assert_eq!(
                expanded_nodes(&rendered.bound),
                if rows == 1 { 3 } else { 7 }
            );
            assert_eq!(screen.binding.node_limit(), 7);
        }
    }
}

#[test]
fn a_large_factory_prototype_is_bounded_before_the_screen_binds_it() {
    let mut catalog = Catalog::default();
    let controls: Vec<_> = (0..1_000)
        .map(|index| serde_json::json!({ format!("leaf{index}"): {"type":"panel"} }))
        .collect();
    catalog.overlay_text("ui/prototype.json", &serde_json::json!({
        "namespace":"prototype",
        "root":{"type":"panel", "factory":{"name":"rows", "control_ids":{"row":"prototype.row"}}},
        "row":{"type":"panel", "controls":controls}
    }).to_string());
    let catalog = Arc::new(catalog);
    let context = Context::default();
    let tree = Arc::new(
        resolve(&catalog, "prototype.root", &context)
            .control
            .unwrap(),
    );
    let mut screen = CachedScreen::from_resolved(
        "prototype.root",
        Arc::clone(&catalog),
        context.clone(),
        tree,
    )
    .with_node_limit(7);
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), None);
    let measure = DisplayMeasure { metrics, px: 2.0 };
    let env = LayoutEnv {
        text: &measure,
        textures: &measure,
    };
    for current in [Arc::clone(&catalog), Arc::new((*catalog).clone())] {
        let mut data = DataSource::new();
        data.set_factory("rows", vec![json_ui::FactoryItem::new("row", 0.0)]);
        let rendered = screen
            .render(
                "prototype.root",
                &current,
                &context,
                Arc::new(data),
                ([100.0; 2], 2.0, [0; 3]),
                &env,
            )
            .unwrap();
        assert_eq!(expanded_nodes(&rendered.bound), 7);
        // Inspect the cached prototype directly: the bind cap alone cannot prove
        // resolution avoided materializing the entire hostile template first.
        let library = CachedLibrary {
            library: CatalogLibrary {
                catalog: &current,
                context: &context,
            },
            cache: &screen.library,
        };
        let prototype = json_ui::ControlLibrary::resolve(
            &library,
            &json_ui::ControlRef::parse("prototype.row", "prototype"),
        )
        .unwrap();
        assert_eq!(expanded_nodes(&prototype), 7);
    }
}

/// A display change with the same logical layout refreshes culling and measurements, keeping live bindings.
#[test]
fn authored_screen_refreshes_at_changed_dpi_with_unchanged_logical_inputs() {
    let catalog = Arc::new(Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/cache.json"]}"#.as_slice()),
        ("ui/cache.json", br##"{"namespace":"cache","root":{"type":"panel",
            "size":["100%","100%"],"clips_children":true,"controls":[
            {"cap":{"type":"custom","renderer":"$path_renderer","size":[16.1,16.1],
                "anchor_from":"top_left","anchor_to":"top_left","offset":[-18.9,10],
                "view_box":[0,0,16,16],"stroke_width":4,"paths":[[[0,8],[16,8]]]}},
            {"label":{"type":"label","text":"#text","size":["default",8],
                "bindings":[{"binding_name":"#once","binding_name_override":"#text","binding_condition":"once"}]}}
        ]}}"##.as_slice()),
    ]).unwrap());
    let context = Context::desktop().with_var(
        "path_renderer",
        serde_json::json!(json_ui::vector_path::RENDERER),
    );
    let view = ViewState::default();
    let mut data = DataSource::new();
    data.set_global("#once", Scalar::Text("stable".into()));
    let data = Arc::new(data);
    let mut screen = CachedScreen::default();
    let mut logical = None;
    for (index, (physical, dpi, visible)) in [([2560, 1200], 2.0, false), ([1280, 600], 1.0, true)]
        .into_iter()
        .enumerate()
    {
        let metrics = TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), None);
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let root = physical.map(|size| f64::from(size) / f64::from(px * dpi));
        if let Some(logical) = logical {
            assert_eq!((root, px), logical);
        }
        logical = Some((root, px));
        let measure = DisplayMeasure { metrics, px };
        let env = LayoutEnv {
            text: &measure,
            textures: &measure,
        };
        let at = (root, px, [0; 3]);
        let render = screen
            .render_shared_with(
                "cache.root",
                &catalog,
                &context,
                Arc::clone(&data),
                at,
                &env,
                &view,
            )
            .unwrap();
        assert_eq!(render.nodes.iter().any(|node| node.name == "cap"), visible);
        let label = render
            .nodes
            .iter()
            .find(|node| node.name == "label")
            .unwrap();
        assert_eq!(label.dest.w, f64::from(dpi) * 10.0);
        assert!(matches!(&label.draw, Draw::Text { text, .. } if text == "stable"));
        assert_eq!(screen.passes, index + 1);
        screen
            .render_shared_with(
                "cache.root",
                &catalog,
                &context,
                Arc::clone(&data),
                at,
                &env,
                &view,
            )
            .unwrap();
        assert_eq!(
            screen.passes,
            index + 1,
            "unchanged display must keep the layout"
        );
    }
}
