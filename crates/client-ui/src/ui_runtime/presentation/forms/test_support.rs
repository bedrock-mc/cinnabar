//! Shared menu fixtures for renderer and app service integration tests.
use super::pack_harness::engine_presentation;
use crate::{
    menu::{MenuAction, MenuScreen, MenuView},
    ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
};
use std::{path::PathBuf, sync::Arc};
use ui::DpiScale;

/// Draws the retained menu twice and returns the active hit actions.
pub fn draw_menu_actions(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    view: &MenuView,
) -> Vec<MenuAction> {
    for _ in 0..2 {
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(
                player_runtime,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    presentation
        .menu_hit_targets
        .iter()
        .map(|(action, _)| *action)
        .collect()
}

/// Creates a settings view without starting a session.
pub fn settings_view() -> MenuView {
    let mut view = crate::menu::MenuView::new(true, "Steve".into());
    view.screen = MenuScreen::Settings;
    view
}

/// Renders an offline menu fixture with the requested retained-frame state.
pub fn snapshot_menu(player_runtime: &player_state::PlayerState, view: &MenuView, name: &str) {
    snapshot_menu_at(player_runtime, view, name, 0);
}

/// Renders an offline menu fixture with the requested retained-frame state.
pub fn snapshot_menu_at(
    player_runtime: &player_state::PlayerState,
    view: &MenuView,
    name: &str,
    now_millis: u64,
) {
    snapshot_menu_after(player_runtime, view, name, now_millis, 2);
}

/// Renders an offline menu fixture with the requested retained-frame state.
pub fn snapshot_menu_after(
    player_runtime: &player_state::PlayerState,
    view: &MenuView,
    name: &str,
    now_millis: u64,
    warm: usize,
) {
    snapshot_menu_with_catalog(player_runtime, view, name, now_millis, warm, None);
}

/// Renders an offline menu fixture with the requested retained-frame state.
pub fn snapshot_menu_vanilla(
    player_runtime: &player_state::PlayerState,
    view: &MenuView,
    name: &str,
) {
    let Some(carrier) = super::pack_harness::carrier() else {
        return;
    };
    let files = carrier.ui_files();
    let catalog =
        json_ui::Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
    snapshot_menu_with_catalog(player_runtime, view, name, 0, 2, Some(Arc::new(catalog)));
}

/// Renders an offline menu fixture with the requested retained-frame state.
pub fn snapshot_menu_with_catalog(
    player_runtime: &player_state::PlayerState,
    view: &MenuView,
    name: &str,
    now_millis: u64,
    warm: usize,
    catalog: Option<Arc<json_ui::Catalog>>,
) {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping: UI carrier absent");
        return;
    };
    if let Some(catalog) = catalog {
        presentation
            .form_presentation
            .engine
            .as_mut()
            .unwrap()
            .install_pack_catalog(catalog);
    }
    let mut runtime = UiRuntime::new(1);
    let lang = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("../.local/assets/compiled/vanilla-v1.mcbelang");
    if let Some(lang) = std::fs::read(lang)
        .ok()
        .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
    {
        runtime.set_lang_catalog(Arc::new(lang));
    }
    presentation.sync_menu_artwork(crate::ui_runtime::presentation::menu_artwork::view_paths(
        view,
    ));
    presentation.finish_menu_artwork();
    let dpi = DpiScale::new(2.0).unwrap();
    // Warm frames run up to `now_millis` so screen entry animations have played.
    for frame in 0..warm {
        let at = now_millis.saturating_sub(((warm - frame) as u64) * 1_000);
        presentation.set_menu_view(Some(view.clone()));
        presentation
            .build(player_runtime, &runtime, at, [2560, 1440], dpi)
            .unwrap();
        if warm > 2 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    presentation.set_menu_view(Some(view.clone()));
    let input = presentation
        .build(player_runtime, &runtime, now_millis, [2560, 1440], dpi)
        .unwrap();
    crate::ui_runtime::presentation::forms::snapshot::write(&input, name);
}

/// Reads the current menu hit rectangles for app input assertions.
pub fn menu_hit_targets(presentation: &UiPresentationRuntime) -> &[(MenuAction, ui::UiRect)] {
    &presentation.menu_hit_targets
}

/// Reads an authored screen policy through the renderer's active catalog.
pub fn screen_settings(
    presentation: &UiPresentationRuntime,
    reference: &str,
) -> json_ui::ScreenSettings {
    let engine = presentation
        .form_presentation
        .engine
        .as_deref()
        .expect("fixture JSON-UI engine");
    engine.scene_settings(reference, engine.context())
}

/// Builds the actual per-frame text metrics for app rendering integration tests.
pub fn text_metrics(
    physical_size: [u32; 2],
    dpi: ui::DpiScale,
    preference: Option<u8>,
) -> super::super::text_metrics::TextMetrics {
    super::super::text_metrics::TextMetrics::for_viewport(physical_size, dpi, preference)
}

/// Reads the renderer's selected text scale without exposing mutable metrics.
pub fn text_scale(metrics: &super::super::text_metrics::TextMetrics) -> ui::UiScale {
    metrics.scale
}
