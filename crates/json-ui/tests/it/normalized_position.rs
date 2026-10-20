//! Normalized placement follows painted dimensions and remains a binding-driven layout.

use json_ui::{
    BindState, Catalog, Context, DataSource, LayoutEnv, Scalar, ViewState, bind_screen, hit_test,
    render_bound, resolve, transform::Affine,
};
use serde_json::json;

struct MonoText;
impl json_ui::TextMeasure for MonoText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}

struct Plain;
impl json_ui::TextureSource for Plain {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        Some(json_ui::TextureMeta::plain([16.0, 16.0]))
    }
}

/// Resolves and binds the same card at a caller-selected viewport and placement.
fn render(
    viewport: [f64; 2],
    position: [f64; 2],
    scale: f64,
    enabled: bool,
) -> ([f64; 4], Vec<json_ui::HitRegion>) {
    let document = json!({"namespace":"position_test","root":{"type":"screen",
        "size":["100%","100%"],"controls":[{"card":{"type":"button",
        "size":[40,20],"offset":[10,10],"anchor_from":"top_left","anchor_to":"top_left",
        "cinnabar_position":[0,0],"cinnabar_transform":{"scale":1,"origin":[0,0]},
        "bindings":[{"binding_name":"#at","binding_name_override":"#cinnabar_position"},
            {"binding_name":"#scale","binding_name_override":"#cinnabar_scale"}],
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":"move",
            "mapping_type":"pressed"}],"controls":[{"fill":{"type":"image",
            "size":["100%","100%"],"color":[1,1,1]}}]}}]}})
    .to_string();
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        (
            "ui/_ui_defs.json",
            &br#"{"ui_defs":["ui/position_test.json"]}"#[..],
        ),
        ("ui/position_test.json", document.as_bytes()),
    ])
    .unwrap();
    let catalog = if enabled {
        catalog.with_paint_transforms()
    } else {
        catalog
    };
    let context = Context::default();
    let root = resolve(&catalog, "position_test.root", &context)
        .control
        .unwrap();
    let mut data = DataSource::new();
    data.set_global("#at", Scalar::Json(json!(position)));
    data.set_global("#scale", Scalar::Num(scale));
    let bound = bind_screen(&root, &catalog, &context, &data, &mut BindState::new());
    let frame = render_bound(
        bound,
        viewport,
        &LayoutEnv {
            text: &MonoText,
            textures: &Plain,
        },
        &ViewState::default(),
    );
    let fill = frame.nodes.iter().find(|node| node.name == "fill").unwrap();
    let transform = fill
        .anim
        .as_ref()
        .and_then(|anim| anim.transform)
        .unwrap_or(Affine::IDENTITY);
    let bounds = transform.bounds(json_ui::Rect::new(
        fill.dest.x,
        fill.dest.y,
        fill.dest.w,
        fill.dest.h,
    ));
    (
        [bounds.x, bounds.y, bounds.w, bounds.h],
        frame.hits.to_vec(),
    )
}

#[test]
fn positions_track_available_travel_after_scale_and_viewport_resize() {
    let (bounds, hits) = render([200., 100.], [1., 1.], 2., true);
    assert_eq!(bounds, [120., 60., 80., 40.]);
    assert!(hit_test(&hits, [190., 90.]).is_some());
    assert!(hit_test(&hits, [110., 50.]).is_none());
    let (bounds, _) = render([200., 100.], [0.125, 0.45], 2., true);
    assert_eq!(bounds, [15., 27., 80., 40.]);
    let (bounds, _) = render([400., 200.], [0.125, 0.45], 2., true);
    assert_eq!(bounds, [40., 72., 80., 40.]);
}

#[test]
fn untrusted_catalogs_ignore_native_position_and_transform_bindings() {
    assert_eq!(
        render([200., 100.], [1., 1.], 2., false).0,
        [10., 10., 40., 20.]
    );
}
