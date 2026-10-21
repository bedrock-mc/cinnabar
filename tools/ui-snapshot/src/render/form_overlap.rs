//! Renders overlapping requests through production form admission and presentation.

use std::path::Path;

use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use image::ImageEncoder;
use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm};

use crate::{
    carriers::{self, Error},
    fixtures::State,
};

/// Captures the menu left visible after two requests reuse one protocol ID.
pub(super) fn png(state: &State, dir: &Path) -> Result<Vec<u8>, Error> {
    let mut runtime = UiRuntime::new(1);
    for (sequence, title) in [(1, "Original menu"), (2, "Overlapping menu")] {
        runtime.server_forms_mut().admit(
            FormRequestEvent {
                form_id: 7,
                kind: FormKind::Menu,
                title: Some(title.into()),
                json: "{}".into(),
                model: ServerFormModel::TextMenu(TextMenuForm {
                    title: title.into(),
                    content: "The server sent two requests with the same ID.".into(),
                    buttons: vec!["Continue".into(), "Back".into()].into(),
                    button_images: [].into(),
                    omitted_images: 0,
                }),
            },
            sequence,
            1,
            false,
        );
    }
    png_for(state, dir, &runtime)
}

/// Paints an admitted fixture with the same carrier, scale and production presentation.
pub(super) fn png_for(state: &State, dir: &Path, runtime: &UiRuntime) -> Result<Vec<u8>, Error> {
    let carriers = carriers::load(dir, false)?;
    let mut presentation =
        UiPresentationRuntime::new(carriers.font).map_err(|error| error.to_string())?;
    presentation
        .enable_json_ui(carriers.ui)
        .map_err(|error| error.to_string())?;
    presentation.set_gui_scale_preference(Some(state.gui_scale));
    let frame = presentation
        .build(
            &player_state::PlayerState::new(1),
            runtime,
            1_000,
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
