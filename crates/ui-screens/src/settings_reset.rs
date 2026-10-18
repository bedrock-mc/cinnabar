//! Host routing for the pack's three reset_settings buttons.

use json_ui::HitRegion;
use launcher::menu::settings_sections::SettingSection;
use launcher::menu::{MenuAction, MenuView};

/// Uses the active section because HitRegion does not expose the reset_group property bag.
pub(super) fn action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    if region.pressed.as_deref() != Some("button.reset_settings") {
        return None;
    }
    let section = if view.settings.section == 0 {
        SettingSection::Video
    } else {
        SettingSection::from_index(view.settings.section)?
    };
    let group = section.definition().reset_group?;
    Some(MenuAction::SettingsResetGroup(group))
}
