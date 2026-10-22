//! App and mod update preferences share a settings tab with app worker status.

use super::{Content, services};
use launcher::menu::{
    MenuAction,
    settings_options::{SettingId, layout::SettingPanel},
    updates::{Action, installed_version},
};
use ui_engine::engine::screen::PaintError;

/// Presents the installed version, channel policy and one explicit fresh-check action.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let view = content.view;
    content.heading("Updates", "")?;
    for option in SettingPanel::Updates.options() {
        if view.updates.active
            || matches!(
                option.id,
                SettingId::ModUpdateChecks | SettingId::ModAutoUpdate
            )
        {
            content.option(option.id)?;
        }
    }
    if !view.updates.active {
        return content.label(
            &installed_version(),
            "App update controls are unavailable in this window.",
        );
    }
    let mut status = view.updates.status_text();
    if view.over_world {
        status.push_str(" Return to the home menu to check for updates.");
    }
    services::action_row(
        content,
        &installed_version(),
        &status,
        "Check now",
        (!view.over_world && !view.updates.status.is_busy())
            .then_some(MenuAction::Update(Action::CheckNow)),
    )?;
    if !view.over_world {
        if let Some(message) = &view.updates.message {
            content.label("", message)?;
        }
        if view.updates.switch_available {
            services::action_row(
                content,
                "Change channel now",
                "Download this channel's older build for manual installation.",
                "Switch now",
                Some(MenuAction::Update(Action::SwitchNow)),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
