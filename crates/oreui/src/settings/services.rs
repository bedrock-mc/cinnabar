//! Settings panels backed by launcher account, language, pack and storage state.

use super::super::theme::{BODY, CAPTION, EDGE, NEUTRAL, TEXT, TEXT_DIMMER};
use super::super::widgets;
use super::{Content, button};
use launcher::menu::settings_sections::SettingSection;
use launcher::menu::{
    MenuAction,
    auth::AuthState,
    settings_storage::{CATEGORIES, StorageAction, StorageFolder},
    settings_support::{SupportAction, SupportLink},
};
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

pub(super) fn draw(
    content: &mut Content<'_, '_>,
    section: SettingSection,
) -> Result<bool, PaintError> {
    match section {
        SettingSection::Language => language(content)?,
        SettingSection::Account => account(content)?,
        SettingSection::GlobalTexturePack => resources(content)?,
        SettingSection::Mods => super::mods::draw(content)?,
        SettingSection::StorageManagement => storage(content)?,
        SettingSection::Touch => touch(content)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn language(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.language.tab.title", "menu.language.tab.description")?;
    let view = content.view;
    let selected = view.settings_options.language();
    for (index, (code, name)) in view.language_choices.iter().enumerate() {
        let Ok(index_action) = u16::try_from(index) else {
            continue;
        };
        let action = MenuAction::SettingsLanguage(index_action);
        let checked = selected.map_or(index == 0, |selected| selected == code);
        let height = content.canvas.r(5.6);
        let bounds = [
            content.span[0],
            content.y,
            content.span[1],
            content.y + height,
        ];
        let state = content.canvas.interaction(view, Some(action));
        content.canvas.fill(bounds, NEUTRAL.fill)?;
        let centre = [
            bounds[0] + content.canvas.r(3.6),
            (bounds[1] + bounds[3]) * 0.5,
        ];
        super::super::radio::draw(content.canvas, centre, checked, state, true)?;
        content.canvas.text(
            name,
            [
                centre[0] + content.canvas.r(2.4),
                content.y + content.canvas.r(1.8),
            ],
            bounds[2] - centre[0] - content.canvas.r(4.8),
            BODY,
            TEXT,
            false,
        )?;
        content.canvas.hit(action, bounds)?;
        content.y += height;
    }
    Ok(())
}

fn account(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.account.tab.title", "menu.account.tab.description")?;
    let view = content.view;
    if matches!(view.auth_state, AuthState::Authenticated) {
        content.label_text(
            &content.word("menu.account.gamertag.title"),
            launcher::menu::profile::current_name(view),
        )?;
        for (title, button, link) in [
            (
                "menu.account.changeGamertag.title",
                "menu.account.changeGamertag.buttonLabel",
                SupportLink::Gamertag,
            ),
            (
                "menu.account.manageAccount.title",
                "menu.account.manageAccount.buttonLabel",
                SupportLink::Account,
            ),
        ] {
            action_row(
                content,
                title,
                "",
                button,
                Some(MenuAction::SettingsSupport(SupportAction::Open(link))),
            )?;
        }
        action_row(
            content,
            "menu.account.signOutOfMicrosoft.title",
            "",
            "menu.account.signOutOfMicrosoft.buttonLabel",
            Some(MenuAction::SignOut),
        )?;
    } else {
        action_row(
            content,
            "menu.account.signIn.title",
            "",
            "menu.account.signIn.buttonLabel",
            Some(MenuAction::StartSignIn),
        )?;
    }
    Ok(())
}

fn resources(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    super::resources::draw(content)
}
fn storage(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.storage.tab.title", "menu.storage.tab.description")?;
    let view = content.view;
    let titles = [
        "hbui.Settings.storage.worlds.title",
        "hbui.Settings.storage.worldTemplates.title",
        "hbui.Settings.storage.resourcePacks.title",
        "hbui.Settings.storage.behaviorPacks.title",
        "storageManager.contentType.skinPacks",
        "hbui.Settings.storage.cachedData.title",
    ];
    for (category_index, (category, title)) in CATEGORIES.iter().zip(titles).enumerate() {
        let Some(items) = view.storage.items(category) else {
            continue;
        };
        let bytes = items
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.bytes));
        let description = format!("{} · {}", items.len(), size_text(content, bytes));
        let category_title = content.word(title);
        action_row_text(
            content,
            &category_title,
            &description,
            if view.storage.expanded[category_index] {
                "−"
            } else {
                "+"
            },
            Some(MenuAction::SettingsStorage(StorageAction::Toggle(
                category_index as u8,
            ))),
        )?;
        if !view.storage.expanded[category_index] {
            continue;
        }
        if items.is_empty() {
            let empty = content
                .word("hbui.Settings.storage.emptyList.title")
                .replace("%1$s", &category_title);
            content.label_text("", &empty)?;
        }
        for (index, item) in items.iter().enumerate() {
            let select = match *category {
                "world" => StorageAction::SelectWorld(index),
                "cache" => StorageAction::Select(index),
                _ => continue,
            };
            let selected = match *category {
                "world" => view.storage.selected_world == Some(index),
                "cache" => view.storage.selected == Some(index),
                _ => false,
            };
            let description = format!("{} · {}", size_text(content, item.bytes), item.date);
            action_row_text(
                content,
                &item.name,
                &description,
                "gui.select",
                Some(MenuAction::SettingsStorage(select)),
            )?;
            if selected {
                action_row(
                    content,
                    "",
                    "",
                    "hbui.Settings.storage.listHeader.delete",
                    Some(MenuAction::SettingsStorage(StorageAction::RequestDelete)),
                )?;
            }
        }
    }
    if view.storage.opens_folders {
        for (folder, title, description) in [
            (
                StorageFolder::Worlds,
                "options.cinnabar.worldsFolder",
                "options.cinnabar.worldsFolder.description",
            ),
            (
                StorageFolder::ResourcePacks,
                "options.cinnabar.resourcePacksFolder",
                "options.cinnabar.resourcePacksFolder.description",
            ),
        ] {
            action_row(
                content,
                title,
                description,
                "options.cinnabar.openFolder.buttonLabel",
                Some(MenuAction::SettingsStorage(StorageAction::OpenFolder(
                    folder,
                ))),
            )?;
        }
    }
    action_row(
        content,
        "options.dev_clearDownloadeCache.name",
        "",
        "gui.clear",
        Some(MenuAction::SettingsStorage(StorageAction::RequestClear)),
    )?;
    action_row(
        content,
        "options.dev_deleteLocalScreenshots",
        "",
        "hbui.Settings.storage.listHeader.delete",
        Some(MenuAction::SettingsStorage(
            StorageAction::RequestScreenshots,
        )),
    )?;
    Ok(())
}

