//! Which vanilla screen each menu state opens, the screen globals it binds from
//! the menu's view, and how the screen's pressed buttons map back to menu
//! actions. Screens bind strictly (an unbound visibility flag reads false, as
//! the vanilla screen controllers answer it), so each spec names the flags its
//! layout needs on.

use std::sync::Arc;

use json_ui::{Context, DataSource, HitKind, HitRegion, Scalar};
use serde_json::Value;

use super::{menu_caret::with_caret, play_screen};
use crate::menu::{MenuAction, MenuDialog, MenuField, MenuScreen, MenuView, auth::AuthState};

/// Settings selector index vars as 1.26.50's settings screen assigns them.
pub(super) const SETTINGS_SECTIONS: &[(&str, u8)] = &[
    ("server_forced_index", 1),
    ("accessibility_forced_index", 2),
    ("how_to_play_index", 3),
    ("game_forced_index", 4),
    ("classroom_forced_index", 5),
    ("edu_cloud_level_forced_index", 6),
    ("multiplayer_forced_index", 7),
    ("world_forced_index", 8),
    ("members_forced_index", 9),
    ("realms_saves_forced_index", 10),
    ("subscription_forced_index", 11),
    ("backup_forced_index", 12),
    ("dev_options_forced_index", 13),
    ("keyboard_and_mouse_forced_index", 14),
    ("controller_and_switch_forced_index", 15),
    ("touch_forced_index", 16),
    ("party_forced_index", 17),
    ("general_forced_index", 18),
    ("account_forced_index", 19),
    ("creator_forced_index", 20),
    ("video_forced_index", 21),
    ("view_subscriptions_forced_index", 22),
    ("sound_forced_index", 23),
    ("global_texture_pack_forced_index", 24),
    (
        "storage_management_forced_index",
        crate::menu::settings_storage::SECTION_INDEX,
    ),
    ("edu_cloud_storage_forced_index", 26),
    ("language_forced_index", 27),
    ("preview_forced_index", 28),
    ("debug_forced_index", 29),
    ("discovery_debug_forced_index", 30),
    ("ui_debug_forced_index", 31),
    ("edu_debug_forced_index", 32),
    ("marketplace_debug_forced_index", 33),
    ("flighting_debug_forced_index", 34),
    ("realms_debug_forced_index", 35),
    ("automation_forced_index", 36),
    ("level_texture_pack_index", 37),
    ("broadcast_forced_index", 38),
    ("addon_index", 39),
    ("invite_links_forced_index", 40),
    ("general_invite_link_forced_index", 41),
    ("advanced_invite_link_forced_index", 42),
    ("realms_advanced_forced_index", 43),
];
/// The section the settings screen opens on before one is picked.
const VIDEO_SECTION: &str = "video_forced_index";

/// Lang key the vanilla start and pause controllers give the unlock-full-game text.
const UNLOCK_FULL_GAME_TEXT: &str = "trial.pauseScreen.buyGame";

/// The retail desktop context for this build's platform.
pub(super) fn retail_context() -> Context {
    Context::retail(cfg!(target_os = "macos"))
}

/// Vanilla start screen variables for a full-game, non-edu
/// account: demo, edu and unlock controls stay ignored.
fn start_screen_vars(context: Context) -> Context {
    unlock_text(context)
        .with_flag("unlock_full_game_button_ignored", true)
        .with_flag("featured_world_ignored", true)
        .with_flag("courses_ignored", true)
        .with_flag("edu_feedback_ignored", true)
        .with_flag("play_button_visible", true)
        .with_flag("use_single_column_for_buttons", false)
        .with_flag("can_swap_vr_mode", false)
        .with_flag("showing_new_player_flow_buttons", false)
        .with_flag("supports_launching_legacy_version", false)
}

fn unlock_text(context: Context) -> Context {
    context.with_var(
        "unlock_full_game_button_text",
        Value::String(UNLOCK_FULL_GAME_TEXT.into()),
    )
}

/// The screen a menu state opens and what it binds.
pub(super) struct MenuScreenData {
    pub(super) reference: &'static str,
    pub(super) context: Context,
    pub(super) data: DataSource,
    /// A screen drawn over this one, which then takes all input (a Marketplace popup).
    pub(super) overlay: Option<Box<MenuScreenData>>,
}

