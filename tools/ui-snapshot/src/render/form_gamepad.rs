//! Captures original form artwork bound to the active input device.

use std::path::Path;

use client_ui::{
    test_support::{form_input_mode, mini_carrier, pack_harness},
    ui_runtime::presentation::UiPresentationRuntime,
};
use image::ImageEncoder;

use crate::{
    carriers::{self, Error},
    fixtures::State,
};

/// Paints the gamepad image through the same retained form cache used by the client.
pub(super) fn png(state: &State, dir: &Path) -> Result<Vec<u8>, Error> {
    let carriers = carriers::load(dir, false)?;
    let mut player = player_state::PlayerState::new(1);
    let runtime = pack_harness::action_form(&mut player, "Menu", &["Continue"]);
    let mut presentation =
        UiPresentationRuntime::new(carriers.font).map_err(|error| error.to_string())?;
    presentation
        .enable_json_ui(mini_carrier())
        .map_err(|error| error.to_string())?;
    presentation.set_server_ui_pack(&form_input_mode::pack());
    presentation.set_form_input_mode(json_ui::InputMode::Gamepad);
    presentation.set_gui_scale_preference(Some(state.gui_scale));
    let frame = presentation
        .build(
            &player,
            &runtime,
            0,
            state.size,
            ui::DpiScale::new(1.0).unwrap(),
        )
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
