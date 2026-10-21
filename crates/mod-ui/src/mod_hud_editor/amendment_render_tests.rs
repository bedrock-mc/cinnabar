//! The production painter retains catalogs, layouts and text across unchanged editor frames.

use super::*;

struct NoImages;
impl ui_engine::sources::RemoteSource for NoImages {
    /// The original editor fixture has no remote artwork.
    fn state(&self, _: &str) -> ui_engine::sources::RemoteImage {
        ui_engine::sources::RemoteImage::Failed
    }
}

/// Creates a production painter without local game carriers or a graphics device.
fn engine() -> FormEngine {
    use ui_engine::{engine::screen::ScreenRegistry, sources::ImageWork};
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop())),
        Arc::new(NoImages),
        ImageWork::inline(),
    )
}

/// Paints one editor frame through the same path used by the client.
fn paint(
    ui: &mut ModUi,
    engine: &FormEngine,
    font: &assets::RuntimeFontCatalog,
    layouts: &mut ui::TextLayoutCache,
    now: f64,
) {
    let icons = ui.mod_hud_editor_icons(|_, _| None).unwrap();
    ui.paint_mod_hud_editor(
        Some(engine),
        &mut PaintFrame {
            layouts,
            font,
            metrics: ui_engine::text_metrics::TextMetrics::for_viewport(
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
                Some(2),
            ),
            solid_page: 0,
            safe_area: ui::SafeArea::ZERO,
            content: [1280., 720.],
            translate: &|_| None,
            language: [0; 3],
            now,
            nodes: &mut Vec::new(),
            next: &mut 1,
            renderers: &mut [],
        },
        &icons,
    );
    assert!(ui.mod_hud_editor_open());
}

#[test]
fn amendment_hidden_artwork_and_steady_frames_retain_catalog_bindings_and_text() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = ModUi::default();
    ui.set_mod_panel(
        crate::PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    let mut hud = Hud {
        inspector_size: Some([180., 120.]),
        cards: vec![ui::mod_hud::Card {
            id: "disabled".into(),
            title: "Hidden module".into(),
            visible: false,
            ..Default::default()
        }],
        ..Default::default()
    };
    ui.open_mod_hud_editor(&hud).unwrap();
    ui.mod_hud_editor.as_mut().unwrap().selected = Some(0);
    paint(&mut ui, &engine, &font, &mut layouts, 1.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert!(
        !editor.screen.nodes().iter().any(|node| matches!(&node.draw,
        json_ui::Draw::Text { text, .. } if text == "Hidden module"))
    );
    let catalog = Arc::clone(editor.catalog.as_ref().unwrap());
    let data = Arc::clone(&editor.data);
    let passes = editor.screen.passes;
    let shaped = layouts.built_layout_count();
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert!(Arc::ptr_eq(&catalog, editor.catalog.as_ref().unwrap()));
    assert!(Arc::ptr_eq(&data, &editor.data));
    assert_eq!(editor.screen.passes, passes);
    assert_eq!(layouts.built_layout_count(), shaped);
    for action in ["hud.grid", "hud.snap", "hud.visibility:0"] {
        ui.mod_hud_editor.as_mut().unwrap().action(action, &mut []);
        paint(&mut ui, &engine, &font, &mut layouts, 3.);
        assert!(Arc::ptr_eq(
            &catalog,
            ui.mod_hud_editor
                .as_ref()
                .unwrap()
                .catalog
                .as_ref()
                .unwrap()
        ));
    }
    assert!(
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .screen
            .nodes()
            .iter()
            .any(|node| matches!(&node.draw,
        json_ui::Draw::Text { text, .. } if text == "Hidden module"))
    );
    hud.inspector_size = Some([220., 160.]);
    ui.update_mod_hud_editor(&hud).unwrap();
    paint(&mut ui, &engine, &font, &mut layouts, 4.);
    assert!(Arc::ptr_eq(
        &catalog,
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap()
    ));
}
