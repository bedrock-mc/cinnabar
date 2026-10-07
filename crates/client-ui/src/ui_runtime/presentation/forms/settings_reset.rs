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
    use crate::menu::{MenuScreen, MenuView};

    #[test]
    fn authored_section_reset_buttons_route_to_their_own_group() {
        let player_runtime = player_state::PlayerState::new(1);

        let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
            eprintln!(
                "skipping authored_section_reset_buttons_route_to_their_own_group: fixture unavailable; requires installed local carriers (make assets)"
            );
            return;
        };
        for (section, group) in [
            ("video_forced_index", SettingsGroup::Video),
            ("accessibility_forced_index", SettingsGroup::Accessibility),
            ("sound_forced_index", SettingsGroup::Audio),
        ] {
            let mut view = MenuView::new(true, "Steve".into());
            view.screen = MenuScreen::Settings;
            view.settings_section = SETTINGS_SECTIONS
                .iter()
                .find_map(|(name, index)| (*name == section).then_some(*index))
                .unwrap();
            presentation.set_menu_view(Some(view.clone()));
            let runtime = crate::ui_runtime::UiRuntime::new(1);
            let dpi = ui::DpiScale::new(1.0).unwrap();
            presentation
                .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
                .unwrap();
            let action = MenuAction::SettingsResetGroup(group);
            assert!(
                presentation
                    .menu_focus_actions()
                    .any(|candidate| candidate == action),
                "{section} must expose its reset action to keyboard navigation",
            );
            view.focused_action = Some(action);
            presentation.set_menu_view(Some(view));
            presentation
                .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
                .unwrap();
            let bounds = presentation
                .menu_hit_targets
                .iter()
                .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
                .expect("keyboard focus reveals the section's reset button");
            let point = ui::UiPoint::new(
                (bounds.min().x() + bounds.max().x()) * 0.5,
                (bounds.min().y() + bounds.max().y()) * 0.5,
            )
            .unwrap();
            assert_eq!(presentation.hit_test_menu(point), Some(action), "{section}");
        }
    }
}