pub(super) type Translate<'a> = &'a dyn Fn(&str) -> Option<Arc<str>>;

pub(super) fn text(value: impl Into<String>) -> Scalar {
    Scalar::Text(value.into())
}

pub(super) fn translated(translate: Translate<'_>, key: &str, fallback: &str) -> String {
    translate(key).map_or_else(|| fallback.to_owned(), |value| value.to_string())
}

pub(super) fn flags(data: &mut DataSource, on: &[&str]) {
    for name in on {
        data.set_global(*name, Scalar::Bool(true));
    }
}

pub use launcher::menu::menu_reference;

/// The vanilla screen for `view`, or `None` for states without one (the
/// programmatic launcher then draws them).
pub(super) fn screen_data(view: &MenuView, translate: Translate<'_>) -> Option<MenuScreenData> {
    let mut data = DataSource::new();
    data.set_strict(true);
    let mut context = base_context();
    let reference = if let Some(progress) = &view.local.progress {
        local_world_progress(&mut data, translate, progress);
        // The world-modal progress panel the overworld loading screen also wraps; its dirt
        // backdrop needs block textures the menu engine does not carry.
        LOCAL_WORLD_PROGRESS_SCREEN
    } else if view.connecting {
        super::join_progress::bind(&view.feeds.join, &mut data, translate)
    } else if let Some(error) = &view.disconnect_message {
        let words = crate::menu::disconnect::describe(error);
        data.set_global(
            "#title_text",
            text(translated(translate, words.title, words.title)),
        );
        let body = match words.body {
            crate::menu::disconnect::DisconnectBody::Key(key) => translated(translate, key, key),
            crate::menu::disconnect::DisconnectBody::Server(message) => message,
        };
        data.set_global("#disconnect_text", text(body));
        if view.can_reconnect {
            context = context.with_flag("cinnabar_reconnect", true).with_var(
                "cinnabar_reconnect_text",
                Value::String(translated(translate, "cinnabar.reconnect", "Reconnect")),
            );
        }
        "disconnect.disconnect_screen"
    } else {
        let reference = menu_reference(view.screen)?;
        match view.screen {
            MenuScreen::Death => {
                flags(
                    &mut data,
                    &[
                        "#buttons_and_deathmessage_visible",
                        "#respawn_visible",
                        "#respawn_enabled",
                        "#quit_visible",
                        "#quit_enabled",
                    ],
                );
            }
            MenuScreen::Pause => {
                data.set_global(
                    "#playername",
                    text(super::accounts::current_name(view).to_owned()),
                );
                flags(&mut data, &["#playername_visible"]);
                if view.hosting {
                    // Vanilla's pause invite entry sits in a panel 1.26.50 no longer draws; the
                    // friends drawer is the pause screen's visible social entry.
                    flags(
                        &mut data,
                        &[
                            "#legacy_invite_button_visible",
                            "#legacy_invite_button_enabled",
                            "#friends_drawer_button_visible",
                            "#friends_drawer_button_enabled",
                        ],
                    );
                }
                data.set_global("#unlock_full_game_button_text", text(UNLOCK_FULL_GAME_TEXT));
                // A non-edu client draws the retail pause content, not edu_pause's.
                context = unlock_text(context)
                    .with_flag("ignore_edu_pause", true)
                    .with_var(
                        "store_button_text",
                        Value::String(server_store_text(translate)),
                    );
            }
            MenuScreen::Home => {
                start_screen(view, &mut data, translate);
                context = start_screen_vars(context);
            }
            MenuScreen::Play | MenuScreen::Social | MenuScreen::Servers => {
                super::play_screen::bind(view, &mut data);
            }
            MenuScreen::AddServer => {
                add_server_screen(view, &mut data, translate);
                // The controller's edit mode swaps Play for Remove.
                context = context.with_flag("edit_mode", view.editing.is_some());
            }
            MenuScreen::Settings => {
                settings_screen(view, &mut data, translate);
                super::settings_defaults::bind(&mut data, &|key: &str| {
                    translated(translate, key, key)
                });
                super::settings_controls::bind(view, &mut data, &|key: &str| {
                    translated(translate, key, key)
                });
                super::settings_language::bind(view, &mut data);
                super::settings_account::bind(view, &mut data);
                super::settings_resources::bind(&mut data);
                super::settings_storage::bind(view, &mut data, &|key| {
                    translated(translate, key, key)
                });
                super::settings_keys::bind(view, &mut data, &|key: &str| {
                    translated(translate, key, key)
                });
                super::global_resources::bind(&view.global_resources, &mut data);
                return Some(MenuScreenData {
                    reference,
                    context: settings_context(context),
                    data,
                    overlay: super::global_resources::overlay(&view.global_resources),
                });
            }
            MenuScreen::Store => return store_screen(view, &context, translate),
            MenuScreen::Invite => {
                super::invite_screen::bind(view, &mut data, translate);
                context = super::invite_screen::context(view, context);
            }
            MenuScreen::Profile
            | MenuScreen::DressingRoom
            | MenuScreen::Inbox
            | MenuScreen::Friends => return None,
        }
        reference
    };
    Some(MenuScreenData {
        reference,
        context,
        data,
        overlay: None,
    })
}

