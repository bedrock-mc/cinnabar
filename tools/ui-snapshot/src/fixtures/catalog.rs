//! The ordered catalog of named snapshots and their viewport and fixture inputs.

#[cfg(feature = "test-support")]
use super::chat::chat_server_content;
use super::chat::{
    chat_background, chat_command_background, chat_command_targets, chat_completion,
    chat_history_width, chat_mentions, colored_mentions,
};
#[cfg(feature = "test-support")]
use super::cosmetics;
use launcher::local_worlds::world::UnavailableReason;

use super::{
    Fixture, GUI_SCALE, KEYBOARD_SIZE, SIZE, SMALL_SIZE, SettingId, SettingSection, State,
    TALL_SIZE, collection_reveal, hud_card_fit, hud_cards, hud_editor, local_identity,
    minecraft_import, mods, out_of_game, profile, roster, settings, settings_screens,
    shader_options, shader_pack, updates,
};

/// A capture at GUI scale 1 that needs no game assets.
const fn asset_free(name: &'static str, size: [u32; 2], fixture: fn() -> Fixture) -> State {
    State {
        name,
        size,
        gui_scale: 1,
        fixture,
    }
}

pub(super) const STATES: &[State] = &[
    asset_free("sprite-outline-before", [800, 240], || {
        Fixture::SpriteOutline { outline: false }
    }),
    asset_free("sprite-outline", [800, 240], || Fixture::SpriteOutline {
        outline: true,
    }),
    asset_free("saturation-overlap-before", [640, 240], || {
        Fixture::SaturationOverlap {
            crop: true,
            source_alpha: 255,
        }
    }),
    asset_free("saturation-overlap", [640, 240], || {
        Fixture::SaturationOverlap {
            crop: false,
            source_alpha: 255,
        }
    }),
    asset_free("saturation-mask-regressions", [640, 240], || {
        Fixture::MaskRegression
    }),
    asset_free("saturation-translucent-overlap", [640, 240], || {
        Fixture::SaturationOverlap {
            crop: false,
            source_alpha: 64,
        }
    }),
    State {
        name: "mod-hud-card-fit-before",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || hud_card_fit::fixture(false),
    },
    State {
        name: "mod-hud-card-fit-after",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || hud_card_fit::fixture(true),
    },
    State {
        name: "collection-reveal-before",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || collection_reveal::fixture(false),
    },
    State {
        name: "collection-reveal-after",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || collection_reveal::fixture(true),
    },
    State {
        name: "hud-editor-before",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || hud_editor::fixture(false),
    },
    State {
        name: "hud-editor-after",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || hud_editor::fixture(true),
    },
    State {
        name: "mod-local-player-neutral",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || local_identity::fixture(false),
    },
    State {
        name: "mod-local-player-head",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || local_identity::fixture(true),
    },
    State {
        name: "mod-hud-card-previews",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || hud_cards::fixture(true),
    },
    State {
        name: "mod-hud-card-previews-empty",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || hud_cards::fixture(false),
    },
    State {
        name: "sign-in-cache-error",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || out_of_game::sign_in_recovery(false),
    },
    State {
        name: "sign-in-device-code",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || out_of_game::sign_in_recovery(true),
    },
    State {
        name: "backup-font",
        size: [640, 360],
        gui_scale: 2,
        fixture: || Fixture::BackupFont,
    },
    State {
        name: "jsonui-tiled-image",
        size: [256, 128],
        gui_scale: 1,
        fixture: || Fixture::TiledImage { scale: 1.0 },
    },
    State {
        name: "jsonui-tiled-image-extreme",
        size: [256, 128],
        gui_scale: 1,
        fixture: || Fixture::TiledImage { scale: 0.001 },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "add-server-keyboard",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::MenuKeyboard,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "settings-draft-return",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::SettingsDraft,
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
        name: "profile-overview-fallback",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: profile::overview,
    },
    State {
        name: "profile-overview-fallback-narrow",
        size: [360, TALL_SIZE[1]],
        gui_scale: GUI_SCALE,
        fixture: profile::overview,
    },
    State {
        name: "profile-stats-fallback",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: profile::statistics,
    },
    State {
        name: "profile-stats-fallback-narrow",
        size: [360, SIZE[1]],
        gui_scale: GUI_SCALE,
        fixture: profile::statistics,
    },
    State {
        name: "profile-partial-fallback",
        size: TALL_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || profile::partial(launcher::menu::ProfileTab::Overview),
    },
    State {
        name: "profile-partial-stats-fallback",
        size: [360, SIZE[1]],
        gui_scale: GUI_SCALE,
        fixture: || profile::partial(launcher::menu::ProfileTab::Stats),
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
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: cosmetics::wardrobe,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics-locked",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: cosmetics::locked_cosmetics,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "dressing-room-cosmetics-locked-narrow",
        size: [480, 1280],
        gui_scale: GUI_SCALE,
        fixture: cosmetics::locked_cosmetics,
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
        fixture: || Fixture::CraftOverlap {
            selected: false,
            block_identity: 0,
        },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "craft-overlap-selected",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::CraftOverlap {
            selected: true,
            block_identity: 0,
        },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "craft-block-identity-selected",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::CraftOverlap {
            selected: true,
            block_identity: i32::MIN + 1,
        },
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
        name: "disconnect-local-world-offline",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::disconnect_local_world_offline,
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
        name: "local-docker-missing",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || out_of_game::docker_prompt(UnavailableReason::DockerMissing),
    },
    State {
        name: "local-docker-not-running",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || out_of_game::docker_prompt(UnavailableReason::DockerNotRunning),
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
        name: "chat-colored-mentions",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: colored_mentions,
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
        fixture: || roster::badges(true),
    },
    State {
        name: "tab-list-heads",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: roster::heads,
    },
    State {
        name: "tab-list-heads-columns",
        size: [640, 360],
        gui_scale: 3,
        fixture: roster::head_columns,
    },
    State {
        name: "tab-list-expired-badges",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || roster::badges(false),
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
        name: "settings-video-shader-shortcut",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::video_shortcut,
    },
    State {
        name: "settings-shaders",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::pack_picker,
    },
    State {
        name: "settings-shaders-remove",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::removal_dialog,
    },
    State {
        name: "settings-shaders-empty",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::empty_library,
    },
    State {
        name: "settings-shaders-empty-without-drops",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_pack::empty_library_without_drops,
    },
    State {
        name: "settings-shaders-narrow-library",
        size: [800, 720],
        gui_scale: GUI_SCALE,
        fixture: shader_pack::long_library,
    },
    State {
        name: "settings-storage",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: out_of_game::settings_storage,
    },
    State {
        name: "settings-keyboard",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_screens::settings_keyboard,
    },
    State {
        name: "settings-general",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_screens::settings_general_toggle(SettingId::FilterProfanity),
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
        name: "settings-updates-mod-updates-passive",
        size: [1280, 1200],
        gui_scale: GUI_SCALE,
        fixture: || {
            let mut view = settings(SettingSection::Updates.definition().name);
            view.updates.active = false;
            Fixture::Menu(Box::new(view))
        },
    },
    State {
        name: "settings-general-advanced",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_screens::settings_general_toggle(SettingId::RecordNetworkSession),
    },
    State {
        name: "settings-general-advanced-on",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_screens::settings_general_diagnostics_on,
    },
    State {
        name: "settings-keyboard-physical-label",
        size: KEYBOARD_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_screens::settings_keyboard_layout(false),
    },
    State {
        name: "settings-keyboard-azerty",
        size: KEYBOARD_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_screens::settings_keyboard_layout(true),
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
        name: "settings-shaders-options-button",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_options::options_button,
    },
    State {
        name: "settings-shader-options",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_options::main_screen,
    },
    State {
        name: "settings-shader-options-lighting",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_options::lighting,
    },
    State {
        name: "settings-shader-options-rest",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: shader_options::rest,
    },
    State {
        name: "settings-video-culling-on",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_screens::settings_video_culling(true),
    },
    State {
        name: "settings-video-culling-off",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_screens::settings_video_culling(false),
    },
];
