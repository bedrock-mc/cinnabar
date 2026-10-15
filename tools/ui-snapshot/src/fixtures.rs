//! Named, anonymous screen fixtures. Add a state here when a PR needs a new capture.

mod minecraft_import;
mod mods;

use std::sync::Arc;

use launcher::menu::{
    MenuAction, MenuScreen, MenuView, settings_options::SettingId,
    settings_sections::SettingSection,
};

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
/// Keeps the first hotbar rows visible below the keyboard options.
const KEYBOARD_SIZE: [u32; 2] = [SIZE[0], 2000];

pub const STATES: &[State] = &[
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
        name: "chat-command-background-transparent",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: chat_command_background,
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
        name: "settings-video-distance",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_video_option(SettingId::FarTerrain),
    },
    State {
        name: "settings-video-full-detail",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_video_option(SettingId::FullDetailDistance),
    },
    State {
        name: "settings-keyboard",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_keyboard,
    },
    State {
        name: "settings-keyboard-physical-label",
        size: KEYBOARD_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_keyboard_layout(false),
    },
    State {
        name: "settings-keyboard-azerty",
        size: KEYBOARD_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_keyboard_layout(true),
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
    State {
        name: "settings-mods-permissions-collapsed",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: mods::collapsed,
    },
    State {
        name: "settings-mods-permissions-consent",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: mods::consent,
    },
    State {
        name: "settings-mods-permissions-requested",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: mods::requested_only,
    },
    State {
        name: "settings-mods-permissions-all",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: mods::show_all,
    },
    State {
        name: "settings-mods-permissions-narrow",
        size: [360, 900],
        gui_scale: GUI_SCALE,
        fixture: mods::show_all,
    },
    State {
        name: "settings-mods-permissions-undeclared-consent",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: mods::undeclared_permissions,
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

/// Shows the command input and suggestions with chat backgrounds set to zero percent.
fn chat_command_background() -> Fixture {
    use launcher::menu::settings_options::SettingsOptions;

    let mut options = SettingsOptions::default();
    options.set(SettingId::ChatBackgroundOpacity.index(), 0);
    Fixture::Chat {
        draft: "/",
        suggestions: ["/give", "/help", "/say"].map(Arc::from).into(),
        messages: Default::default(),
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
    Fixture::Menu(Box::new(settings(SettingSection::Video.definition().name)))
}

/// Reveals the distance controls through the same focus scrolling as the client.
fn settings_video_option(id: SettingId) -> Fixture {
    let mut view = settings(SettingSection::Video.definition().name);
    view.focused_action = Some(MenuAction::SettingsOption(
        id.index() as u16,
        view.settings_options.value(id),
    ));
    Fixture::Menu(Box::new(view))
}

/// Shows the keyboard options and command macro modifier without a live session.
fn settings_keyboard() -> Fixture {
    Fixture::Menu(Box::new(settings("keyboard_and_mouse_forced_index")))
}

/// Shows the same physical hotbar binding with a fallback or an observed AZERTY label.
fn settings_keyboard_layout(observed: bool) -> Fixture {
    use launcher::menu::settings_options::{
        BindingId, KEY_BINDINGS, control_bindings::decode_control,
    };

    let mut view = settings("keyboard_and_mouse_forced_index");
    let row = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == BindingId::Hotbar1.name())
        .unwrap();
    let settings = Arc::make_mut(&mut view.settings_options);
    assert!(settings.remap(row, decode_control(0x14).unwrap()));
    if observed {
        settings.set_key_label(0x14, "a");
    }
    Fixture::Menu(Box::new(view))
}