const LOCAL_WORLD_PROGRESS_SCREEN: &str = "progress.world_convert_modal_progress_screen";

/// The local-world loading screen: vanilla's "Starting World" title over the current stage,
/// a determinate bar when the stage knows its total, and Cancel until the join starts.
fn local_world_progress(
    data: &mut DataSource,
    translate: Translate<'_>,
    progress: &crate::local_worlds::Progress,
) {
    use crate::local_worlds::Stage;
    let (title, message) = match progress.stage {
        Stage::StartingServer => (
            translated(
                translate,
                "progressScreen.title.connectingLocal",
                "Starting World",
            ),
            translated(
                translate,
                "progressScreen.message.building",
                "Building terrain",
            ),
        ),
        Stage::Connecting => (
            translated(
                translate,
                "progressScreen.title.connectingLocal",
                "Starting World",
            ),
            translated(
                translate,
                "progressScreen.message.locating",
                "Locating server",
            ),
        ),
        stage => (
            translated(
                translate,
                "progressScreen.title.connectingLocal",
                "Starting World",
            ),
            stage.title().to_owned(),
        ),
    };
    data.set_global("#title_text", text(title));
    let detail = match progress.stage {
        Stage::StartingServer | Stage::Connecting => message,
        _ if progress.detail.is_empty() => message,
        _ => format!("{message}\n{}", progress.detail),
    };
    data.set_global("#progress_text", text(detail));
    match progress.fraction {
        Some(fraction) => {
            flags(data, &["#loading_bar_visible"]);
            // The fancy bar binds this as its `#clip_ratio`: the share clipped away.
            data.set_global(
                "#loading_bar_percentage",
                Scalar::Num(1.0 - f64::from(fraction)),
            );
            data.set_global("#loading_bar_total_amount", Scalar::Num(1000.0));
            data.set_global(
                "#loading_bar_current_amount",
                Scalar::Num((f64::from(fraction) * 1000.0).round()),
            );
        }
        None => flags(data, &["#bar_animation_visible"]),
    }
    if progress.stage != Stage::Connecting {
        flags(data, &["#cancel_visible"]);
        data.set_global(
            "#cancel_button_text",
            text(translated(translate, "gui.cancel", "Cancel")),
        );
    }
}

/// The Marketplace screen (and popup) for the published store state.
fn store_screen(
    view: &MenuView,
    context: &Context,
    translate: Translate<'_>,
) -> Option<MenuScreenData> {
    let snapshot = view.store.as_deref()?;
    let tr = |key: &str| translated(translate, key, key);
    let screens = crate::store::screens(snapshot, context, &tr);
    let convert = |spec: crate::store::ScreenSpec| MenuScreenData {
        reference: spec.reference,
        context: spec.context,
        data: spec.data,
        overlay: None,
    };
    let mut base = convert(screens.base);
    base.overlay = screens.overlay.map(|spec| Box::new(convert(spec)));
    Some(base)
}

