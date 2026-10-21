//! Named, anonymous screen fixtures. Add a state here when a PR needs a new capture.

mod chat_settings;
mod minecraft_import;
mod mods;
mod out_of_game;

use std::sync::Arc;

use launcher::menu::{
    MenuAction, MenuScreen, MenuView, settings_options::SettingId,
    settings_sections::SettingSection,
};
use ui_screens::player_list::RosterRow;

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
    /// The held tab roster's rows.
    TabList(Vec<RosterRow>),
    /// Two original custom-form inputs after selection moves from the first to the second.
    EditSelection,
    /// Survival recipe-book inputs admitted by the inventory session and wire decoder.
    #[cfg(feature = "test-support")]
    CraftOverlap {
        selected: bool,
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
    #[cfg(feature = "test-support")]
    State {
        name: "craft-overlap-ready",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::CraftOverlap { selected: false },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "craft-overlap-selected",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::CraftOverlap { selected: true },
    },
    State {
        name: "edit-selection-transfer",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::EditSelection,
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
        name: "connect-failed",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::connect_failed,
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
        name: "storage-error-long",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::storage_error_long,
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
        name: "play-worlds",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::play_worlds,
    },
    State {
        name: "play-worlds-importing",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::play_worlds_importing,
    },
    State {
        name: "world-edit-after-delete-back",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::world_edit_after_delete_back,
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
        name: "chat-history-width",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: chat_history_width,
    },
    State {
        name: "chat-history-width-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: chat_history_width,
    },
    State {
        name: "chat-command-background-transparent",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: chat_command_background,
    },
    State {
        name: "chat-command-completion",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_completion("/give @", &["@a", "@e", "@p"]),
    },
    State {
        name: "chat-command-completion-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: || {
            chat_completion(
                "/tell ExamplePlayer AnotherPlayerWithALongName @",
                &["@Alex", "@ExamplePlayerWithALongName", "@Bo"],
            )
        },
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
        name: "tab-list-badges",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || tab_list_badges(true),
    },
    State {
        name: "tab-list-expired-badges",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || tab_list_badges(false),
    },
    State {
        name: "settings-chat",
        size: [1280, 1360],
        gui_scale: GUI_SCALE,
        fixture: || chat_settings::panel(false, None),
    },
    State {
        name: "settings-chat-top",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_settings::panel(false, None),
    },
    State {
        name: "settings-chat-bottom",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_settings::bottom(false),
    },
    State {
        name: "settings-chat-changed",
        size: [1280, 1360],
        gui_scale: GUI_SCALE,
        fixture: || chat_settings::panel(true, None),
    },
    State {
        name: "settings-chat-minimum",
        size: [1280, 1360],
        gui_scale: GUI_SCALE,
        fixture: chat_settings::minimum,
    },
    State {
        name: "settings-chat-narrow",
        size: [800, 720],
        gui_scale: GUI_SCALE,
        fixture: || chat_settings::panel(false, None),
    },
    State {
        name: "settings-chat-reset",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: chat_settings::reset,
    },
    State {
        name: "settings-chat-color",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_settings::panel(false, Some(SettingId::ChatColor)),
    },
    State {
        name: "settings-chat-mentions",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_settings::panel(false, Some(SettingId::MentionsColor)),
    },
    State {
        name: "settings-video",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_video,
    },
    State {
        name: "settings-storage",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_storage,
    },
    State {
        name: "settings-keyboard",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_keyboard,
    },
    State {
        name: "settings-general",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_general_toggle(SettingId::EcomodeToggle),
    },
    State {
        name: "settings-general-advanced",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_general_toggle(SettingId::RecordNetworkSession),
    },
    State {
        name: "settings-general-advanced-on",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_general_diagnostics_on,
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
    chat_completion("/", &["/give", "/help", "/say"])
}

/// Shows equal-width history backgrounds and a message that wraps inside them.
fn chat_history_width() -> Fixture {
    let mut fixture = chat_background(50);
    if let Fixture::Chat { messages, .. } = &mut fixture {
        messages.push_back(ui::ChatMessage {
            fifo_sequence: messages.len() as u64 + 1,
            received_millis: 0,
            kind: ui::ChatMessageKind::System,
            source: None,
            message: "A longer chat message wraps inside the history box while every visible line keeps the same background width.".into(),
            parameters: Arc::from([]),
        });
    }
    fixture
}

/// Shows completions after an anonymous draft with transparent message backgrounds.
fn chat_completion(draft: &'static str, suggestions: &[&str]) -> Fixture {
    use launcher::menu::settings_options::SettingsOptions;

    let mut options = SettingsOptions::default();
    options.set(SettingId::ChatBackgroundOpacity.index(), 0);
    Fixture::Chat {
        draft,
        suggestions: suggestions.iter().map(|text| Arc::from(*text)).collect(),
        messages: Default::default(),
        options: Arc::new(options),
    }
}

/// Badges two Cinnabar players, one of them sharing a name with an unbadged player.
fn tab_list_badges(visible: bool) -> Fixture {
    Fixture::TabList(
        [
            ("Alex", true),
            ("Alex", false),
            ("Riley", false),
            ("Steve", visible),
            ("Zoe", false),
        ]
        .map(|(name, badged)| RosterRow {
            name: Arc::from(name),
            badged,
        })
        .into(),
    )
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

/// Shows Storage as a desktop host fills it, with the folder buttons.
fn settings_storage() -> Fixture {
    let mut view = settings(SettingSection::StorageManagement.definition().name);
    Arc::make_mut(&mut view.storage).opens_folders = true;
    Fixture::Menu(Box::new(view))
}

/// Shows the keyboard options and command macro modifier without a live session.
fn settings_keyboard() -> Fixture {
    Fixture::Menu(Box::new(settings("keyboard_and_mouse_forced_index")))
}

/// Scrolls the General section to toggle `id` through the client's focus scrolling; a toggle's
/// action carries the value it switches to.
fn settings_general_toggle(id: SettingId) -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    view.focused_action = Some(MenuAction::SettingsOption(
        id.index() as u16,
        1 - view.settings_options.value(id),
    ));
    Fixture::Menu(Box::new(view))
}

/// The Advanced rows with Verbose logging and Record network session turned on.
fn settings_general_diagnostics_on() -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    let mut options = (*view.settings_options).clone();
    for id in [SettingId::VerboseLogging, SettingId::RecordNetworkSession] {
        options.set(id.index(), 1);
    }
    view.settings_options = Arc::new(options);
    view.focused_action = Some(MenuAction::SettingsOption(
        SettingId::RecordNetworkSession.index() as u16,
        0,
    ));
    Fixture::Menu(Box::new(view))
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
