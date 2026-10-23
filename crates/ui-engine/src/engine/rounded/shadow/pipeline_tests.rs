//! Full canvas and retained draw checks without installed carriers.

use super::*;
use crate::{
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};
use json_ui::{Catalog, Context};
use serde_json::json;
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiTree};

struct NoImages;
impl RemoteSource for NoImages {
    /// This fixture never requests network artwork.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// Paints an authored rounded rectangle inside an explicit ancestor clip.
fn painted(properties: Value) -> Vec<UiNode> {
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let registry = REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop()));
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    let engine = FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        registry,
        Arc::new(NoImages),
        ImageWork::inline(),
    );
    let mut rectangle = json!({"type":"custom","renderer":json_ui::rounded::RENDERER,
        "size":[40,20],"offset":[20,20],"anchor_from":"top_left","anchor_to":"top_left",
        "color":[0.8,0.6,0.4,1],"radius":6});
    rectangle
        .as_object_mut()
        .unwrap()
        .extend(properties.as_object().unwrap().clone());
    let doc = json!({"namespace":"shadow","root":{"type":"panel","size":[100,80],
        "clips_children":true,"controls":[{"rect":rectangle}]}})
    .to_string();
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/shadow.json"]}"#.as_slice(),
        ),
        ("ui/shadow.json", doc.as_bytes()),
    ])
    .unwrap();
    let root = json_ui::resolve(&catalog, "shadow.root", &Context::desktop())
        .control
        .unwrap();
    let metrics = TextMetrics::for_viewport([400, 320], DpiScale::new(1.0).unwrap(), Some(1));
    let font = fixture_font();
    let mut nodes = Vec::new();
    let mut next = 1;
    engine
        .draw(
            ScreenArt::default(),
            &mut PaintFrame {
                layouts: &mut TextLayoutCache::new(4, 1 << 20),
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [400.0, 320.0],
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            },
            |env, size| Some(json_ui::render_bound(root, size, env, &Default::default())),
        )
        .unwrap();
    nodes
}

#[test]
fn rounded_shadows_disabled_leave_the_draw_output_identical() {
    let plain = painted(json!({}));
    for properties in [
        json!({"shadow_blur":8,"shadow_offset":[3,5]}),
        json!({"shadow_color":[0,0,0,0],"shadow_blur":8}),
    ] {
        let disabled = painted(properties);
        assert_eq!(plain, disabled);
        let plain = UiTree::new(plain.clone())
            .unwrap()
            .build_draw_list()
            .unwrap();
        let disabled = UiTree::new(disabled).unwrap().build_draw_list().unwrap();
        assert_eq!(plain.vertices, disabled.vertices);
        assert_eq!(plain.indices, disabled.indices);
        assert_eq!(plain.batches, disabled.batches);
    }
}

#[test]
fn rounded_shadows_canvas_places_the_shadow_behind_the_fill_with_ancestor_scissors() {
    let nodes = painted(json!({"shadow_color":[0,0,0,0.5],"shadow_offset":[4,6],"shadow_blur":8}));
    let shadow = nodes
        .iter()
        .position(|node| matches!(node.visual(), UiVisual::TintedMesh { .. }))
        .unwrap();
    let fill = nodes
        .iter()
        .position(|node| matches!(node.visual(), UiVisual::Mesh(_)))
        .unwrap();
    assert!(shadow < fill);
    assert_eq!(nodes[shadow].parent(), nodes[fill].parent());
    assert!(nodes[shadow].bounds().min().x() < nodes[fill].bounds().min().x());
    assert!(nodes[shadow].bounds().max().y() > nodes[fill].bounds().max().y());
    let draw = UiTree::new(nodes).unwrap().build_draw_list().unwrap();
    assert!(
        draw.vertices
            .iter()
            .any(|vertex| vertex.style_flags == u32::from(render_api::ui_shadow::UI_STYLE_SHADOW))
    );
    assert!(
        draw.batches
            .iter()
            .all(|batch| batch.clip == draw.batches[0].clip)
    );
}