fn start_screen(view: &MenuView, data: &mut DataSource, translate: Translate<'_>) {
    let gamertag = super::accounts::current_name(view).to_owned();
    data.set_global("#playername", text(gamertag.clone()));
    data.set_global("#gamertag_label", text(gamertag));
    let portrait = super::accounts::current_picture(view).is_some()
        || !view.feeds.home.persona_head.is_empty();
    data.set_global("#show_gamerpic", Scalar::Bool(portrait));
    flags(
        data,
        &[
            "#show_paper_doll",
            "#persona_and_skins_enabled",
            "#profile_button_a_visible",
        ],
    );
    data.set_global(
        "#is_paper_doll_visible",
        Scalar::Bool(view.profile_icon.is_some()),
    );
    super::start_feed::bind(view, data);
    data.set_global("#version", text(version_label(protocol::GAME_VERSION)));
    data.set_global("#unlock_full_game_button_text", text(UNLOCK_FULL_GAME_TEXT));
    data.set_global("#edu_demo_only_ui_visible", Scalar::Bool(false));
    // Retail Realms is enabled, so its row shows between Settings and
    // Marketplace, as on the release client.
    flags(
        data,
        &[
            "#online_stack_visible",
            "#realms_promo_visible",
            "#not_realms_promo_visible_and_supports_launching_legacy_version",
            "#dressing_room_button_visible",
            "#is_appearance_visible",
        ],
    );
    match &view.auth_state {
        AuthState::SignedOut | AuthState::Failed(_) => {
            flags(data, &["#sign_in_visible", "#upper_online_buttons_visible"])
        }
        AuthState::Checking => {
            flags(data, &["#signingin_visible"]);
            data.set_global(
                "#signingin_text",
                text(translated(
                    translate,
                    "xbox.signingin",
                    "Signing in with your Microsoft account...",
                )),
            );
        }
        AuthState::Authenticated => flags(data, &["#gamertag_pic_and_label_visible"]),
        AuthState::AwaitingCode { .. } => {}
    }
}

/// The pause store button on a third-party server, as vanilla names it: "%s Store" with the server's store name, else the generic "Server".
fn server_store_text(translate: Translate<'_>) -> String {
    let server = translated(translate, "menu.serverGenericName", "Server");
    translated(translate, "menu.serverStore", "%s Store").replacen("%s", &server, 1)
}

/// The start screen's version: the release client shows `1.26.50` as `v26.50`.
fn version_label(game_version: &str) -> String {
    format!(
        "v{}",
        game_version.strip_prefix("1.").unwrap_or(game_version)
    )
}

/// The vanilla two-button popup a launcher dialog opens, and the action its
/// left (confirm) button takes; the right button dismisses.
pub(super) fn dialog_model(
    view: &MenuView,
    dialog: MenuDialog,
    translate: Translate<'_>,
) -> (json_ui::FormModel, MenuAction) {
    let (title, body, button1, button2, confirm) = match dialog {
        MenuDialog::Accounts => (
            "Accounts".into(),
            String::new(),
            "Close".into(),
            "Close".into(),
            MenuAction::DismissDialog,
        ),
        MenuDialog::SettingsResetGroup(group) => {
            return super::settings_reset::dialog_model(group, translate);
        }
        MenuDialog::SettingsResetBindings(gamepad) => (
            translated(
                translate,
                "controllerLayoutScreen.resetAllBindings",
                "Reset to Default",
            ),
            translated(
                translate,
                "controllerLayoutScreen.confirmation.reset",
                "Reset all bindings to their defaults?",
            ),
            translated(translate, "options.continue", "Continue"),
            translated(translate, "controllerLayoutScreen.cancel", "Cancel"),
            MenuAction::SettingsConfirmResetBindings(gamepad),
        ),
        MenuDialog::SettingsSupport(dialog) => {
            return super::settings_support::dialog_model(dialog, translate);
        }
        MenuDialog::StorageDelete | MenuDialog::StorageError => {
            return super::settings_storage::dialog_model(view, dialog, translate);
        }
        MenuDialog::Exit => (
            translated(
                translate,
                "gui.warning.exitGameWarning",
                "Do you want to exit Minecraft?",
            ),
            String::new(),
            translated(translate, "gui.yes", "Yes"),
            translated(translate, "gui.no", "No"),
            MenuAction::ConfirmExit,
        ),
        // The popup's title is one line, so the server names it and the body asks.
        MenuDialog::RemoveSaved(index) => (
            view.servers
                .get(index)
                .map(|server| server.name.clone())
                .unwrap_or_default(),
            translated(
                translate,
                "addExternalServerScreen.removeConfirmation",
                "Are you sure you want to remove this server?",
            ),
            translated(
                translate,
                "addExternalServerScreen.removeButtonLabel",
                "Remove",
            ),
            translated(translate, "gui.cancel", "Cancel"),
            MenuAction::ConfirmRemoveSaved(index),
        ),
    };
    let model = json_ui::FormModel::Modal(json_ui::ModalForm {
        title,
        body,
        button1,
        button2,
    });
    (model, confirm)
}

