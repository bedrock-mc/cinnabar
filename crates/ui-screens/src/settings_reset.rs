//! Host routing for the pack's three reset_settings buttons.

use json_ui::HitRegion;
use launcher::menu::settings_sections::SETTINGS_SECTIONS;
use launcher::menu::{MenuAction, MenuView, settings_options::SettingsGroup};
use ui_engine::engine::screen::{Translate, translated};

/// Uses the active section because HitRegion does not expose the reset_group property bag.
pub(super) fn action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    if region.pressed.as_deref() != Some("button.reset_settings") {
        return None;
    }
    let section = SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*index == view.settings_section).then_some(*name));
    let group = match section {
        Some("video_forced_index") => SettingsGroup::Video,
        None if view.settings_section == 0 => SettingsGroup::Video,
        Some("accessibility_forced_index") => SettingsGroup::Accessibility,
        Some("sound_forced_index") => SettingsGroup::Audio,
        _ => return None,
    };
    Some(MenuAction::SettingsResetGroup(group))
}

/// The native confirmation asks before applying any defaults.
pub(super) fn dialog_model(
    group: SettingsGroup,
    translate: Translate<'_>,
) -> (json_ui::FormModel, MenuAction) {
    let words = |key| translated(translate, key, key);
    (
        json_ui::FormModel::Modal(json_ui::ModalForm {
            title: words("options.resetSettings"),
            body: words("options.resetSettings.popUp"),
            button1: words("options.continue"),
            button2: words("gui.cancel"),
        }),
        MenuAction::SettingsConfirmResetGroup(group),
    )
}
