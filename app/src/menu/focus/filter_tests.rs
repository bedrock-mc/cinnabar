use super::review_tests::native_rows;
use super::*;

#[test]
fn server_filter_navigation_tracks_visible_controls() {
    let mut menu = MenuRuntime::new(true, 2, "Filter".into());
    menu.enter(MenuScreen::Servers);
    native_rows(
        &mut menu,
        &[
            (MenuAction::PlayAddServer, [0.0, 0.0, 40.0, 20.0]),
            (MenuAction::OpenServerFilter, [40.0, 0.0, 60.0, 20.0]),
            (MenuAction::SelectSaved(0), [0.0, 20.0, 60.0, 40.0]),
        ],
    );
    menu.focus_pointer(MenuAction::SelectSaved(0));
    assert_eq!(menu.view().focused_action, Some(MenuAction::SelectSaved(0)));
    native_rows(
        &mut menu,
        &[
            (MenuAction::PlayAddServer, [0.0, 0.0, 40.0, 20.0]),
            (MenuAction::OpenServerFilter, [40.0, 0.0, 60.0, 20.0]),
        ],
    );
    assert!(!menu.focus_actions().contains(&MenuAction::SelectSaved(0)));
    menu.focus_pointer(MenuAction::OpenServerFilter);
    menu.activate_focused();
    assert_eq!(menu.view().dialog, Some(MenuDialog::ServerFilter));
}
