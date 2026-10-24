//! The anonymous survival HUD with the Bedrock Hotbar Position setting off or on.
use crate::{
    carriers::{self, Error},
    fixtures::State,
};
use client_ui::ui_runtime::{
    SequencedLocalAttributes, SequencedUiEvent, UiRuntime, presentation::UiPresentationRuntime,
};
use image::ImageEncoder;
use launcher::menu::settings_options::{BEDROCK_HOTBAR_OPTION, SETTINGS_OPTIONS, SettingsOptions};
use protocol::{ActorAttribute, PlayerGameMode, TextCategory, TextEvent, TextKind, UiEvent};
use std::{path::Path, sync::Arc};

/// Paints full health and hunger, an empty hotbar and one chat line, with the hotbar flush
/// (`raised` false) or at vanilla Bedrock's height.
pub(super) fn png(state: &State, dir: &Path, raised: bool) -> Result<Vec<u8>, Error> {
    let pack = dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(dir)
        .join(launcher::install_layout::vanilla_pack_relative());
    if !pack.is_dir() {
        return Err(Error::Missing(format!(
            "HUD resource pack: {}; make assets",
            pack.display()
        )));
    }
    let carriers = carriers::load(dir, false)?;
    let mut presentation = UiPresentationRuntime::new(carriers.font).map_err(|e| e.to_string())?;
    presentation.enable_json_ui(carriers.ui)?;
    presentation.set_vanilla_texture_root(pack);
    presentation.set_gui_scale_preference(Some(state.gui_scale));
    let mut settings = SettingsOptions::default();
    let option = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == BEDROCK_HOTBAR_OPTION.name)
        .expect("registered hotbar setting");
    settings.set(option, i32::from(raised));
    presentation.set_chat_settings_snapshot((Arc::new(settings), None));
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(carriers.lang));
    let attribute = |name: &str| ActorAttribute {
        name: Arc::from(name),
        min: 0.0,
        max: 20.0,
        current: 20.0,
        default: None,
        modifiers: Arc::from([]),
    };
    runtime
        .apply_local_attributes(
            &mut player,
            SequencedLocalAttributes {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: 1,
                attributes: vec![
                    attribute("minecraft:health"),
                    attribute("minecraft:player.hunger"),
                ]
                .into(),
            },
        )
        .map_err(|e| Error::Invalid(format!("{e:?}")))?;
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Text(TextEvent {
                    category: TextCategory::MessageOnly,
                    kind: TextKind::Chat,
                    needs_translation: false,
                    source: None,
                    message: Arc::from("Chat rises with the hotbar"),
                    parameters: Arc::from([]),
                    xuid: Arc::from(""),
                    platform_chat_id: Arc::from(""),
                    filtered_message: None,
                }),
            },
        )
        .map_err(|e| Error::Invalid(format!("{e:?}")))?;
    let frame = presentation
        .build(
            &player,
            &runtime,
            0,
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
