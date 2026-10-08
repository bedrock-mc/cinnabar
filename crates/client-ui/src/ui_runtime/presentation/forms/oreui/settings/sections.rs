//! Settings registry sections share the launcher’s persisted options and binding actions.

use super::super::super::super::UiPresentationError;
use super::super::theme::{self, BODY, EDGE, NEUTRAL80, TEXT};
use super::super::widgets::Interaction;
use super::{Content, button};
use crate::menu::{
    MenuAction,
    settings_options::{
        ANIMATIONS_OPTION, CHAT_POSITION_OPTION, EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS,
        GAMEPAD_OFFSET, INVERT_CROSSHAIR_OPTION, KEY_BINDINGS, MOTION_BLUR_OPTION, SettingsGroup,
        SettingsOptions, THIRD_PERSON_CROSSHAIR_OPTION, VOLUME_SETTINGS, key_name,
    },
    settings_support::{SupportAction, SupportDialog, SupportLink},
};

pub(super) fn draw(
    content: &mut Content<'_, '_>,
    section: &str,
) -> Result<(), UiPresentationError> {
    if super::services::draw(content, section)? {
        return Ok(());
    }
    match section {
        "accessibility_forced_index" => accessibility(content),
        "video_forced_index" => video(content),
        "sound_forced_index" => audio(content),
        "keyboard_and_mouse_forced_index" => keyboard(content),
        "controller_and_switch_forced_index" => controller(content),
        "general_forced_index" => general(content),
        "creator_forced_index" => creator(content),
        _ => Ok(()),
    }
}

fn options(content: &mut Content<'_, '_>, names: &[&str]) -> Result<(), UiPresentationError> {
    for name in names {
        content.option(name)?;
    }
    Ok(())
}

fn accessibility(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading(
        "menu.accessibility.tab.title",
        "menu.accessibility.tab.description",
    )?;
    content.heading(
        "menu.accessibility.tab.tts.title",
        "menu.accessibility.tab.tts.description",
    )?;
    options(
        content,
        &[
            "enable_ui_text_to_speech",
            "enable_chat_text_to_speech",
            "texttospeech_volume",
            "enable_open_chat_message",
        ],
    )?;
    content.heading(
        "menu.accessibility.tab.gameplay.title",
        "menu.accessibility.tab.gameplay.description",
    )?;
    options(
        content,
        &[
            "enable_gameplay_subtitles",
            "camera_shake",
            "hide_endflash",
            "enable_dithering_blocks",
            "enable_dithering_mobs",
            "darkness",
            "screen_distortion",
            "glint_strength",
            "glint_speed",
        ],
    )?;
    content.heading(
        "menu.accessibility.tab.ui.title",
        "menu.accessibility.tab.ui.description",
    )?;
    options(
        content,
        &[
            "actionbar_text_background_opacity",
            "chat_background_opacity",
            "hud_text_background_opacity",
            "chat_message_duration",
            "toast_notification_duration",
            "gui_scale",
            "gui_accessibility_scaling",
        ],
    )?;
    reset(
        content,
        SettingsGroup::Accessibility,
        "options.accessibility.resetSettings",
    )
}

