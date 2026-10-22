//! Settings registry sections share the launcher’s persisted options and binding actions.

use super::super::theme::{self, BODY, CAPTION, EDGE, NEUTRAL80, TEXT, TEXT_DIMMER};
use super::super::widgets::{self, Interaction};
use super::{Content, button};
use launcher::menu::settings_options::SettingId;
use launcher::menu::settings_sections::SettingSection;
use launcher::menu::{
    MenuAction, MenuField,
    settings_options::{
        COMMAND_MACRO_COUNT, EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET,
        KEY_BINDINGS, SettingsGroup, SettingsOptions, custom_key_label, key_name,
        layout::SettingPanel, macro_message, macro_row, macro_row_index, macro_title,
    },
    settings_support::{SupportAction, SupportDialog, SupportLink},
};
use ui_engine::engine::screen::PaintError;

pub(super) fn draw(
    content: &mut Content<'_, '_>,
    section: SettingSection,
) -> Result<(), PaintError> {
    if super::services::draw(content, section)? {
        return Ok(());
    }
    match section {
        SettingSection::Accessibility => accessibility(content),
        SettingSection::Video => video(content),
        SettingSection::Shaders => super::renderer::shaders(content),
        SettingSection::Sound => audio(content),
        SettingSection::KeyboardAndMouse => keyboard(content),
        SettingSection::ControllerAndSwitch => controller(content),
        SettingSection::General => general(content),
        SettingSection::Updates => super::updates::draw(content),
        SettingSection::Creator => creator(content),
        SettingSection::Chat => chat(content),
        SettingSection::Discord => discord(content),
        SettingSection::Server => content.heading(
            section.title(),
            "Settings supplied by the connected server appear here when available.",
        ),
        _ => Ok(()),
    }
}

/// Draws the rows assigned to a panel by the settings registry.
fn options(content: &mut Content<'_, '_>, panel: SettingPanel) -> Result<(), PaintError> {
    for option in panel.options() {
        content.option(option.id)?;
    }
    Ok(())
}

fn accessibility(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading(
        "menu.accessibility.tab.title",
        "menu.accessibility.tab.description",
    )?;
    content.heading(
        "menu.accessibility.tab.tts.title",
        "menu.accessibility.tab.tts.description",
    )?;
    options(content, SettingPanel::AccessibilityTts)?;
    content.heading(
        "menu.accessibility.tab.gameplay.title",
        "menu.accessibility.tab.gameplay.description",
    )?;
    options(content, SettingPanel::AccessibilityGameplay)?;
    content.heading(
        "menu.accessibility.tab.ui.title",
        "menu.accessibility.tab.ui.description",
    )?;
    options(content, SettingPanel::AccessibilityUi)?;
    reset(
        content,
        SettingsGroup::Accessibility,
        "options.accessibility.resetSettings",
    )
}

fn video(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.video.tab.title", "menu.video.tab.description")?;
    super::presets::draw(content)?;
    options(content, SettingPanel::VideoBackend)?;
    content.heading(
        "menu.video.group.general",
        "menu.video.group.general.description",
    )?;
    options(content, SettingPanel::VideoGeneral)?;
    content.heading(
        "menu.video.group.performance",
        "menu.video.group.performance.description",
    )?;
    super::renderer::draw(content)?;
    options(content, SettingPanel::VideoPerformance)?;
    let graphics = if content.view.settings_options.value(SettingId::GraphicsMode) == 0 {
        "options.graphicsModeOptions.simple"
    } else {
        "options.graphicsModeOptions.fancy"
    };
    content.heading(graphics, "")?;
    {
        content.nested = true;
        options(content, SettingPanel::VideoGraphics)?;
        content.nested = false;
    }
    options(content, SettingPanel::VideoClouds)?;
    content.heading(
        "menu.video.group.customization",
        "menu.video.group.customization.description",
    )?;
    options(content, SettingPanel::VideoCustomization)?;
    content.heading(
        "menu.video.group.accessibility",
        "menu.video.group.accessibility.description",
    )?;
    options(content, SettingPanel::VideoAccessibility)?;
    if SettingPanel::VideoAdvanced.options().next().is_some() {
        content.heading("options.cinnabar.advanced", "")?;
        options(content, SettingPanel::VideoAdvanced)?;
    }
    reset(content, SettingsGroup::Video, "options.video.resetSettings")
}

