//! Captures retained field focus after a keyboard changes the usable viewport.

use std::path::Path;

use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use image::ImageEncoder;
use launcher::menu::{MenuAction, MenuField, MenuScreen, MenuView};

use crate::{
    carriers::{self, Error},
    fixtures::State,
};

/// Paints the same focused field before and after the simulated keyboard overlap.
pub(super) fn png(state: &State, dir: &Path) -> Result<Vec<u8>, Error> {
    let carriers = carriers::load(dir, true)?;
    let mut presentation =
        UiPresentationRuntime::new(carriers.font).map_err(|error| error.to_string())?;
    if let Some(images) = oreui::images::load_optional_oreui_images(dir) {
        presentation.enable_oreui_originals(images)?;
    }
    presentation.set_gui_scale_preference(Some(state.gui_scale));
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::AddServer;
    view.field = Some(MenuField::Port);
    view.focused_action = Some(MenuAction::AddPort);
    view.settings_options = std::sync::Arc::new({
        let mut options = (*view.settings_options).clone();
        options.set(
            launcher::menu::settings_options::SettingId::ScreenAnimations.index(),
            0,
        );
        options
    });
    view.navigation_focus_visible = true;
    view.name = "Test server".into();
    view.address = "example.test".into();
    presentation.set_menu_view(Some(view));
    let player = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    let dpi = ui::DpiScale::new(1.0).unwrap();
    presentation
        .build(&player, &runtime, 0, state.size, dpi)
        .map_err(|error| error.to_string())?;
    // This fixture supplies measured overlap directly; Android supplies it from window insets.
    let overlap = state.size[1] as f32 * 0.6;
    presentation.set_safe_area(ui::SafeArea::new(0.0, 0.0, 0.0, overlap).unwrap());
    let frame = presentation
        .build(&player, &runtime, 1_000, state.size, dpi)
        .map_err(|error| error.to_string())?;
    let pixels = client_ui::test_support::rasterize(&frame);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &pixels,
            state.size[0],
            state.size[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(png)
}
