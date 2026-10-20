//! Named, anonymous screen fixtures. Add a state here when a PR needs a new capture.

#[cfg(feature = "test-support")]
mod cosmetics;
mod minecraft_import;
mod mods;
mod out_of_game;
mod shader_pack;
mod updates;

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
    /// A server form whose editable controls are locked during a session.
    #[cfg(feature = "test-support")]
    ControlLocked,
    /// An equipped original cosmetic rendered by the full production particle-preview path.
    #[cfg(feature = "test-support")]
    CosmeticParticles {
        view: client_ui::ui_runtime::presentation::player_preview::PreviewView,
        emitting: bool,
    },
    Menu(Box<MenuView>),
    Loading(ui::LoadingStage),
    Chat {
        draft: &'static str,
        suggestions: Vec<Arc<str>>,
        messages: std::collections::VecDeque<ui::ChatMessage>,
        options: Arc<launcher::menu::settings_options::SettingsOptions>,
        server_hover: Option<bool>,
    },
    /// The held tab roster's rows.
    TabList(Vec<RosterRow>),
    /// Two original custom-form inputs after selection moves from the first to the second.
    EditSelection,
    /// An externally supplied bounded native mod panel.
    ModPanel(Box<ui::mod_panel::Panel>),
    /// Two same-ID requests arriving while the first menu is still open.
    #[cfg(feature = "test-support")]
    FormOverlap,
    /// A damaged stack picked up from the survival inventory.
    #[cfg(feature = "test-support")]
    CursorDurability,
    /// Survival recipe-book inputs admitted by the inventory session and wire decoder.
    #[cfg(feature = "test-support")]
    CraftOverlap {
        selected: bool,
    },
    /// A client toast as the toast screen shows it once slid in.
    Toast {
        title: String,
        message: String,
    },
}

const SIZE: [u32; 2] = [1280, 720];
const GUI_SCALE: u8 = 2;
const SMALL_SIZE: [u32; 2] = [360, 240];
/// Fits an expanded card above two collapsed ones.
const TALL_SIZE: [u32; 2] = [1280, 900];
/// Keeps the first hotbar rows visible below the keyboard options.
const KEYBOARD_SIZE: [u32; 2] = [SIZE[0], 2000];
/// Reaches the Video pane's Renderer row below the general options.
const SHADER_PACK_ROW_SIZE: [u32; 2] = [SIZE[0], 1600];

