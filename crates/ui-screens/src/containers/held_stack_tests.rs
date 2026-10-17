use super::held_stack::held_stack;
use std::sync::Arc;
use ui_engine::{
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    text_metrics::TextMetrics,
};

struct Offline;
impl RemoteSource for Offline {
    /// Cursor decoration tests never request network artwork.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// Paints a cursor overlay through the production custom renderer without installed carriers.
fn paint(durability: Option<f32>, icon: bool) -> Vec<ui::UiNode> {
    let mut files = [
        ("ui/_global_variables.json", "{}"),
        ("ui/_ui_defs.json", r#"{"ui_defs":["ui/test.json"]}"#),
        (
            "ui/test.json",
            r#"{"namespace":"test","screen":{"type":"screen"}}"#,
        ),
    ]
    .map(|(path, text)| assets::UiFile {
        path: path.into(),
        bytes: text.as_bytes().into(),
    });
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let page = assets::UiAtlasPage {
        width: 1,
        height: 1,
        rgba8: vec![255; 4].into(),
    };
    let bytes = assets::encode_ui_catalog([1; 32], &[page], &[], &[], &files).unwrap();
    let carrier = Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap());
    let catalog = json_ui::Catalog::from_files(
        files
            .iter()
            .map(|file| (file.path.as_ref(), file.bytes.as_ref())),
    )
    .unwrap();
    let registry = Box::leak(Box::new(ScreenRegistry::new(json_ui::Context::desktop())));
    registry.screen("test.screen");
    let engine = FormEngine::new(
        carrier,
        catalog,
        1,
        registry,
        Arc::new(Offline),
        ImageWork::inline(),
    );
    let font = ui_engine::test_support::fixture_font();
    let mut nodes = Vec::new();
    let mut layouts = ui::TextLayoutCache::new(128, 1024 * 1024);
    let mut next = 1;
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics: TextMetrics::for_viewport([320, 240], ui::DpiScale::new(1.0).unwrap(), Some(1)),
        solid_page: 0,
        safe_area: ui::SafeArea::ZERO,
        content: [320.0, 240.0],
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    let icon = icon.then_some(ui::IconRef {
        page: 1,
        uv: [0, 0, 1, 1],
        glint: false,
    });
    let overlay = held_stack(
        Some(&protocol::NetworkItemStack {
            network_id: 1,
            count: 1,
            ..protocol::NetworkItemStack::empty()
        }),
        icon,
        durability,
        Some([100.0, 100.0]),
        &mut Vec::new(),
    );
    engine
        .render_screen(
            "test.screen",
            &json_ui::DataSource::new(),
            &json_ui::Context::desktop(),
            &json_ui::ViewState::default(),
            ScreenArt {
                overlay: &overlay,
                ..Default::default()
            },
            &mut frame,
        )
        .unwrap()
        .unwrap();
    nodes
}

#[test]
fn cursor_durability_paints_its_fill_even_without_artwork() {
    for icon in [true, false] {
        let worn = paint(Some(0.5), icon);
        assert!(
            worn.iter().any(|node| matches!(
                node.visual(),
                ui::UiVisual::Solid {
                    color: [255, 255, 0, 255],
                    ..
                }
            )),
            "a half-worn cursor item must paint a yellow fill"
        );
        let undamaged = paint(None, icon);
        assert!(
            !undamaged.iter().any(|node| matches!(
                node.visual(),
                ui::UiVisual::Solid {
                    color: [255, 255, 0, 255],
                    ..
                }
            )),
            "items without a durability fraction must not acquire a bar"
        );
    }
}