/// Vanilla's first-join question for a NetherNet server reached over plain http, in the
/// active language or vanilla's English.
pub(super) fn server_trust_model(url: &str, translate: Translate<'_>) -> json_ui::FormModel {
    let message = translated(
        translate,
        "permissions.servertrust.message",
        "You are connecting to %1$s for the first time. Only trust servers you recognize.",
    );
    json_ui::FormModel::Modal(json_ui::ModalForm {
        title: translated(
            translate,
            "permissions.servertrust.title",
            "Trust this server?",
        ),
        body: message.replace("%1$s", url),
        button1: translated(
            translate,
            "permissions.servertrust.button.trust",
            "Trust and Join",
        ),
        button2: translated(
            translate,
            "permissions.servertrust.button.doNotTrust",
            "Don't Trust",
        ),
    })
}

/// The host's answer to `name`'s Discord join request, with vanilla's Accept and Decline.
pub(super) fn join_request_model(name: &str, translate: Translate<'_>) -> json_ui::FormModel {
    json_ui::FormModel::Modal(json_ui::ModalForm {
        title: crate::menu::join_requests::title(name),
        body: String::new(),
        button1: translated(translate, "gui.accept", "Accept"),
        button2: translated(translate, "gui.decline", "Decline"),
    })
}

fn add_server_screen(view: &MenuView, data: &mut DataSource, translate: Translate<'_>) {
    let title = if view.editing.is_some() {
        translated(translate, "addServer.title.edit", "Edit Server")
    } else {
        translated(translate, "addServer.title", "Add Server")
    };
    data.set_global("#title_text", text(title));
    let boxes = [
        ("#name_text_box_content", MenuField::Name, &view.name),
        ("#ip_text_box_content", MenuField::Address, &view.address),
        ("#port_text_box_content", MenuField::Port, &view.port),
    ];
    for (key, field, value) in boxes {
        data.set_global(key, text(with_caret(view, field, value).into_owned()));
    }
    let ready = !view.name.trim().is_empty() && !view.address.trim().is_empty();
    data.set_global("#save_button_enabled", Scalar::Bool(ready));
    data.set_global("#save_button_disabled", Scalar::Bool(!ready));
    data.set_global("#play_button_enabled", Scalar::Bool(ready));
    data.set_global("#play_button_disabled", Scalar::Bool(!ready));
}

