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
fn menu_render_mode_requests_follow_the_build_feature() {
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::ToggleRenderMode);
    let requested = render_model::ENHANCED_RENDERING_ENABLED.then_some(ui::RenderMode::Enhanced);
    assert_eq!(menu.take_render_mode_request(), requested);
    assert_eq!(
        menu.view().render_mode,
        requested.unwrap_or(ui::RenderMode::Vanilla)
    );
    assert!(menu.take_render_mode_request().is_none());
}