pub const STATES: &[State] = &[
    #[cfg(feature = "test-support")]
    State {
        name: "form-overlap",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::FormOverlap,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "form-control-locked",
        size: [1280, 960],
        gui_scale: GUI_SCALE,
        fixture: || Fixture::ControlLocked,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "paper-doll-particles-before",
        size: [576, 672],
        gui_scale: 1,
        fixture: || Fixture::CosmeticParticles {
            view: client_ui::ui_runtime::presentation::player_preview::PreviewView::Doll {
                yaw: 30.0,
                tilt: -10.0,
            },
            emitting: false,
        },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "paper-doll-particles",
        size: [576, 672],
        gui_scale: 1,
        fixture: || Fixture::CosmeticParticles {
            view: client_ui::ui_runtime::presentation::player_preview::PreviewView::Doll {
                yaw: 30.0,
                tilt: -10.0,
            },
            emitting: true,
        },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "hud-doll-particles",
        size: [288, 336],
        gui_scale: 1,
        fixture: || Fixture::CosmeticParticles {
            view: client_ui::ui_runtime::presentation::player_preview::PreviewView::Hud,
            emitting: true,
        },
    },
    State {
        name: "home-mod-update-before",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::mods(false, false),
    },
    State {
        name: "home-mod-update",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::mods(true, false),
    },
    State {
        name: "home-mod-update-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: || updates::mods(true, false),
    },
    State {
        name: "settings-mod-update",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::mods(true, true),
    },
    State {
        name: "settings-mod-updating",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: updates::mod_installing,
    },
    State {
        name: "settings-mod-updating-small",
        size: [640, 720],
        gui_scale: GUI_SCALE,
        fixture: updates::mod_installing,
    },
    State {
        name: "settings-mod-installing",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::mod_stage(launcher::mods::UpdatePhase::Installing),
    },
    State {
        name: "settings-mod-reloading",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::mod_stage(launcher::mods::UpdatePhase::Reloading),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "inventory-cursor-durability",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::CursorDurability,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "chat-server-content-idle",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_server_content(false),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "chat-server-content-hover",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || chat_server_content(true),
    },
    State {
        name: "home-update-compact",
        size: [480, 640],
        gui_scale: 3,
        fixture: || updates::home(true, false),
    },
    State {
        name: "home-signed-in-update",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: updates::signed_in,
    },
    State {
        name: "home-update-ready",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::home(true, false),
    },
    State {
        name: "home-update-required",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::home(true, true),
    },
    State {
        name: "home-update-download",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::home(false, false),
    },
    State {
        name: "home-update-small",
        size: SMALL_SIZE,
        gui_scale: 1,
        fixture: || updates::home(true, false),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: cosmetics::wardrobe,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics-head",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || cosmetics::cosmetic_category(launcher::dressing_room::CosmeticCategory::Head),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics-filter",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: cosmetics::cosmetic_filter,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics-filter-narrow",
        size: [320, 720],
        gui_scale: GUI_SCALE,
        fixture: cosmetics::cosmetic_filter,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics-back",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || cosmetics::cosmetic_category(launcher::dressing_room::CosmeticCategory::Back),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics-back-narrow",
        size: [320, 720],
        gui_scale: GUI_SCALE,
        fixture: || cosmetics::cosmetic_category(launcher::dressing_room::CosmeticCategory::Back),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-devil-horns",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: cosmetics::devil_horns,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-dragon-wings",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: cosmetics::dragon_wings,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-parrot",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: cosmetics::parrot,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-parrot-narrow",
        size: [360, 720],
        gui_scale: GUI_SCALE,
        fixture: cosmetics::parrot,
    },
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
        name: "disconnect-device-clock",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::disconnect_device_clock,
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
        name: "tab-list-heads",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: tab_list_heads,
    },
    State {
        name: "tab-list-heads-columns",
        size: [640, 360],
        gui_scale: 3,
        fixture: tab_list_head_columns,
    },
    State {
        name: "tab-list-expired-badges",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || tab_list_badges(false),
    },
    State {
        name: "settings-session-navigation",
        size: [1280, 1600],
        gui_scale: GUI_SCALE,
        fixture: || {
            let mut view = settings("video_forced_index");
            view.over_world = true;
            Fixture::Menu(Box::new(view))
        },
    },
    State {
        name: "settings-video",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_video,
    },
    State {
        name: "settings-video-shader-packs",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_video_shader_packs,
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
        name: "settings-updates",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::settings(launcher::menu::updates::Status::UpToDate),
    },
    State {
        name: "settings-updates-downloading",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || updates::settings(launcher::menu::updates::Status::Downloading),
    },
    State {
        name: "settings-updates-small",
        size: [640, 480],
        gui_scale: GUI_SCALE,
        fixture: || updates::settings(launcher::menu::updates::Status::Failed),
    },
    State {
        name: "settings-general-mod-updates-passive",
        size: [1280, 1200],
        gui_scale: GUI_SCALE,
        fixture: || {
            let mut view = settings(SettingSection::General.definition().name);
            view.updates.active = false;
            Fixture::Menu(Box::new(view))
        },
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
    State {
        name: "toast-shader-pack-loading",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::loading,
    },
    State {
        name: "toast-shader-pack-loaded",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::loaded,
    },
    State {
        name: "toast-shader-pack-failed",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::failed,
    },
    State {
        name: "settings-video-shader-pack",
        size: SHADER_PACK_ROW_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || shader_pack::settings_video(false),
    },
    State {
        name: "settings-video-shader-pack-failed",
        size: SHADER_PACK_ROW_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || shader_pack::settings_video(true),
    },
];

/// Exercises the native chat collection with an unfinished mention.
fn chat_mentions() -> Fixture {
    Fixture::Chat {
        server_hover: None,
        draft: "@",
        suggestions: ["@here", "@Alex", "@Steve"].map(Arc::from).into(),
        messages: Default::default(),
        options: Default::default(),
    }
}