fn video(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading("menu.video.tab.title", "menu.video.tab.description")?;
    content.heading(
        "menu.video.group.general",
        "menu.video.group.general.description",
    )?;
    options(content, &["field_of_view", "third_person"])?;
    content.option(CHAT_POSITION_OPTION.name)?;
    content.heading(
        "menu.video.group.performance",
        "menu.video.group.performance.description",
    )?;
    content.option("graphics_mode")?;
    let graphics = if content.view.settings_options.value("graphics_mode") == 0 {
        "options.graphicsModeOptions.simple"
    } else {
        "options.graphicsModeOptions.fancy"
    };
    content.heading(graphics, "")?;
    {
        content.nested = true;
        options(
            content,
            &[
                "gamma",
                "max_framerate",
                "vsync",
                MOTION_BLUR_OPTION.name,
                "smooth_lighting",
                "fancy_skies",
                "render_distance",
            ],
        )?;
        content.nested = false;
    }
    options(
        content,
        &["render_clouds", "transparent_leaves", "bubble_particles"],
    )?;
    content.heading(
        "menu.video.group.customization",
        "menu.video.group.customization.description",
    )?;
    options(
        content,
        &[
            "full_screen",
            "hide_hand",
            "hide_paperdoll",
            "hide_hud",
            THIRD_PERSON_CROSSHAIR_OPTION.name,
            INVERT_CROSSHAIR_OPTION.name,
            "classic_box_selection",
            "ingame_player_names",
            "interface_opacity",
            "show_auto_save_icon",
            crate::menu::settings_options::SHOW_EXACT_SERVER_PING,
            crate::menu::settings_options::OREUI_DARK_MODE,
        ],
    )?;
    content.heading(
        "menu.video.group.accessibility",
        "menu.video.group.accessibility.description",
    )?;
    options(
        content,
        &[
            "gui_scale",
            "gui_accessibility_scaling",
            "screen_animations",
            "panorama_speed",
            "view_bobbing",
            ANIMATIONS_OPTION.name,
            "damage_bob",
            "camera_shake",
            "field_of_view_toggle",
        ],
    )?;
    reset(content, SettingsGroup::Video, "options.video.resetSettings")
}

fn audio(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading("menu.audio.tab.title", "menu.audio.tab.description")?;
    options(content, &VOLUME_SETTINGS)?;
    reset(content, SettingsGroup::Audio, "options.sound.resetSettings")
}

fn reset(
    content: &mut Content<'_, '_>,
    group: SettingsGroup,
    key: &str,
) -> Result<(), UiPresentationError> {
    super::services::action_row(
        content,
        key,
        &format!("{key}.description"),
        &format!("{key}.buttonLabel"),
        Some(MenuAction::SettingsResetGroup(group)),
    )
}

fn keyboard(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading(
        "menu.keyboardAndMouse.tab.title",
        "menu.keyboardAndMouse.tab.description",
    )?;
    options(
        content,
        &[
            "keyboard_mouse_sensitivity",
            "spyglass_mouse_dampening",
            "keyboard_mouse_invert_y_axis",
            "keyboard_mouse_autojump",
            "always_sprint",
            "keyboard_show_full_keyboard_options",
        ],
    )?;
    content.heading(
        "menu.keyboardAndMouse.tab.mappings.title",
        "menu.keyboardAndMouse.tab.mappings.description",
    )?;
    bindings(content, false)?;
    super::services::action_row(
        content,
        "options.keyboardAndMouse.resetMappings",
        "options.keyboardAndMouse.resetMappings.description",
        "options.keyboardAndMouse.resetMappings.buttonLabel",
        Some(MenuAction::SettingsResetBindings(false)),
    )
}

fn controller(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading(
        "menu.controller.tab.title",
        "menu.controller.tab.description",
    )?;
    options(
        content,
        &[
            "controller_sensitivity",
            "spyglass_gamepad_dampening",
            "controller_invert_y_axis",
            "controller_autojump",
            "hide_tooltips",
            "hide_gamepad_cursor",
            "controller_clear_hotbar",
            "swap_gamepad_ab_buttons",
            "swap_gamepad_xy_buttons",
        ],
    )?;
    content.heading(
        "menu.controller.tab.mappings.title",
        "menu.controller.tab.mappings.description",
    )?;
    bindings(content, true)?;
    super::services::action_row(
        content,
        "options.controller.resetMappings",
        "options.controller.resetMappings.description",
        "options.controller.resetMappings.buttonLabel",
        Some(MenuAction::SettingsResetBindings(true)),
    )
}

fn bindings(content: &mut Content<'_, '_>, gamepad: bool) -> Result<(), UiPresentationError> {
    let defaults = SettingsOptions::default();
    if gamepad {
        for (index, label) in GAMEPAD_BINDINGS
            .iter()
            .map(|(_, label)| *label)
            .chain(EXTRA_GAMEPAD.iter().map(|(label, _)| *label))
            .enumerate()
        {
            binding(content, &defaults, GAMEPAD_OFFSET + index, label)?;
        }
    } else {
        for (index, label) in KEY_BINDINGS
            .iter()
            .map(|(_, label)| *label)
            .chain(EXTRA_KEYS.iter().map(|(label, _)| *label))
            .enumerate()
        {
            binding(content, &defaults, index, label)?;
        }
    }
    Ok(())
}