/// `host:port` split for the separate IP and port boxes (a bare host keeps the
/// default Bedrock port shown).
/// Select the section and the titles supplied by its vanilla toggle property bag.
fn settings_screen(view: &MenuView, data: &mut DataSource, translate: Translate<'_>) {
    super::enhanced_setting::bind(view, data);
    let section = match view.settings_section {
        0 => section_index(VIDEO_SECTION),
        picked => picked,
    };
    data.select_radio("navigation_tab", usize::from(section));
    let step = view
        .gui_scale_choices
        .iter()
        .position(|choice| choice.offset == view.gui_scale_offset)
        .unwrap_or(0);
    data.set_global("#gui_scale", Scalar::Num(step as f64));
    data.set_global(
        "#gui_scale_steps",
        Scalar::Num(view.gui_scale_choices.len() as f64),
    );
    data.set_global(
        "#gui_scale_slider_label",
        text(format!(
            "{}: {}",
            translated(
                translate,
                "options.guiScale.optionName",
                "GUI Scale Modifier"
            ),
            view.gui_scale_offset
        )),
    );
    data.set_global(
        "#gui_scale_text_value",
        text(view.gui_scale_offset.to_string()),
    );
    data.set_global(
        "#gui_scale_enabled",
        Scalar::Bool(view.gui_scale_choices.len() > 1),
    );
    data.set_global(
        "#gui_scale_disabled_option_visible",
        Scalar::Bool(view.gui_scale_choices.len() <= 1),
    );
    data.set_global(
        "#gui_scale_disabled_option_tooltip_text",
        text(translated(
            translate,
            "options.guiScale.disabled",
            "GUI scale cannot be adjusted at this resolution.",
        )),
    );
    flags(data, &["#gui_scale_visible"]);
    data.set_global("#full_screen", Scalar::Bool(view.fullscreen));
    flags(data, &["#full_screen_enabled"]);
    let variable = SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*index == section).then_some(*name));
    let title = match variable {
        Some("accessibility_forced_index") => "options.accessibility.title",
        Some("keyboard_and_mouse_forced_index") => "options.keyboardAndMouseSettings",
        Some("controller_and_switch_forced_index") => "options.controllerSettings",
        Some("general_forced_index") => "options.generalTitle",
        Some("account_forced_index") => "options.accountTitle",
        Some("creator_forced_index") => "options.creatorTitle",
        Some("sound_forced_index") => "options.sounds.title",
        Some("global_texture_pack_forced_index") => "menu.globalpacks",
        Some("storage_management_forced_index") => "menu.storageManagement",
        Some("language_forced_index") => "options.language",
        Some("view_subscriptions_forced_index") => "options.viewSubscriptions",
        _ => "options.videoTitle",
    };
    data.set_global("#section_title", text(translated(translate, title, title)));
    data.set_global(
        "#dialog_title",
        text(translated(translate, "menu.settings", "menu.settings")),
    );
}

#[cfg(test)]
use launcher::menu::SETTINGS_SCREEN;

/// Supplies the production settings context for offline layout checks.
#[cfg(test)]
pub(super) fn settings_target() -> (&'static str, Context) {
    (SETTINGS_SCREEN, settings_context(base_context()))
}

/// Every launcher screen's context before its own vars.
fn base_context() -> Context {
    retail_context().with_flag("can_quit", true).with_var(
        "play_button_target",
        Value::String("button.menu_play".into()),
    )
}

/// The static vars vanilla's settings screen sets for the global settings a
/// desktop client opens from the start screen: no world, realm or creation state.
fn settings_context(context: Context) -> Context {
    let flags: &[(&str, bool)] = &[
        ("include_controls_and_settings_sections", true),
        // Set when "/settings" resolves to JSON UI, as retail does with the
        // `mc-new-settings-screen` flight off.
        ("include_migrated_json_ui_settings_tabs", true),
        // The general sub-controller's vars on a desktop platform.
        ("show_fullscreen_toggle", true),
        ("supports_user_configured_safezone", true),
        ("feedback_visible", true),
        ("is_global_texture_packs_visible", true),
        ("supports_cross_platform_play_toggle", false),
        ("is_world_create", false),
        ("is_world_edit", false),
        ("is_template_create", false),
        ("is_realms_edit", false),
        ("is_realm_slot", false),
        ("is_mp_host", false),
        ("is_mp_client", false),
        ("non_config_realms_env", false),
        ("realms_pack_feature_enabled", false),
        ("gamepad_supported", true),
        ("keyboard_and_mouse_supported", true),
        ("touch_supported", false),
        ("supports_flite_tts", false),
        ("platform_tts_exists", false),
        ("ignore_creator_section", false),
        ("may_include_world_section", false),
        ("ignore_global_resources_section", false),
        ("ignore_storage_section", false),
        ("ignore_profile_switch_account_button", false),
        ("ignore_profile_sso_toggle", true),
        ("ignore_profile_sign_out_button", false),
        ("ignore_controller_layout", false),
        ("edu_ignore_cloud_storage", true),
        ("storage_location_switch_enabled", false),
        ("copy_interal_storage_button_enabled", false),
        ("show_preview_button", false),
        ("show_preview_app1_button", false),
        ("show_preview_app2_button", false),
        ("debug_settings", false),
        ("party_settings_enabled", false),
        // Select vanilla's compact treatment; the live retail flight remains unverified.
        ("settings_spatial_pattern_fix_enabled", false),
        ("display_copyright_info", false),
        ("is_pregame", true),
        ("is_editor_mode_enabled", false),
    ];
    let context = flags.iter().fold(context, |context, (name, value)| {
        context.with_flag(name, *value)
    });
    SETTINGS_SECTIONS
        .iter()
        .fold(context, |context, (name, index)| {
            context.with_var(name, Value::from(*index))
        })
}

