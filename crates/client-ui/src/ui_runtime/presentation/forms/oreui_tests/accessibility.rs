//! A hidden menu still supplies the saved accessibility preference to loading screens.

use std::sync::Arc;

use launcher::menu::{MenuView, settings_options::EXTRA_LARGE_UI};
use ui::{DpiScale, LoadingStage, UiNode};
use ui_engine::{test_support::fixture_font, text_metrics::TextMetrics};

use super::super::super::UiPresentationRuntime;

/// Measures the loading screen's painted title through the real frame configuration.
fn loading_title_height(
    presentation: &mut UiPresentationRuntime,
    view: &MenuView,
    dpi: f32,
) -> f32 {
    presentation.set_menu_view(Some(view.clone()));
    let physical = [1280, 720];
    let dpi_scale = DpiScale::new(dpi).unwrap();
    presentation
        .build(
            &player_state::PlayerState::new(1),
            &crate::ui_runtime::UiRuntime::new(1),
            0,
            physical,
            dpi_scale,
        )
        .unwrap();
    let metrics = TextMetrics::for_viewport(physical, dpi_scale, Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    presentation
        .append_oreui_loading(
            LoadingStage::Connecting,
            ["Loading", "Building terrain"],
            &mut nodes,
            &mut next,
            metrics,
            physical.map(|value| value as f32 / dpi),
        )
        .unwrap();
    title_height(&nodes)
}

/// Finds the actual title's layout height without inspecting the screen's private state.
fn title_height(nodes: &[UiNode]) -> f32 {
    nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. }
                if layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>()
                    == "LOADING" =>
            {
                Some(node.bounds().height())
            }
            _ => None,
        })
        .expect("the loading title is painted")
}

#[test]
fn hidden_menu_accessibility_changes_loading_text_and_restores_ordinary_scale() {
    for dpi in [1.0, 2.0] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_gui_scale_preference(Some(2));
        let mut view = MenuView::new(false, "Fixture".into());
        let index = oreui::test_support::option_index(EXTRA_LARGE_UI);
        let ordinary = loading_title_height(&mut presentation, &view, dpi);
        assert!(Arc::make_mut(&mut view.settings_options).set(index, 1));
        let large = loading_title_height(&mut presentation, &view, dpi);
        assert!((large / ordinary - 1.5).abs() < 0.01);
        assert_eq!(presentation.gui_scale_preference(), Some(2));
        assert!(Arc::make_mut(&mut view.settings_options).set(index, 0));
        assert_eq!(
            loading_title_height(&mut presentation, &view, dpi),
            ordinary
        );
    }
}