/// Exercises the native command completion collection with target candidates.
fn chat_command_targets() -> Fixture {
    Fixture::Chat {
        server_hover: None,
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
        server_hover: None,
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
        server_hover: None,
        draft,
        suggestions: suggestions.iter().map(|text| Arc::from(*text)).collect(),
        messages: Default::default(),
        options: Arc::new(options),
    }
}

/// Keeps encoded message metadata out of history and reveals it through a hover renderer.
#[cfg(feature = "test-support")]
fn chat_server_content(hover: bool) -> Fixture {
    Fixture::Chat {
        draft: "",
        suggestions: Vec::new(),
        messages: [ui::ChatMessage {
            fifo_sequence: 1,
            received_millis: 0,
            kind: ui::ChatMessageKind::System,
            source: None,
            message: ui_screens::test_support::hover_chat::message().into(),
            parameters: Arc::from([]),
        }]
        .into(),
        options: Default::default(),
        server_hover: Some(hover),
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
            annotation: None,
            skin: None,
        })
        .into(),
    )
}

/// Distinct original head texels alongside missing and invalid skins that must show Steve.
fn tab_list_heads() -> Fixture {
    let Fixture::TabList(mut rows) = tab_list_badges(true) else {
        unreachable!()
    };
    for (index, row) in rows
        .iter_mut()
        .enumerate()
        .filter(|(i, _)| matches!(i, 0 | 2))
    {
        let side = assets::starter_skins::STARTER_SKIN_SIDE;
        let mut pixels = vec![0; (side * side * 4) as usize];
        for y in 8..16 {
            for x in 8..16 {
                let color = if (y == 11 || y == 12) && (x == 9 || x == 10 || x == 13 || x == 14) {
                    [240, 240, 255, 255]
                } else if index == 0 {
                    [60, 155, 100, 255]
                } else {
                    [110, 80, 180, 255]
                };
                let offset = ((y * side + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&color);
                if y < 10 {
                    let hat = ((y * side + x + 32) * 4) as usize;
                    pixels[hat..hat + 4].copy_from_slice(&[35, 60, 110, 255]);
                }
            }
        }
        row.skin = Some(render_api::StandardSkin {
            width: side,
            height: side,
            rgba8: pixels.into(),
            cape: None,
            geometry: None,
        });
    }
    rows.last_mut().unwrap().skin = Some(render_api::StandardSkin {
        width: 1,
        height: 1,
        rgba8: vec![0; 4].into(),
        cape: None,
        geometry: None,
    });
    Fixture::TabList(rows)
}

/// Exercises every roster column at the maximum list size and a larger GUI scale.
fn tab_list_head_columns() -> Fixture {
    let Fixture::TabList(rows) = tab_list_heads() else {
        unreachable!()
    };
    Fixture::TabList(
        (0..80)
            .map(|index| {
                let mut row = rows[index % rows.len()].clone();
                row.name = format!("Player{index:02}").into();
                row
            })
            .collect(),
    )
}

/// Opens a settings category by its registered name rather than a copied index.
pub fn settings(section: &str) -> MenuView {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings.section = launcher::menu::settings_sections::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == section).then_some(*index))
        .expect("registered settings category");
    view
}

/// Shows an ordinary OreUI route without account, world or network services.
fn settings_video() -> Fixture {
    Fixture::Menu(Box::new(settings(SettingSection::Video.definition().name)))
}

/// Scrolls Video to the renderer picker of a `shaderpacks` build listing two packs, with the
/// shader-pack folder's Open folder button a desktop host shows below it.
fn settings_video_shader_packs() -> Fixture {
    use launcher::menu::settings_storage::{StorageAction, StorageFolder};
    let mut view = settings(SettingSection::Video.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
    Arc::make_mut(&mut view.storage).opens_folders = true;
    view.focused_action = Some(MenuAction::SettingsStorage(StorageAction::OpenFolder(
        StorageFolder::ShaderPacks,
    )));
    Fixture::Menu(Box::new(view))
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
