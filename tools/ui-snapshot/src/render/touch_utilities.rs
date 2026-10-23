//! Anonymous gameplay controls rendered through the production touch HUD.
use crate::{
    carriers::{self, Error},
    fixtures::{State, TouchUtilityView},
};
use client_ui::ui_runtime::{
    UiRuntime,
    presentation::{DebugLines, UiPresentationRuntime},
};
use image::ImageEncoder;
use std::{path::Path, sync::Arc};

/// Paints the touch utility trigger and dropdown without a live game session.
pub(super) fn png(state: &State, dir: &Path, view: TouchUtilityView) -> Result<Vec<u8>, Error> {
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
    if matches!(view, TouchUtilityView::Menu) {
        presentation.toggle_touch_utilities();
    }
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(carriers.lang));
    match view {
        TouchUtilityView::PlayerList => {
            runtime.player_roster.set_entries(
                [
                    "Alex", "Blair", "Casey", "Drew", "Emery", "Jordan", "Morgan", "Riley",
                ]
                .into_iter()
                .enumerate()
                .map(|(index, name)| (Arc::from(name), Arc::from(""), [index as u8; 16]))
                .collect(),
            );
            runtime.toggle_touch_player_list();
        }
        TouchUtilityView::DebugOverlay => presentation.set_debug_lines(Some(DebugLines {
            left: vec![
                format!(
                    "{} {} (Bedrock {})",
                    launcher::PRODUCT_NAME,
                    env!("CARGO_PKG_VERSION"),
                    protocol::GAME_VERSION
                ),
                "Preview: sample diagnostic values".into(),
                "120 fps | 8.3 ms avg / 12.5 ms max".into(),
                "GPU: 4.20 ms | terrain 2.10 / actors 0.80".into(),
                "XYZ: 0.500 / 64.00000 / 0.500".into(),
                "Block: 0 64 0".into(),
                "Facing: south (Towards positive Z) (0.0 / 0.0)".into(),
            ],
            right: vec![
                "Targeted Block: 0, 63, 3".into(),
                "minecraft:grass_block".into(),
            ],
        })),
        TouchUtilityView::Closed | TouchUtilityView::Menu => {}
    }
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
