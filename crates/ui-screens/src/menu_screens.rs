//! Which vanilla screen each menu state opens, the screen globals it binds from
//! the menu's view, and how the screen's pressed buttons map back to menu
//! actions. Screens bind strictly (an unbound visibility flag reads false, as
//! the vanilla screen controllers answer it), so each spec names the flags its
//! layout needs on.

use json_ui::{Context, DataSource, HitKind, HitRegion, Scalar};
use serde_json::Value;

use super::{menu_caret::with_caret, play_screen};
use launcher::menu::settings_sections::{SETTINGS_SECTIONS, SettingSection};
use launcher::menu::{MenuAction, MenuField, MenuScreen, MenuView, auth::AuthState};
use ui_engine::engine::screen::{Translate, translated};

/// Lang key the vanilla start and pause controllers give the unlock-full-game text.
const UNLOCK_FULL_GAME_TEXT: &str = "trial.pauseScreen.buyGame";

/// The retail desktop context for this build's platform. It is built once; clones share its
/// variables, so screens built over it compare their contexts by pointer.
pub fn retail_context() -> Context {
    static RETAIL: std::sync::LazyLock<Context> =
        std::sync::LazyLock::new(|| Context::retail(cfg!(target_os = "macos")));
    RETAIL.clone()
}

/// Vanilla start screen variables for a full-game, non-edu
/// account: demo, edu and unlock controls stay ignored.
fn start_screen_vars(context: Context) -> Context {
    unlock_text(context)
        .with_flag("unlock_full_game_button_ignored", true)
        .with_flag("featured_world_ignored", true)
        .with_flag("store_ignored", true)
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
pub struct MenuScreenData {
    pub reference: &'static str,
    pub context: Context,
    pub data: DataSource,
    /// A screen drawn over this one, which then takes all input (a resource-pack dialog).
    pub overlay: Option<Box<MenuScreenData>>,
}

pub(super) fn text(value: impl Into<String>) -> Scalar {
    Scalar::Text(value.into())
}

pub(super) fn flags(data: &mut DataSource, on: &[&str]) {
    for name in on {
        data.set_global(*name, Scalar::Bool(true));
    }
}

use launcher::menu::menu_reference;

/// The in-session screen for `view`, or `None` for states without one (the
/// programmatic launcher then draws them). `now_unix` is the wall-clock time in seconds.
pub fn screen_data(
    view: &MenuView,
    translate: Translate<'_>,
    now_unix: i64,
) -> Option<MenuScreenData> {
    if view.out_of_game() {
        return None;
    }
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
        let words = launcher::menu::disconnect::describe(error);
        data.set_global(
            "#title_text",
            text(translated(translate, words.title.0, words.title.1)),
        );
        let body = match words.body {
            launcher::menu::disconnect::DisconnectBody::Key(key) => translated(
                translate,
                key,
                launcher::menu::disconnect::body_fallback(key),
            ),
            launcher::menu::disconnect::DisconnectBody::Server(message) => message,
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
                data.set_global(
                    super::death_screen::REASON_BINDING,
                    text(&view.death_reason),
                );
                flags(
                    &mut data,
                    if view.death_loading {
                        &["#loading_message_visible"]
                    } else if view.death_controls_visible {
                        &[
                            "#buttons_and_deathmessage_visible",
                            "#respawn_visible",
                            "#respawn_enabled",
                            "#quit_visible",
                            "#quit_enabled",
                        ]
                    } else {
                        &[
                            "#buttons_and_deathmessage_visible",
                            "#respawn_enabled",
                            "#quit_enabled",
                        ]
                    },
                );
            }
            MenuScreen::Pause => {
                data.set_global(
                    "#playername",
                    text(launcher::menu::profile::current_name(view).to_owned()),
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
                context = unlock_text(context).with_flag("ignore_edu_pause", true);
            }
            MenuScreen::Home => {
                start_screen(view, &mut data, translate, now_unix);
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
                data.set_global("#should_show_subscription_tab", Scalar::Bool(false));
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
    progress: &launcher::local_worlds::Progress,
) {
    use launcher::local_worlds::Stage;
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

fn start_screen(view: &MenuView, data: &mut DataSource, translate: Translate<'_>, now_unix: i64) {
    let gamertag = launcher::menu::profile::current_name(view).to_owned();
    data.set_global("#playername", text(gamertag.clone()));
    data.set_global("#gamertag_label", text(gamertag));
    let portrait = launcher::menu::profile::current_picture(view).is_some()
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
    super::start_feed::bind(view, data, now_unix);
    data.set_global("#version", text(version_label(protocol::GAME_VERSION)));
    data.set_global("#unlock_full_game_button_text", text(UNLOCK_FULL_GAME_TEXT));
    data.set_global("#edu_demo_only_ui_visible", Scalar::Bool(false));
    // Keep the Realms and Dressing Room entries available.
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
        AuthState::AwaitingCode { .. } | AuthState::AwaitingXboxSignup { .. } => {}
    }
}

/// Formats the protocol game version for the start screen.
fn version_label(game_version: &str) -> String {
    format!(
        "v{}",
        game_version.strip_prefix("1.").unwrap_or(game_version)
    )
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
    let section = match view.settings.section {
        0 => SettingSection::Video.definition().index,
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
    let title = SettingSection::from_index(section)
        .unwrap_or(SettingSection::Video)
        .title();
    data.set_global("#section_title", text(translated(translate, title, title)));
    data.set_global(
        "#dialog_title",
        text(translated(translate, "menu.settings", "menu.settings")),
    );
}

#[cfg(any(test, feature = "test-support"))]
use json_ui::vanilla::Reference;

/// Supplies the production settings context for offline layout checks.
#[cfg(any(test, feature = "test-support"))]
pub fn settings_target() -> (&'static str, Context) {
    (
        Reference::SettingsScreenControlsAndSettings.as_str(),
        settings_context(base_context()),
    )
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

/// The menu action a pressed region means on `view`'s screen.
pub fn action_for(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
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
            MenuAction::LocalWorld(launcher::menu::LocalWorldAction::Back)
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
        "button.main_menu_button" if view.screen == MenuScreen::Death => MenuAction::OpenDeathQuit,
        "button.menu_quit" | "button.main_menu_button" => MenuAction::PauseDisconnect,
        "button.respawn_button" => MenuAction::Respawn,
        "button.gathering" => MenuAction::OpenLiveEvent,
        "button.menu_inbox" => MenuAction::Navigate(MenuScreen::Inbox),
        "button.friends_drawer" | "button.menu_friends" => {
            MenuAction::Navigate(MenuScreen::Friends)
        }
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
        "button.menu_cancel" if view.auth_state_awaiting_browser() => MenuAction::CancelSignIn,
        "button.menu_exit" if view.auth_state_awaiting_browser() => MenuAction::CancelSignIn,
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
pub fn slider_actions(view: &MenuView, region: &HitRegion) -> Option<Vec<MenuAction>> {
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