fn binding(
    content: &mut Content<'_, '_>,
    defaults: &SettingsOptions,
    index: usize,
    key: &str,
) -> Result<(), UiPresentationError> {
    let action = MenuAction::SettingsKey(index as u16);
    let capturing = content.view.key_remap == Some(index as u16);
    let control = content.view.settings_options.key_control(index);
    let changed = control != defaults.key_control(index);
    let title = content.word(key);
    let [left, right] = content.inset();
    let field_width = ((right - left) * 0.5).min(content.canvas.r(24.0));
    let reset_width = if changed && !capturing {
        content.canvas.r(5.2)
    } else {
        0.0
    };
    let label_width = (right - left - field_width - reset_width - content.canvas.r(0.8)).max(1.0);
    let bounds = content.row(&title, "", label_width, Some(action), content.canvas.r(2.8))?;
    let middle = (bounds[1] + bounds[3]) * 0.5;
    content.canvas.text(
        &title,
        [left, middle - content.canvas.r(1.0)],
        label_width,
        BODY,
        TEXT,
        false,
    )?;
    let field = [
        right - field_width,
        middle - content.canvas.r(2.4),
        right,
        middle + content.canvas.r(2.4),
    ];
    let state = Interaction::of(content.view, Some(action));
    content.canvas.fill(field, theme::BORDER)?;
    let edge = content.canvas.r(EDGE);
    let face = [
        field[0] + edge,
        field[1] + edge,
        field[2] - edge,
        field[3] - edge,
    ];
    content.canvas.fill(
        face,
        if state.hovered {
            NEUTRAL80.hovered
        } else {
            NEUTRAL80.fill
        },
    )?;
    content.canvas.fill(
        [face[0], face[1], face[2], face[1] + content.canvas.r(0.4)],
        NEUTRAL80.shadow,
    )?;
    if state.focused || capturing {
        content.canvas.frame(field, EDGE, theme::OUTLINE)?;
    }
    let value = if capturing {
        "...".to_owned()
    } else if index >= GAMEPAD_OFFSET {
        control
            .map(gamepad_name)
            .unwrap_or_else(|| content.word("controllerLayoutScreen.unassigned"))
    } else {
        let mut value = control
            .map(key_name)
            .unwrap_or_else(|| content.word("controllerLayoutScreen.unassigned"));
        if let Some(secondary) = content.view.settings_options.secondary_key_control(key) {
            value.push_str(" / ");
            value.push_str(&key_name(secondary));
        }
        value
    };
    content
        .canvas
        .text_centred(&value, face, BODY, TEXT, false)?;
    content.canvas.hit(action, field)?;
    if changed && !capturing {
        let reset = [
            field[0] - content.canvas.r(5.2),
            middle - content.canvas.r(2.0),
            field[0] - content.canvas.r(0.8),
            middle + content.canvas.r(2.4),
        ];
        button::binding_reset(
            content.canvas,
            content.view,
            reset,
            MenuAction::SettingsResetKey(index as u16),
        )?;
    }
    Ok(())
}

fn gamepad_name(control: semantic_input::PhysicalControl) -> String {
    use semantic_input::PhysicalControl;
    match control {
        PhysicalControl::GamepadButton(0) => "A".to_owned(),
        PhysicalControl::GamepadButton(1) => "B".to_owned(),
        PhysicalControl::GamepadButton(2) => "Y".to_owned(),
        PhysicalControl::GamepadButton(3) => "X".to_owned(),
        PhysicalControl::GamepadButton(4) => "LB".to_owned(),
        PhysicalControl::GamepadButton(5) => "RB".to_owned(),
        PhysicalControl::GamepadButton(8) => "LS".to_owned(),
        PhysicalControl::GamepadButton(9) => "RS".to_owned(),
        PhysicalControl::GamepadButton(11) => "D-pad Up".to_owned(),
        PhysicalControl::GamepadButton(12) => "D-pad Down".to_owned(),
        PhysicalControl::GamepadButton(13) => "D-pad Left".to_owned(),
        PhysicalControl::GamepadButton(14) => "D-pad Right".to_owned(),
        PhysicalControl::GamepadAxis { axis: 4, .. } => "LT".to_owned(),
        PhysicalControl::GamepadAxis { axis: 5, .. } => "RT".to_owned(),
        _ => key_name(control),
    }
}

