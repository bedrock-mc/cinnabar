//! Host routing for the pack's three reset_settings buttons.

use super::menu_screens::{SETTINGS_SECTIONS, Translate, translated};
use crate::menu::{MenuAction, MenuView, settings_options::SettingsGroup};
use json_ui::HitRegion;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{MenuRuntime, MenuScreen};

    #[test]
    fn authored_section_reset_buttons_route_to_their_own_group() {
        let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

        let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
            return;
        };
        for (section, group) in [
            ("video_forced_index", SettingsGroup::Video),
            ("accessibility_forced_index", SettingsGroup::Accessibility),
            ("sound_forced_index", SettingsGroup::Audio),
        ] {
            let mut view = MenuRuntime::new(true, 2, "Steve".into()).view();
            view.screen = MenuScreen::Settings;
            view.settings_section = SETTINGS_SECTIONS
                .iter()
                .find_map(|(name, index)| (*name == section).then_some(*index))
                .unwrap();
            presentation.set_menu_view(Some(view));
            let runtime = crate::ui_runtime::UiRuntime::new(1);
            let dpi = ui::DpiScale::new(1.0).unwrap();
            presentation
                .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
                .unwrap();
            let bounds = presentation
                .menu_hit_targets
                .iter()
                .find_map(|(action, bounds)| {
                    matches!(
                        action,
                        MenuAction::SettingsOption(..) | MenuAction::SettingsDropdown(_)
                    )
                    .then_some(*bounds)
                })
                .expect("section exposes a control inside its scroll view");
            let point = ui::UiPoint::new(bounds.min().x() + 1.0, bounds.min().y() + 1.0).unwrap();
            assert!(presentation.scroll_menu(point, -1000.0, false));
            presentation
                .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
                .unwrap();
            assert!(
                presentation
                    .menu_hit_targets
                    .iter()
                    .any(|(action, _)| *action == MenuAction::SettingsResetGroup(group)),
                "{section}: {:?}",
                presentation.menu_hit_targets
            );
        }
    }
}