fn section_index(name: &str) -> u8 {
    SETTINGS_SECTIONS
        .iter()
        .find_map(|(section, index)| (*section == name).then_some(*index))
        .unwrap_or_default()
}

/// The menu action a pressed region means on `view`'s screen.
pub(super) fn action_for(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    if view.screen == MenuScreen::Settings
        && let Some(action) = super::settings_language::action(region)
            .or_else(|| super::settings_account::action(region))
            .or_else(|| super::settings_storage::action(region))
            .or_else(|| super::settings_support::action(region))
            .or_else(|| super::settings_reset::action(view, region))
            .or_else(|| super::settings_keys::action(region))
            .or_else(|| super::settings_controls::action(view, region))
    {
        return Some(action);
    }
    if view.screen == MenuScreen::Store {
        return crate::store::action(view.store.as_deref(), region).map(MenuAction::Store);
    }
    if view.screen == MenuScreen::Invite
        && let Some(action) = super::invite_screen::action(region)
    {
        return Some(action);
    }
    if view.screen == MenuScreen::Settings
        && let Some(action) = super::global_resources::action(view, region)
    {
        return Some(action);
    }
    let index = region.collection_index;
    let collection = region.collection.as_deref();
    if region.kind == HitKind::Toggle {
        return toggle_action(view, region);
    }
    if region.kind == HitKind::EditBox {
        return match region.control_name.as_deref() {
            Some("#name_text_box") => Some(MenuAction::AddName),
            Some("#ip_text_box") => Some(MenuAction::AddAddress),
            Some("#port_text_box") => Some(MenuAction::AddPort),
            _ => None,
        };
    }
    Some(match region.pressed.as_deref()? {
        "button.cinnabar_reconnect" if view.disconnect_message.is_some() && view.can_reconnect => {
            MenuAction::Reconnect
        }
        // The local-world loading screen's Cancel closes the world.
        "button.menu_exit" if view.local.progress.is_some() => {
            MenuAction::LocalWorld(crate::menu::LocalWorldAction::Back)
        }
        "button.menu_continue" if view.screen == MenuScreen::Pause => MenuAction::PauseResume,
        // Acknowledging a disconnect clears it (every action does).
        "button.menu_continue" | "button.menu_leave_screen" | "button.menu_select" => {
            MenuAction::DismissDialog
        }
        "button.menu_settings" if view.screen == MenuScreen::Pause => MenuAction::PauseSettings,
        "button.menu_invite_players" if view.hosting => {
            MenuAction::Invite(launcher::menu::invite::Action::Open)
        }
        "button.friends_drawer" if view.hosting && view.screen == MenuScreen::Pause => {
            MenuAction::Invite(launcher::menu::invite::Action::Open)
        }
        "button.menu_settings" => MenuAction::Navigate(MenuScreen::Settings),
        "button.menu_quit" | "button.main_menu_button" => MenuAction::PauseDisconnect,
        "button.respawn_button" => MenuAction::Respawn,
        "button.gathering" => MenuAction::OpenLiveEvent,
        "button.menu_inbox" => MenuAction::Navigate(MenuScreen::Inbox),
        "button.friends_drawer" | "button.menu_friends" => {
            MenuAction::Navigate(MenuScreen::Friends)
        }
        "button.menu_store" => MenuAction::Store(crate::store::OPEN),
        "button.menu_play" => MenuAction::Navigate(MenuScreen::Play),
        "button.menu_realms" => MenuAction::Navigate(MenuScreen::Social),
        "button.menu_servers" => MenuAction::Navigate(MenuScreen::Servers),
        "button.signin" => MenuAction::StartSignIn,
        "button.sign_out" => MenuAction::SignOut,
        "button.menu_profile" if view.screen == MenuScreen::Home => MenuAction::OpenAccounts,
        "button.to_profile_screen" => MenuAction::Navigate(MenuScreen::DressingRoom),
        "button.menu_profile" | "button.manage_account" => {
            MenuAction::Navigate(MenuScreen::Profile)
        }
        // The join progress screen's cancel; the menu drops it where vanilla cannot cancel.
        "button.menu_exit" if view.connecting => MenuAction::AddBack,
        "button.menu_cancel" if view.auth_state_awaiting_code() => MenuAction::CancelSignIn,
        "button.menu_exit" if view.auth_state_awaiting_code() => MenuAction::CancelSignIn,
        "button.menu_exit" => match view.screen {
            MenuScreen::Home => MenuAction::OpenExitDialog,
            _ => MenuAction::AddBack,
        },
        "button.save_button" => MenuAction::AddSave,
        "button.play_button" => MenuAction::AddSaveConnect,
        "button.remove_button" => MenuAction::RemoveSavedDialog(view.editing?),
        "button.menu_network_world_item" => match collection? {
            "friends_network_worlds" => MenuAction::PlayFriend(index?),
            "servers_network_worlds" => MenuAction::PlaySaved(index?),
            _ => return play_screen::featured_action(view, region),
        },
        "button.menu_network_server_item" | "button.connect_to_third_party_server" => {
            match collection {
                Some("servers_network_worlds") => MenuAction::PlaySaved(index?),
                _ => return play_screen::featured_action(view, region),
            }
        }
        "button.menu_network_server_world_edit" => MenuAction::EditSaved(index?),
        "button.description_read_toggle" => MenuAction::ToggleReadMore(0),
        "button.news_read_toggle" => MenuAction::ToggleReadMore(1),
        "button.menu_start_realms_world" => return play_screen::realm_action(view, region),
        "button.menu_start_local_world" => MenuAction::PlayLocalWorld(index?),
        _ => return None,
    })
}

