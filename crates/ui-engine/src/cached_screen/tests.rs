use super::*;
use crate::{engine::pixel_snap, text_metrics::TextMetrics};
use json_ui::{Draw, LayoutEnv, Rect, Scalar, TextMeasure, TextureMeta, TextureSource};
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
