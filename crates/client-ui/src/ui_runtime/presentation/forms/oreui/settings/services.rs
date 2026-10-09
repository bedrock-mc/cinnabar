//! Settings panels backed by launcher account, language, pack and storage state.

use super::super::super::super::UiPresentationError;
use super::super::theme::{BODY, CAPTION, EDGE, NEUTRAL, TEXT, TEXT_DIMMER};
use super::super::widgets;
use super::{Content, button};
use crate::menu::{
    MenuAction,
    auth::AuthState,
    settings_storage::{CATEGORIES, StorageAction},
    settings_support::{SupportAction, SupportLink},
};

#[cfg(test)]
mod tests;

pub(super) fn draw(
    content: &mut Content<'_, '_>,
    section: &str,
) -> Result<bool, UiPresentationError> {
    match section {
        "language_forced_index" => language(content)?,
        "account_forced_index" => account(content)?,
        "global_texture_pack_forced_index" => resources(content)?,
        "storage_management_forced_index" => storage(content)?,
        "view_subscriptions_forced_index" => subscriptions(content)?,
        "touch_forced_index" => {
            content.heading("menu.touch.tab.title", "menu.touch.tab.description")?;
            content.label("", "hudScreen.controlCustomization.tooltip.notouch")?;
        }
        "party_forced_index" => {
            content.heading("options.party", "")?;
            for title in [
                "options.partyInviteReceivedFilter",
                "options.partyPrivacy",
                "options.partyInviteSendPrivileges",
            ] {
                action_row(content, title, "", "gui.select", None)?;
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn language(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
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

fn account(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading("menu.account.tab.title", "menu.account.tab.description")?;
    let view = content.view;
    if matches!(view.auth_state, AuthState::Authenticated) {
        content.label(
            "menu.account.gamertag.title",
            super::super::super::accounts::current_name(view),
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
            "menu.account.privacyAndSafety.title",
            "",
            "menu.account.privacyAndSafety.buttonLabel",
            None,
        )?;
        action_row(
            content,
            "menu.account.realmMembershipInvites.title",
            "",
            "menu.account.realmMembershipInvites.buttonLabel",
            None,
        )?;
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

fn subscriptions(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    content.heading("options.viewSubscriptions", "")?;
    if matches!(content.view.auth_state, AuthState::Authenticated) {
        content.label("options.viewSubscriptions.mySubscriptions", "")?;
        action_row(
            content,
            "options.viewSubscriptions.realmsServer",
            "",
            "options.viewSubscriptions.button.manage",
            None,
        )?;
    } else {
        action_row(
            content,
            "menu.account.signIn.title",
            "",
            "options.viewSubscriptions.signIn",
            Some(MenuAction::StartSignIn),
        )?;
    }
    Ok(())
}

fn resources(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    super::resources::draw(content)
}
fn storage(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
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
        let items = view.storage.items(category);
        let bytes = items
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.bytes));
        let description = format!("{} · {}", items.len(), size_text(content, bytes));
        action_row(
            content,
            title,
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
                .replace("%1$s", &content.word(title));
            content.label("", &empty)?;
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
            action_row(
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

pub(super) fn action_row(
    content: &mut Content<'_, '_>,
    title: &str,
    description: &str,
    label: &str,
    action: Option<MenuAction>,
) -> Result<(), UiPresentationError> {
    let title = content.word(title);
    let description = content.word(description);
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
            .measure_height(&title, text_width, BODY)?
            .max(content.canvas.r(BODY.line))
    };
    let description_height = if description.is_empty() {
        0.0
    } else {
        content
            .canvas
            .measure_height(&description, text_width, CAPTION)?
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
        .text(&title, [left, text_y], text_width, BODY, TEXT, false)?;
    content.canvas.text(
        &description,
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
