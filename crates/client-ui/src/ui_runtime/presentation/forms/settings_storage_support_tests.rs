//! Real-carrier storage and support input checks; PNGs use the offline gallery path.

use launcher::menu::{
    MenuAction, MenuDialog,
    settings_support::{SupportAction, SupportDialog, SupportLink},
};

#[test]
fn settings_help_owns_input_and_short_font_attribution_stays_clamped() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping settings_help_owns_input_and_short_font_attribution_stays_clamped: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut view = settings();
    view.dialog = Some(MenuDialog::SettingsSupport(SupportDialog::Help));
    let actions = draw(&player_runtime, &mut presentation, &view);
    assert!(
        actions.contains(&MenuAction::SettingsSupport(SupportAction::Open(
            SupportLink::Help
        ))),
        "{actions:?}"
    );
    assert!(actions.contains(&MenuAction::DismissDialog));
    assert_eq!(actions.len(), 2, "modal must own input");
    super::play_flow_snapshots::snapshot(&player_runtime, &view, "settings-help-center");
    view.dialog = Some(MenuDialog::SettingsSupport(SupportDialog::FontLicense));
    assert!(
        draw(&player_runtime, &mut presentation, &view)
            .iter()
            .all(|action| *action == MenuAction::DismissDialog)
    );
    assert!(
        presentation.scroll_menu(ui::UiPoint::new(640.0, 360.0).unwrap(), -8.0, false),
        "font attribution modal must own wheel input"
    );
    assert!(
        presentation
            .menu_scrolls
            .offsets()
            .values()
            .all(|offset| *offset == 0.0),
        "short font attribution must not scroll beyond its content"
    );
    super::play_flow_snapshots::snapshot(&player_runtime, &view, "settings-font-license");
}

use crate::test_support::{draw_menu_actions as draw, settings_view as settings};