fn audio(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.audio.tab.title", "menu.audio.tab.description")?;
    options(content, SettingPanel::Audio)?;
    reset(content, SettingsGroup::Audio, "options.sound.resetSettings")
}

fn reset(content: &mut Content<'_, '_>, group: SettingsGroup, key: &str) -> Result<(), PaintError> {
    super::services::action_row(
        content,
        key,
        &format!("{key}.description"),
        &format!("{key}.buttonLabel"),
        Some(MenuAction::SettingsResetGroup(group)),
    )
}

fn keyboard(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading(
        "menu.keyboardAndMouse.tab.title",
        "menu.keyboardAndMouse.tab.description",
    )?;
    options(content, SettingPanel::Keyboard)?;
    content.heading(
        "menu.keyboardAndMouse.tab.mappings.title",
        "menu.keyboardAndMouse.tab.mappings.description",
    )?;
    bindings(content, false)?;
    draw_macros(content)?;
    super::services::action_row(
        content,
        "options.keyboardAndMouse.resetMappings",
        "options.keyboardAndMouse.resetMappings.description",
        "options.keyboardAndMouse.resetMappings.buttonLabel",
        Some(MenuAction::SettingsResetBindings(false)),
    )
}

fn draw_macros(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading(
        "menu.keyboardAndMouse.tab.macros.title",
        "menu.keyboardAndMouse.tab.macros.description",
    )?;
    for slot in 0..COMMAND_MACRO_COUNT {
        draw_macro_slot(content, slot)?;
    }
    Ok(())
}

/// One macro slot: its title with the bound key, then the command box beneath.
pub(super) fn draw_macro_slot(
    content: &mut Content<'_, '_>,
    slot: usize,
) -> Result<(), PaintError> {
    draw_macro_binding(content, slot)?;
    draw_macro_command(content, slot)
}

fn draw_macro_binding(content: &mut Content<'_, '_>, slot: usize) -> Result<(), PaintError> {
    let capture = MenuAction::SettingsKey(macro_row_index(slot) as u16);
    let control = content.view.settings_options.macro_control(slot);
    let capturing = content
        .view
        .settings
        .key_remap
        .is_some_and(|index| macro_row(usize::from(index)) == Some(slot));
    let title = content.word(&macro_title(slot));
    let [left, right] = content.inset();
    let field_width = ((right - left) * 0.5).min(content.canvas.r(24.0));
    let reset_width = if control.is_some() && !capturing {
        content.canvas.r(5.2)
    } else {
        0.0
    };
    let label_width = (right - left - field_width - reset_width - content.canvas.r(0.8)).max(1.0);
    let bounds = content.row(
        &title,
        "",
        label_width,
        Some(capture),
        content.canvas.r(2.8),
    )?;
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
    let state = Interaction::of(content.view, Some(capture));
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
    } else {
        control.map_or_else(
            || content.word("controllerLayoutScreen.unassigned"),
            |control| content.view.settings_options.key_name(control),
        )
    };
    content
        .canvas
        .text_centred(&value, face, BODY, TEXT, false)?;
    content.canvas.hit(capture, field)?;
    if control.is_some() && !capturing {
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
            MenuAction::SettingsResetKey(macro_row_index(slot) as u16),
        )?;
    }
    Ok(())
}

