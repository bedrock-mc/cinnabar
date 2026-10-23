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
        KEY_BINDINGS, SettingsGroup, SettingsOptions, key_name, layout::SettingPanel,
        macro_message, macro_row, macro_row_index, macro_title,
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
    // Vanilla's tab description lists content-log and script tools the client does not have.
    content.heading("menu.creator.tab.title", "")?;
    options(content, SettingPanel::CreatorCoordinates)
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