fn size_text(content: &Content<'_, '_>, bytes: u64) -> String {
    let (divisor, key) = if bytes >= 1 << 30 {
        (1_u64 << 30, "playscreen.fileSize.GB")
    } else {
        (1_u64 << 20, "playscreen.fileSize.MB")
    };
    format!("{:.2} {}", bytes as f64 / divisor as f64, content.word(key))
}

/// Draws a row whose title, description and button label are localization keys.
pub(super) fn action_row(
    content: &mut Content<'_, '_>,
    title: &str,
    description: &str,
    label: &str,
    action: Option<MenuAction>,
) -> Result<(), PaintError> {
    let title = content.word(title);
    let description = content.word(description);
    action_row_text(content, &title, &description, label, action)
}

/// Draws a literal title and description with a localized button label.
pub(super) fn action_row_text(
    content: &mut Content<'_, '_>,
    title: &str,
    description: &str,
    label: &str,
    action: Option<MenuAction>,
) -> Result<(), PaintError> {
    let label = content.word(label);
    let pad = content.canvas.r(2.4);
    let left = content.span[0] + pad;
    let right = content.span[1] - pad;
    let narrow = right - left < content.canvas.r(54.0);
    let button_width = (content.canvas.measure(&label, BODY)?
        + content.canvas.r(4.8 + button::icon_width(action)))
    .max(content.canvas.r(button::ACTION_MIN_WIDTH));
    let text_width = if narrow {
        right - left
    } else {
        (right - left) * 0.5
    };
    let title_height = if title.is_empty() {
        0.0
    } else {
        content
            .canvas
            .measure_height(title, text_width, BODY)?
            .max(content.canvas.r(BODY.line))
    };
    let description_height = if description.is_empty() {
        0.0
    } else {
        content
            .canvas
            .measure_height(description, text_width, CAPTION)?
            .max(content.canvas.r(CAPTION.line))
    };
    let text_height = title_height + description_height;
    let button_height = content.canvas.r(button::ACTION_HEIGHT);
    let padding_y = content.canvas.r(1.2);
    let height = if narrow {
        text_height
            + button_height
            + content.canvas.r(if text_height > 0.0 { 0.8 } else { 0.0 })
            + 2.0 * padding_y
    } else {
        text_height.max(button_height) + 2.0 * padding_y
    };
    let bounds = [
        content.span[0],
        content.y,
        content.span[1],
        content.y + height,
    ];
    content.canvas.fill(bounds, NEUTRAL.fill)?;
    let text_y = if narrow {
        content.y + padding_y
    } else {
        content.y + (height - text_height) * 0.5
    };
    content
        .canvas
        .text(title, [left, text_y], text_width, BODY, TEXT, false)?;
    content.canvas.text(
        description,
        [left, text_y + title_height],
        text_width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    let button = if narrow {
        [
            left,
            bounds[3] - padding_y - button_height,
            right,
            bounds[3] - padding_y,
        ]
    } else {
        [
            right - button_width,
            content.y + (height - button_height) * 0.5,
            right,
            content.y + (height + button_height) * 0.5,
        ]
    };
    button::action(content.canvas, content.view, button, &label, action)?;
    widgets::divider(
        content.canvas,
        bounds[0],
        bounds[2],
        bounds[3] - content.canvas.r(EDGE),
    )?;
    content.y += height;
    Ok(())
}

/// Touch-capable hosts expose the live touch preference instead of the desktop placeholder.
fn touch(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    content.heading("menu.touch.tab.title", "menu.touch.tab.description")?;
    if content.view.touch_available {
        for option in launcher::menu::settings_options::layout::SettingPanel::Touch.options() {
            content.option(option.id)?;
        }
        action_row(
            content,
            "Control layout",
            "Move touch controls and align them with guides.",
            "Edit layout",
            Some(MenuAction::EditTouchControls),
        )?;
        content.label(
            "Touch controls",
            "Move with the left stick. Drag outside the buttons to look around.",
        )?;
        content.label(
            "",
            "Use the on-screen buttons to jump, sneak, sprint, attack and use items.",
        )?;
    } else {
        content.label("", "hudScreen.controlCustomization.tooltip.notouch")?;
    }
    Ok(())
}
