//! Launcher actions remain app-owned while UI consumes their views.
use crate::{
    global_resources::{Action, Snapshot},
    menu::{MenuAction, MenuRuntime, MenuScreen},
};
use std::sync::Arc;

#[test]
fn back_closes_pack_settings_before_leaving_global_resources() {
    let mut menu = MenuRuntime::new(true, 2, "Steve".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.global_resources = Arc::new(Snapshot {
        settings: Some(0),
        ..Default::default()
    });
    menu.activate(MenuAction::AddBack);
    assert_eq!(menu.view().screen, MenuScreen::Settings);
    assert_eq!(menu.global_resource_actions, vec![Action::CloseSettings]);
}

#[test]
fn disabled_enhanced_ignores_menu_toggle_requests() {
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::ToggleRenderMode);
    assert!(menu.take_render_mode_request().is_none());
    assert_eq!(menu.view().render_mode, ui::RenderMode::Vanilla);
}
