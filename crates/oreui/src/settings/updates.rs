//! App update preferences and worker status have their own settings tab.

use super::{Content, services};
use launcher::menu::{
    MenuAction,
    settings_options::layout::SettingPanel,
    updates::{Action, installed_version},
};
use ui_engine::engine::screen::PaintError;

/// Presents the installed version, channel policy and one explicit fresh-check action.
pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let view = content.view;
    content.heading("Updates", "")?;
    if view.updates.managed {
        return content.label(
            &installed_version(),
            "Updates are managed by Flatpak. Use your software store or run flatpak update.",
        );
    }
    if !view.updates.active {
        return content.label(
            &installed_version(),
            "Update controls are unavailable in this window.",
        );
    }
    for option in SettingPanel::Updates.options() {
        content.option(option.id)?;
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
