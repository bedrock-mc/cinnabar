//! Anonymous gameplay controls rendered through the production touch HUD.
use crate::{
    carriers::{self, Error},
    fixtures::State,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use image::ImageEncoder;
use std::{path::Path, sync::Arc};

/// Paints the touch utility trigger and dropdown without a live game session.
pub(super) fn png(state: &State, dir: &Path, open: bool) -> Result<Vec<u8>, Error> {
    let pack = dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(dir)
        .join(launcher::install_layout::vanilla_pack_relative());
    if !pack.is_dir() {
        return Err(Error::Missing(format!(
            "touch utility resource pack: {}; make assets",
            pack.display()
        )));
    }
    let carriers = carriers::load(dir, false)?;
    let mut presentation = UiPresentationRuntime::new(carriers.font).map_err(|e| e.to_string())?;
    presentation.enable_json_ui(carriers.ui)?;
    presentation.set_vanilla_texture_root(pack);
    presentation.enable_touch_controls(true);
    presentation.set_gui_scale_preference(Some(state.gui_scale));
    if open {
        presentation.toggle_touch_utilities();
    }
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(carriers.lang));
    let frame = presentation
        .build(
            &player_state::PlayerState::new(1),
            &runtime,
            1000,
            state.size,
            ui::DpiScale::new(1.).unwrap(),
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let pixels = client_ui::test_support::rasterize(&frame);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &pixels,
            state.size[0],
            state.size[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(png)
}