fn toggle_action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    if let Some(action) = super::enhanced_setting::action(view, region) {
        return Some(action);
    }
    match region.control_name.as_deref()?.trim_start_matches('#') {
        "full_screen" if view.screen == MenuScreen::Settings => {
            Some(MenuAction::SettingsFullscreen(!view.fullscreen))
        }
        "navigation_tab" if view.screen == MenuScreen::Settings => Some(
            MenuAction::SettingsSection(u8::try_from(region.group_index?).ok()?),
        ),
        "navigation_tab" => Some(MenuAction::Navigate(match region.group_index? {
            0 => MenuScreen::Play,
            1 => MenuScreen::Social,
            _ => MenuScreen::Servers,
        })),
        "server_navigation_toggle" if region.key.contains("add_server") => {
            Some(MenuAction::PlayAddServer)
        }
        "server_navigation_toggle" if play_screen::is_featured(region) => {
            Some(MenuAction::SelectFeatured(region.collection_index?))
        }
        "server_navigation_toggle" => match region.collection.as_deref()? {
            "servers_network_worlds" => Some(MenuAction::PlaySaved(region.collection_index?)),
            _ => None,
        },
        _ => None,
    }
}

/// A settings slider's action per pointer segment, left to right: the slider
/// is split into one hit rect per value it snaps to.
pub(super) fn slider_actions(view: &MenuView, region: &HitRegion) -> Option<Vec<MenuAction>> {
    if region.kind != HitKind::Slider {
        return None;
    }
    let name = region.control_name.as_deref()?;
    if name == "gui_scale" {
        return Some(
            view.gui_scale_choices
                .iter()
                .map(|choice| MenuAction::SettingsScale(choice.offset))
                .collect(),
        );
    }
    super::settings_controls::slider_actions(&view.settings_options, region)
}

#[cfg(test)]
mod tests;