fn draw_macro_command(content: &mut Content<'_, '_>, slot: usize) -> Result<(), PaintError> {
    let action = MenuAction::SettingsMacroCommand(slot);
    let command = content
        .view
        .macro_commands
        .as_ref()
        .map_or_else(
            || content.view.settings_options.macro_command(slot),
            |drafts| drafts[slot].as_str(),
        )
        .to_owned();
    let focused = content.view.field == Some(MenuField::MacroCommand(slot));
    let placeholder = content.word("options.commandMacro.placeholder");
    let [left, right] = content.inset();
    let error = (!command.is_empty() && macro_message(&command).is_err()).then(|| {
        format!(
            "Command is too long. Include the slash within the {}-byte chat limit.",
            ui::MAX_CHAT_INPUT_BYTES
        )
    });
    let error_height = if let Some(error) = &error {
        content
            .canvas
            .measure_height(error, right - left, CAPTION)?
            + content.canvas.r(0.4)
    } else {
        0.0
    };
    let bounds = content.row(
        "",
        "",
        right - left,
        Some(action),
        content.canvas.r(2.8) + error_height,
    )?;
    let middle = (bounds[1] + bounds[3] - error_height) * 0.5;
    let field = [
        left,
        middle - content.canvas.r(2.4),
        right,
        middle + content.canvas.r(2.4),
    ];
    let capture_focus = content.canvas.capture_focus;
    if error.is_some() {
        // Reveal the field and error together without enlarging pointer caret targets.
        content
            .canvas
            .focus_target(action, [field[0], bounds[1], field[2], bounds[3]])?;
        content.canvas.capture_focus = false;
    }
    let result = widgets::text_field_on_panel(
        content.canvas,
        content.view,
        field,
        &command,
        &placeholder,
        focused,
        Some(action),
    );
    content.canvas.capture_focus = capture_focus;
    result?;
    if let Some(error) = error {
        content.canvas.text(
            &error,
            [left, field[3] + content.canvas.r(0.4)],
            right - left,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
    }
    Ok(())
}

fn controller(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading(
        "menu.controller.tab.title",
        "menu.controller.tab.description",
    )?;
    options(content, SettingPanel::Controller)?;
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

fn bindings(content: &mut Content<'_, '_>, gamepad: bool) -> Result<(), PaintError> {
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

/// Draws one binding with its existing capture and reset actions.
pub(super) fn binding(
    content: &mut Content<'_, '_>,
    defaults: &SettingsOptions,
    index: usize,
    key: &str,
) -> Result<(), PaintError> {
    let action = MenuAction::SettingsKey(index as u16);
    let capturing = content.view.settings.key_remap == Some(index as u16);
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
            .map(key_name)
            .unwrap_or_else(|| content.word("controllerLayoutScreen.unassigned"))
    } else {
        let mut value = control
            .map(|control| content.view.settings_options.key_name(control))
            .unwrap_or_else(|| content.word("controllerLayoutScreen.unassigned"));
        if let Some(secondary) = content.view.settings_options.secondary_key_control(
            launcher::menu::settings_options::BindingId::from_name(key)
                .expect("registered binding row"),
        ) {
            value.push_str(" / ");
            value.push_str(&content.view.settings_options.key_name(secondary));
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

fn general(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.general.tab.title", "menu.general.tab.description")?;
    options(content, SettingPanel::General)?;
    content.heading(
        "menu.general.tab.pause.title",
        "menu.general.tab.pause.description",
    )?;
    options(content, SettingPanel::GeneralPause)?;
    content.heading(
        "menu.general.tab.network.title",
        "menu.general.tab.network.description",
    )?;
    options(content, SettingPanel::GeneralNetwork)?;
    content.heading(
        "menu.general.tab.miscellaneous.title",
        "menu.general.tab.miscellaneous.description",
    )?;
    options(content, SettingPanel::GeneralMisc)?;
    super::services::action_row(
        content,
        "Import from Minecraft",
        "Preview settings, servers and user-installed resource packs from a Minecraft profile.",
        "Import",
        (!content.view.over_world
            && !content.view.global_resources.busy
            && !content.view.global_resources.has_pending_changes())
        .then_some(MenuAction::MinecraftImport(
            launcher::minecraft_import::Action::Begin,
        )),
    )?;
    if content.view.minecraft_import.can_revert {
        super::services::action_row(
            content,
            "Revert Minecraft import",
            "Restore the settings, servers and pack order saved before the last import.",
            "Revert",
            (!content.view.over_world && !content.view.global_resources.busy).then_some(
                MenuAction::MinecraftImport(launcher::minecraft_import::Action::Revert),
            ),
        )?;
    }
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
    content.heading("options.cinnabar.advanced", "")?;
    options(content, SettingPanel::GeneralAdvanced)
}

fn creator(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.creator.tab.title", "menu.creator.tab.description")?;
    options(content, SettingPanel::CreatorCoordinates)?;
    for (title, panel) in [
        (
            "menu.creator.group.contentLog",
            SettingPanel::CreatorContentLog,
        ),
        (
            "menu.creator.group.scriptDebugger",
            SettingPanel::CreatorDebugger,
        ),
        (
            "menu.creator.group.scriptDiagnostic",
            SettingPanel::CreatorDiagnostic,
        ),
        (
            "menu.creator.group.scriptWatchdog",
            SettingPanel::CreatorWatchdog,
        ),
        ("menu.creator.group.deviceInfo", SettingPanel::CreatorDevice),
        ("menu.creator.group.editor", SettingPanel::CreatorEditor),
        (
            "menu.creator.group.textFiltering",
            SettingPanel::CreatorFiltering,
        ),
    ] {
        content.heading(title, "")?;
        options(content, panel)?;
    }
    Ok(())
}

/// Exposes the existing chat preferences and their shared reset route in the launcher.
fn chat(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading(
        SettingSection::Chat.title(),
        "options.chat.title.description",
    )?;
    options(content, SettingPanel::Chat)?;
    reset(content, SettingsGroup::Chat, "chat.settings.reset")
}

fn discord(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading(
        SettingSection::Discord.title(),
        "discord.settings.description",
    )?;
    options(content, SettingPanel::Discord)?;
    content.heading(
        "discord.settings.details.title",
        "discord.settings.details.description",
    )?;
    options(content, SettingPanel::DiscordDetails)?;
    reset(content, SettingsGroup::Discord, "discord.settings.reset")
}

pub(super) fn fallback(key: &str) -> &str {
    match key {
        "options.renderClouds" => "Render Clouds",
        "options.cinnabar.advanced" => "Advanced",
        "options.framerateLimit.description" => {
            "Choose an FPS limit or Unlimited. VSync and Variable Refresh Rate may still limit the frame rate."
        }
        "options.showExactServerPing" => "Show exact server ping",
        "options.oreuiDarkMode" => "Dark Mode",
        "options.oreuiDarkMode.description" => "Use dark surfaces for menus, settings and dialogs.",
        "options.estheticTab" | "options.estheticTab.name" => "Esthetic Tab",
        "options.estheticTab.description" => {
            "Display an aesthetic Lunar-style player list overlay with server MOTD and latency indicators."
        },
        "options.showExactServerPing.description" => {
            "Show server latency in milliseconds instead of Low, Medium or High ping."
        }
        "options.renderClouds.description" => "Show clouds in the sky.",
        "hbui.Settings.title" => "Settings",
        "hbui.Settings.category.controls" | "options.controls" | "options.group.input" => {
            "Controls"
        }
        "hbui.Settings.category.social" | "options.social" => "Social",
        "options.chat.title" | "chat.settings.title" => "Chat",
        "options.chat.title.description" => "Chat appearance, opacity, size and colors",
        "chat.settings.reset" => "Reset Chat Settings",
        "chat.settings.reset.description" => "Restore all chat options to default values",
        "chat.settings.reset.buttonLabel" => "Reset",
        "chat.settings.muteAll" => "Mute All Chat",
        "chat.settings.muteAll.description" => "Hide all incoming chat messages",
        "chat.settings.muteEmotes" => "Mute Emotes",
        "chat.settings.muteEmotes.description" => {
            "Saved preference; emote chat filtering is not available yet"
        }
        "chat.settings.tts" => "Text to Speech for Chat",
        "chat.settings.tts.description" => "Saved preference; chat narration is not available yet",
        "chat.settings.fontSize" => "Font Size",
        "chat.settings.fontSize.description" => "Adjust the scale of chat text",
        "chat.settings.lineSpacing" => "Line Spacing",
        "chat.settings.lineSpacing.description" => "Set the spacing between lines in chat",
        "chat.settings.chatColor" => "Chat Color",
        "chat.settings.chatColor.description" => "Default color for chat text",
        "chat.settings.mentionsColor" => "Mentions Color",
        "chat.settings.mentionsColor.description" => {
            "Saved preference; mention highlighting is not available yet"
        }
        "options.chatBackgroundOpacity" => "Chat Background Opacity",
        "options.chatBackgroundOpacity.description" => {
            "Adjust the opacity of the chat background box"
        }
        "options.notificationDuration.Chat" => "Chat Notification Duration",
        "options.notificationDuration.Chat.description" => {
            "How long chat notifications remain visible on the HUD"
        }
        "Discord" | "discord.settings.title" => "Discord",
        "discord.settings.description" => {
            "Control what Cinnabar shares with the Discord desktop app"
        }
        "discord.settings.enabled" => "Discord Rich Presence",
        "discord.settings.enabled.description" => {
            "Publish your current Cinnabar activity to the Discord desktop app"
        }
        "discord.settings.details.title" => "Activity details",
        "discord.settings.details.description" => {
            "Choose which optional information the Discord activity card can show"
        }
        "discord.settings.serverDetails" => "Show server and world details",
        "discord.settings.serverDetails.description" => {
            "Display the server address or local world name in the activity card"
        }
        "discord.settings.partySize" => "Show party size",
        "discord.settings.partySize.description" => {
            "Display the current and maximum player count when available"
        }
        "discord.settings.allowJoins" => "Allow Discord joins",
        "discord.settings.allowJoins.description" => {
            "Let Discord friends join through the activity card and send join requests"
        }
        "discord.settings.timestamp" => "Show elapsed time",
        "discord.settings.timestamp.description" => {
            "Show how long you have been in the current activity"
        }
        "discord.settings.reset" => "Reset Discord Settings",
        "discord.settings.reset.description" => "Restore all Discord options to default values",
        "discord.settings.reset.buttonLabel" => "Reset",
        "color.white" => "White",
        "color.green" => "Green",
        "color.aqua" => "Aqua",
        "color.red" => "Red",
        "color.light_purple" => "Light Purple",
        "color.yellow" => "Yellow",
        "color.gold" => "Gold",
        "Chat Position" => "Chat Position",
        "Chat Position.description" => "Position the chat HUD at the bottom or top of the screen",
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
        "menu.keyboardAndMouse.tab.macros.title" => "Macros",
        "menu.keyboardAndMouse.tab.macros.description" => {
            "Run a command with its bound control and the Macro Modifier selected above. Choose None to use the control alone."
        }
        "options.commandMacro.placeholder" => "Command to run",
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
        "menu.account.realmMembershipInvites.buttonLabel" => "Manage",
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
        "options.cinnabar.worldsFolder" => "Worlds folder",
        "options.cinnabar.worldsFolder.description" => "Where your worlds are saved",
        "options.cinnabar.resourcePacksFolder" => "Resource packs folder",
        "options.cinnabar.resourcePacksFolder.description" => {
            "Where your imported resource packs are stored"
        }
        "options.cinnabar.shaderPacksFolder" => "Shader packs folder",
        "options.cinnabar.shaderPacksFolder.description" => {
            "Add Java shader packs here, or drop them onto this window"
        }
        "options.cinnabar.openFolder.buttonLabel" => "Open folder",
        "options.cinnabar.shaderOptions" => "Shader pack settings",
        "options.cinnabar.shaderOptions.description" => {
            "The selected pack's own options. Applying them reloads the pack."
        }
        "options.cinnabar.shaderOptions.buttonLabel" => "Shader options…",
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
        _ => custom_key_label(key).unwrap_or(key),
    }
}
