//! An anonymous server packet through form admission and production painting.

use std::path::Path;

use client_ui::ui_runtime::{SequencedUiEvent, UiRuntime, presentation::UiPresentationRuntime};
use image::ImageEncoder;

use crate::{
    carriers::{self, Error},
    fixtures::State,
};

/// Paints the same locked controls the form controller receives from a server.
pub(super) fn png(state: &State, dir: &Path) -> Result<Vec<u8>, Error> {
    let carriers = carriers::load(dir, false)?;
    let mut presentation =
        UiPresentationRuntime::new(carriers.font).map_err(|error| error.to_string())?;
    presentation
        .enable_json_ui(carriers.ui)
        .map_err(|error| error.to_string())?;
    presentation.set_gui_scale_preference(Some(state.gui_scale));
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(std::sync::Arc::new(carriers.lang));
    let mut player = player_state::PlayerState::new(1);
    let event = protocol::into_world_event(
        protocol::test_support::packets::modal_form_request_packet(7, r#"{
            "type":"custom_form","title":"Locked form controls","content":[
                {"type":"toggle","text":"Locked toggle","default":true,"control_locked":"ingame"},
                {"type":"slider","text":"Locked slider","min":0,"max":10,"default":4,"control_locked":"ingame"},
                {"type":"step_slider","text":"Locked steps","steps":["Slow","Fast"],"default":1,"control_locked":"ingame"},
                {"type":"input","text":"Locked input","default":"Draft value","control_locked":"ingame"},
                {"type":"toggle","text":"Editable toggle","control_locked":"none"}
            ]}"#), 0,
    ).map_err(|error| error.to_string())?;
    let Some(protocol::WorldEvent::Ui(event)) = event else {
        return Err(Error::Invalid("form fixture did not decode".into()));
    };
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event,
            },
        )
        .map_err(|error| format!("{error:?}"))?;
    let input = presentation
        .build(
            &player,
            &runtime,
            0,
            state.size,
            ui::DpiScale::new(1.0).unwrap(),
        )
        .map_err(|error| error.to_string())?;
    let image = client_ui::test_support::rasterize(&input);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(png)
}