fn general(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading("menu.general.tab.title", "menu.general.tab.description")?;
    options(content, &["only_trusted_skins_allowed", "filter_profanity"])?;
    content.heading(
        "menu.general.tab.pause.title",
        "menu.general.tab.pause.description",
    )?;
    options(
        content,
        &["pause_option_toggle", "pause_menu_on_focus_lost"],
    )?;
    content.heading(
        "menu.general.tab.network.title",
        "menu.general.tab.network.description",
    )?;
    options(
        content,
        &[
            "websockets_enabled",
            "websocket_encryption",
            "auto_update_enabled",
        ],
    )?;
    content.heading(
        "menu.general.tab.miscellaneous.title",
        "menu.general.tab.miscellaneous.description",
    )?;
    content.option("ecomode_toggle")?;
    for (key, action) in [
        ("options.helpCenter", SupportAction::Open(SupportLink::Help)),
        (
            "options.attribution",
            SupportAction::Open(SupportLink::Attribution),
        ),
        (
            "options.licensed_content",
            SupportAction::Open(SupportLink::LicensedContent),
        ),
        (
            "options.font_license",
            SupportAction::Dialog(SupportDialog::FontLicense),
        ),
    ] {
        super::services::action_row(
            content,
            key,
            &format!("{key}.description"),
            &format!("{key}.buttonLabel"),
            Some(MenuAction::SettingsSupport(action)),
        )?;
    }
    Ok(())
}

fn creator(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading("menu.creator.tab.title", "menu.creator.tab.description")?;
    content.option("copy_coordinate_ui")?;
    for (title, names) in [
        (
            "menu.creator.group.contentLog",
            &[
                "content_log_file",
                "content_log_gui",
                "content_log_gui_show_on_errors",
                "content_log_gui_level",
            ][..],
        ),
        (
            "menu.creator.group.scriptDebugger",
            &[
                "script_debugger_passcode_required",
                "script_debugger_auto_attach",
            ][..],
        ),
        (
            "menu.creator.group.scriptDiagnostic",
            &["serverbound_client_diagnostics_enabled"][..],
        ),
        (
            "menu.creator.group.scriptWatchdog",
            &[
                "script_watchdog_spike_warning",
                "script_watchdog_slow_warning",
            ][..],
        ),
        (
            "menu.creator.group.deviceInfo",
            &["device_info_use_memory_tier_override"][..],
        ),
        (
            "menu.creator.group.editor",
            &["editor_collect_network_metrics"][..],
        ),
        (
            "menu.creator.group.textFiltering",
            &["debug_text_filtering_use_delay_sec_override"][..],
        ),
    ] {
        content.heading(title, "")?;
        options(content, names)?;
    }
    Ok(())
}

