//! Named, anonymous screen fixtures. Add a state here when a PR needs a new capture.

mod minecraft_import;
mod mods;
mod out_of_game;

use std::sync::Arc;

use launcher::menu::{MenuScreen, MenuView};

/// A repeatable viewport and the runtime observations its screen paints.
pub struct State {
    pub name: &'static str,
    pub size: [u32; 2],
    pub gui_scale: u8,
    pub fixture: fn() -> Fixture,
}

/// Inputs borrowed by the same screen painters the client uses.
pub enum Fixture {
    Menu(Box<MenuView>),
    Loading(ui::LoadingStage),
    Chat {
        draft: &'static str,
        suggestions: Vec<Arc<str>>,
        messages: std::collections::VecDeque<ui::ChatMessage>,
        options: Arc<launcher::menu::settings_options::SettingsOptions>,
    },
}

const SIZE: [u32; 2] = [1280, 720];
const GUI_SCALE: u8 = 2;
const SMALL_SIZE: [u32; 2] = [360, 240];
/// Fits an expanded card above two collapsed ones.
const TALL_SIZE: [u32; 2] = [1280, 900];

pub const STATES: &[State] = &[
    State {
        name: "pause",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::pause,
    },
    State {
        name: "death",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::death,
    },
    State {
        name: "loading-connecting",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::Loading(ui::LoadingStage::Connecting),
    },
    State {
        name: "loading-terrain",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::Loading(ui::LoadingStage::BuildingTerrain),
    },
    State {
        name: "disconnect",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::disconnect,
    },
    State {
        name: "disconnect-ban",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::disconnect_ban,
    },
    State {
        name: "disconnect-no-retry",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::disconnect_no_retry,
    },
    State {
        name: "server-trust",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::server_trust,
    },
    State {
        name: "join-request",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::join_request,
    },
    State {
        name: "remove-server",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::remove_server,
    },
    State {
        name: "storage-delete",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::storage_delete,
    },
    State {
        name: "storage-error",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::storage_error,
    },
    State {
        name: "settings-help",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::settings_help,
    },
    State {
        name: "font-license",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::font_license,
    },
    State {
        name: "reset-bindings",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::reset_bindings,
    },
    State {
        name: "reset-settings",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::reset_settings,
    },
    State {
        name: "chat-background-transparent",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_background(0),
    },
    State {
        name: "chat-background-half",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_background(50),
    },
    State {
        name: "chat-background-opaque",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_background(100),
    },
    State {
        name: "import-confirmation",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: minecraft_import::confirmation,
    },
    State {
        name: "import-confirmation-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: minecraft_import::confirmation,
    },
    State {
        name: "import-preview-many",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: minecraft_import::preview,
    },
    State {
        name: "import-preview-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: minecraft_import::preview,
    },
    State {
        name: "chat-mentions",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: chat_mentions,
    },
    State {
        name: "chat-command-targets",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: chat_command_targets,
    },
    State {
        name: "settings-video",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_video,
    },
    State {
        name: "settings-keyboard",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_keyboard,
    },
    State {
        name: "settings-mods",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: mods::declared,
    },
    State {
        name: "settings-mods-undeclared",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: mods::undeclared,
    },
];

/// Exercises the native chat collection with an unfinished mention.
fn chat_mentions() -> Fixture {
    Fixture::Chat {
        draft: "@",
        suggestions: ["@here", "@Alex", "@Steve"].map(Arc::from).into(),
        messages: Default::default(),
        options: Default::default(),
    }
}

/// Exercises the native command completion collection with target candidates.
fn chat_command_targets() -> Fixture {
    Fixture::Chat {
        draft: "/give @",
        suggestions: ["@a", "@e", "@p", "@r", "@s"].map(Arc::from).into(),
        messages: Default::default(),
        options: Default::default(),
    }
}

/// Shows readable history and a draft under a selected background-opacity setting.
fn chat_background(percent: i32) -> Fixture {
    use launcher::menu::settings_options::{SettingId, SettingsOptions};
    let mut options = SettingsOptions::default();
    options.set(SettingId::ChatBackgroundOpacity.index(), percent);
    let messages = [
        format!("Chat background opacity: {percent}%"),
        "The chat text stays visible.".into(),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, message)| ui::ChatMessage {
        fifo_sequence: index as u64 + 1,
        received_millis: 0,
        kind: ui::ChatMessageKind::System,
        source: None,
        message: message.into(),
        parameters: Arc::from([]),
    })
    .collect();
    Fixture::Chat {
        draft: "Typing a message",
        suggestions: Vec::new(),
        messages,
        options: Arc::new(options),
    }
}

/// Opens a settings category by its registered name rather than a copied index.
pub fn settings(section: &str) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings_section = launcher::menu::settings_sections::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == section).then_some(*index))
        .expect("registered settings category");
    view
}

/// Shows an ordinary OreUI route without account, world or network services.
fn settings_video() -> Fixture {
    Fixture::Menu(Box::new(settings("video_forced_index")))
}

/// Shows the keyboard options and command macro modifier without a live session.
fn settings_keyboard() -> Fixture {
    Fixture::Menu(Box::new(settings("keyboard_and_mouse_forced_index")))
}