pub(super) fn option_label(name: &str, default: &'static str) -> &'static str {
    match name {
        "controller_sensitivity" => "options.controller.sensitivity.name",
        "keyboard_mouse_sensitivity" => "options.sensitivity.Mouse.name",
        "spyglass_gamepad_dampening" | "spyglass_mouse_dampening" => "options.spyglassdampen.name",
        "field_of_view" => "options.fov.name",
        "field_of_view_toggle" => "options.fov.toggle.name",
        "show_auto_save_icon" => "options.showautosaveicon.name",
        "ingame_player_names" => "options.ingamePlayerNames.name",
        "keyboard_mouse_invert_y_axis" | "controller_invert_y_axis" => "options.invertYAxis.name",
        "keyboard_mouse_autojump" | "controller_autojump" => "options.autojump.name",
        "keyboard_show_full_keyboard_options" => "options.fullKeyboardGameplay.name",
        "websockets_enabled" => "options.websocketsEnabled.name",
        "auto_update_enabled" => "options.autoUpdateEnabled.name",
        "only_trusted_skins_allowed" => "options.onlyTrustedSkinsAllowed.name",
        "filter_profanity" => "options.filterProfanity.name",
        "ecomode_toggle" => "options.enableEcoMode.name",
        "copy_coordinate_ui" => "options.copyCoordinateUI.name",
        "content_log_gui_show_on_errors" => "options.content_log_gui_show_on_errors.name",
        "gui_accessibility_scaling" => "options.guiAccessibilityScaling.name",
        "graphics_mode" => "menu.video.mode",
        "render_clouds" => "options.renderClouds",
        _ => default,
    }
}

pub(super) fn fallback(key: &str) -> &str {
    match key {
        "options.renderClouds" => "Render Clouds",
        "options.showExactServerPing" => "Show exact server ping",
        "options.oreuiDarkMode" => "Dark Mode",
        "options.oreuiDarkMode.description" => "Use dark surfaces for menus, settings and dialogs.",
        "options.showExactServerPing.description" => {
            "Show server latency in milliseconds instead of Low, Medium or High ping."
        }
        "options.renderClouds.description" => "Show clouds in the sky.",
        "hbui.Settings.title" => "Settings",
        "hbui.Settings.category.controls" | "options.controls" | "options.group.input" => {
            "Controls"
        }
        "hbui.Settings.category.social" | "options.social" => "Social",
        "hbui.Settings.category.general" => "General",
        "menu.accessibility.tab.title" | "options.accessibility" => "Accessibility",
        "menu.accessibility.tab.description" => "Minecraft is for everyone, including you",
        "menu.accessibility.tab.tts.title" => "Text to Speech",
        "menu.accessibility.tab.tts.description" => "Adjust how you hear on-screen text",
        "menu.accessibility.tab.gameplay.title" => "Gameplay",
        "menu.accessibility.tab.gameplay.description" => {
            "Change accessibility options for in-game visuals and camera movements"
        }
        "menu.accessibility.tab.ui.title" => "User Interface",
        "menu.accessibility.tab.ui.description" => "Set the game’s UI to fit your playing style",
        "menu.keyboardAndMouse.tab.title" | "options.keyboardAndMouse" => "Keyboard & Mouse",
        "menu.keyboardAndMouse.tab.description" => "Adjust input and mapping options",
        "menu.keyboardAndMouse.tab.mappings.title" => "Keyboard & Mouse Mappings",
        "menu.keyboardAndMouse.tab.mappings.description" => {
            "Remap any and all keyboard & mouse buttons to play your way"
        }
        "menu.controller.tab.title" | "options.controller" => "Controller",
        "menu.controller.tab.description" => {
            "Input options, sensitivity settings and button mapping for controllers"
        }
        "menu.controller.tab.mappings.title" => "Button mapping",
        "menu.controller.tab.mappings.description" => {
            "Remap any and all controller buttons to play your way"
        }
        "menu.general.tab.title" | "options.general" => "General",
        "menu.general.tab.description" => "A bunch of stuff, from network settings to game credits",
        "menu.general.tab.pause.title" => "Pause",
        "menu.general.tab.pause.description" => "Take a break while playing on your own",
        "menu.general.tab.network.title" => "Network Settings",
        "menu.general.tab.network.description" => {
            "Adjust the online experience on a technical and social level"
        }
        "menu.general.tab.miscellaneous.title" => "Miscellaneous",
        "menu.general.tab.miscellaneous.description" => {
            "We didn’t know where else to put these settings"
        }
        "menu.general.tab.tutorial.title" => "Tutorial",
        "menu.general.tab.tutorial.description" => "Help learning the game",
        "menu.creator.tab.title" | "options.creator" => "Creator",
        "menu.creator.tab.description" => "Adjust logs, debuggers, diagnostics, and more",
        "menu.creator.group.contentLog" => "Content log settings",
        "menu.creator.group.scriptDebugger" => "Script debugger settings",
        "menu.creator.group.scriptDiagnostic" => "Script diagnostics settings",
        "menu.creator.group.scriptWatchdog" => "Script watchdog settings",
        "menu.creator.group.deviceInfo" => "Device info settings",
        "menu.creator.group.editor" => "Editor settings",
        "menu.creator.group.textFiltering" => "Text Filtering",
        "menu.video.tab.title" | "options.video" => "Video",
        "menu.video.tab.description" => "Adjust graphics and visual quality",
        "menu.video.group.general" => "General",
        "menu.video.group.general.description" => "Common video settings",
        "menu.video.group.performance" => "Graphics, performance and layout",
        "menu.video.group.performance.description" => "Control how Minecraft looks and runs",
        "menu.video.group.customization" => "View customization",
        "menu.video.group.customization.description" => {
            "Build, stream, or take screenshots without distractions"
        }
        "menu.video.group.accessibility" => "Accessibility: Video",
        "menu.video.group.accessibility.description" => {
            "Controls to make Minecraft more comfortable to play"
        }
        "menu.video.mode" => "Mode",
        "options.graphicsModeOptions.fancy" => "Fancy Graphics Options",
        "options.graphicsModeOptions.simple" => "Simple Graphics Options",
        "menu.audio.tab.title" | "options.audio" => "Audio",
        "menu.audio.tab.description" => "Adjust volume settings for music and sound effects",
        "menu.account.tab.title" | "options.account" => "Account",
        "menu.account.tab.description" => "Access your account information",
        "menu.account.gamertag.title" => "Gamertag",
        "menu.account.changeGamertag.title" => "Change Gamertag",
        "menu.account.manageAccount.title" => "Manage Account",
        "menu.account.privacyAndSafety.title" => "Privacy & online safety",
        "menu.account.realmMembershipInvites.title" => "Manage Realms membership invites",
        "menu.account.signOutOfMicrosoft.title" => "Sign out of your Microsoft account",
        "menu.account.signIn.title" => "Sign in for free",
        "menu.account.changeGamertag.buttonLabel"
        | "menu.account.manageAccount.buttonLabel"
        | "menu.account.privacyAndSafety.buttonLabel" => "Open",
        "menu.account.realmMembershipInvites.buttonLabel"
        | "options.viewSubscriptions.button.manage" => "Manage",
        "menu.account.signOutOfMicrosoft.buttonLabel" => "Sign Out",
        "menu.account.signIn.buttonLabel" => "Sign In",
        "menu.account.clearSignInData.buttonLabel" => "Clear data",
        "menu.language.tab.title" | "options.language" => "Language",
        "menu.language.tab.description" => "Select your preferred language for Minecraft",
        "menu.storage.tab.title" | "options.storage" => "Storage",
        "menu.storage.tab.description" => {
            "Manage worlds, world templates, resource packs, behavior packs and cached data"
        }
        "hbui.Settings.storage.worlds.title" => "Worlds",
        "hbui.Settings.storage.worldTemplates.title" => "World templates",
        "hbui.Settings.storage.resourcePacks.title" => "Resource packs",
        "hbui.Settings.storage.behaviorPacks.title" => "Behavior packs",
        "storageManager.contentType.skinPacks" => "Skin Packs",
        "hbui.Settings.storage.cachedData.title" => "Cached data",
        "hbui.Settings.storage.emptyList.title" => "No %1$s found",
        "hbui.Settings.storage.listHeader.delete" => "Delete",
        "options.dev_clearAllCache" => "Clear All Cache",
        "options.dev_clearDownloadeCache.name" => "Clear download cache",
        "options.dev_deleteLocalScreenshots" => "Delete Local Screenshots",
        "playscreen.fileSize.GB" => "GB",
        "playscreen.fileSize.MB" => "MB",
        "menu.touch.tab.title" | "options.touch" => "Touch",
        "menu.touch.tab.description" => {
            "Input options, sensitivity settings and button mapping for touch screens"
        }
        "hudScreen.controlCustomization.tooltip.notouch" => {
            "Use a touch device to customize controls"
        }
        "menu.party.tab.title" | "options.party" => "Party",
        "options.partyInviteReceivedFilter" => "Receive party invites from",
        "options.partyPrivacy" => "Default party privacy",
        "options.partyInviteSendPrivileges" => "Who can send party invites in your party",
        "options.subscriptions" | "options.viewSubscriptions" => "Subscriptions",
        "options.viewSubscriptions.mySubscriptions" => "My Subscriptions",
        "options.viewSubscriptions.realmsServer" => "Realms Server",
        "options.viewSubscriptions.signIn" => "Sign in",
        "options.globalResources" | "menu.globalpacks" | "options.resourcepacks" => {
            "Global Resources"
        }
        "resourcePack.selected.title.packs" => "Active",
        "resourcePack.available.title.packs" => "My Packs",
        "resourcePack.message.noneFound.packs" => "There are no available packs on this device.",
        "resourcePack.selected.remove" => "Deactivate",
        "resourcePack.available.add" => "Activate",
        "resourcePack.description.bottom.global" => {
            "Resource packs are applied bottom to top. This means any asset that is in two packs will be overridden by the higher pack. Packs in your worlds will apply on top of these global packs. These resources are just for you. No one else will see the resources you set here. Resource Packs in your worlds or worlds you join will apply on top of these global resources."
        }
        "menu.settings" => "Settings",
        "gui.select" => "Select",
        "gui.clear" => "Clear",
        "gui.back" => "Back",
        "options.helpCenter" => "Help Center",
        "options.helpCenter.description" => "Get answers to all of your burning support queries",
        "options.attribution" => "Attribution",
        "options.attribution.description" => "Technology partners and systems",
        "options.licensed_content" => "Licensed Content",
        "options.licensed_content.description" => "All licensed content available in Minecraft",
        "options.font_license" => "Font License",
        "options.font_license.description" => "A license for third-party fonts",
        "options.helpCenter.buttonLabel"
        | "options.attribution.buttonLabel"
        | "options.licensed_content.buttonLabel" => "Open",
        "options.font_license.buttonLabel" => "View",
        "options.guiScale.optionName.name" => "UI scale modifier",
        "options.guiScale.disabled" => {
            "UI scaling is unsupported on this monitor, most likely because of a low resolution."
        }
        "options.renderDistanceFormat" => "%s chunks",
        "options.framerateLimit.max" => "Unlimited",
        "options.guiScale.optionName.description" => {
            "Select the size of the HUD and other UI elements to make them easier to use on different devices"
        }
        "options.guiAccessibilityScaling.name" => "Extra large new UI",
        "options.guiAccessibilityScaling.description" => {
            "Increase the size of the UI for clarity and easy reading"
        }
        "options.keyboardAndMouse.resetMappings" => "Reset to default mapping",
        "options.keyboardAndMouse.resetMappings.description" => {
            "Restore all keyboard & mouse button mappings to their original values"
        }
        "options.controller.resetMappings" => "Reset mapping to default",
        "options.controller.resetMappings.description" => {
            "Restore all controller button mappings to their original values"
        }
        "options.accessibility.resetSettings"
        | "options.video.resetSettings"
        | "options.sound.resetSettings" => "Reset to default",
        "options.accessibility.resetSettings.description" => {
            "Return all accessibility settings and sliders to their original values"
        }
        "options.video.resetSettings.description" => {
            "Restore all video settings to their original values"
        }
        "options.sound.resetSettings.description" => "Set all options to their original values",
        "options.keyboardAndMouse.resetMappings.buttonLabel"
        | "options.controller.resetMappings.buttonLabel"
        | "options.accessibility.resetSettings.buttonLabel"
        | "options.video.resetSettings.buttonLabel"
        | "options.sound.resetSettings.buttonLabel"
        | "options.key.reset.buttonLabel" => "Reset",
        "controllerLayoutScreen.unassigned" => "Unassigned",
        "key.freelook" => "Freelook",
        _ => key,
    }
}
