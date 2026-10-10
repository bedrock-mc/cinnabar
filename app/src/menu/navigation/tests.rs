use {
    super::super::MenuRuntime,
    launcher::menu::{MenuAction, MenuScreen},
};

#[test]
fn a_loaded_self_profile_can_open_dressing_room_from_keyboard_focus() {
    let mut menu = MenuRuntime::new(true, 2, "BugTest".into());
    menu.control_auth = Some(launcher::menu::auth::AuthState::Authenticated);
    menu.feeds.profile.loaded = true;
    menu.feeds.profile.avatar_loaded = true;
    menu.feeds.profile.featured_screenshot_loaded = true;
    menu.activate(MenuAction::Navigate(MenuScreen::Profile));
    let action = MenuAction::Navigate(MenuScreen::DressingRoom);
    menu.focus_pointer(action);
    assert_eq!(menu.view().focused_action, Some(action));
    menu.activate_focused();
    assert_eq!(menu.screen(), MenuScreen::DressingRoom);
    menu.activate(MenuAction::AddBack);
    assert_eq!(menu.screen(), MenuScreen::Profile);
}

fn join_from(screen: MenuScreen) -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(screen));
    menu.request_connect("fixture.invalid".into());
    assert!(menu.take_join_intent().is_some());
    menu.show_home();
    menu.show_connecting();
    menu.show_world();
    menu
}

#[test]
fn quitting_reveals_each_originating_play_page_with_a_way_back_home() {
    for origin in [MenuScreen::Play, MenuScreen::Social, MenuScreen::Servers] {
        let mut menu = join_from(origin);
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);
        menu.show_after_disconnect();
        assert_eq!(menu.screen(), origin);
        assert!(menu.is_visible());
        assert!(menu.uses_panorama());
        assert!(menu.view().message.is_none());
        menu.activate(MenuAction::AddBack);
        assert_eq!(menu.screen(), MenuScreen::Home);
    }
}

#[test]
fn a_replacement_join_retains_the_original_play_page() {
    let mut menu = join_from(MenuScreen::Servers);
    menu.request_connect("replacement.invalid".into());
    assert!(menu.take_join_intent().is_some());
    menu.show_home();
    menu.show_connecting();
    menu.show_world();
    menu.show_after_disconnect();
    assert_eq!(menu.screen(), MenuScreen::Servers);
}

#[test]
fn quitting_a_local_world_reveals_worlds() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Play));
    menu.request_local_world_join("Local fixture".into(), false);
    assert!(menu.take_join_intent().is_some());
    menu.show_world();
    menu.open_pause();
    menu.show_after_disconnect();
    assert_eq!(menu.screen(), MenuScreen::Play);
}

#[test]
fn joining_from_a_server_form_returns_to_the_page_that_opened_it() {
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Servers));
    menu.activate(MenuAction::PlayAddServer);
    menu.request_connect("fixture.invalid".into());
    assert!(menu.take_join_intent().is_some());
    menu.show_world();
    menu.show_after_disconnect();
    assert_eq!(menu.screen(), MenuScreen::Servers);
}

#[test]
fn cancellation_returns_to_the_origin_and_clears_the_queued_join() {
    for origin in [MenuScreen::Play, MenuScreen::Social, MenuScreen::Servers] {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.activate(MenuAction::Navigate(origin));
        menu.request_connect("fixture.invalid".into());
        menu.cancel_join();
        assert_eq!(menu.screen(), origin);
        assert!(!menu.is_connecting());
        assert!(menu.take_join_intent().is_none());
    }
}

#[test]
fn a_new_launcher_join_replaces_the_previous_origin() {
    let mut menu = join_from(MenuScreen::Servers);
    menu.show_after_disconnect();
    menu.activate(MenuAction::Navigate(MenuScreen::Social));
    menu.request_connect("fixture.invalid".into());
    assert!(menu.take_join_intent().is_some());
    menu.show_world();
    menu.show_after_disconnect();
    assert_eq!(menu.screen(), MenuScreen::Social);
}

#[test]
fn a_session_without_a_play_page_keeps_its_existing_return_defaults() {
    for launcher in [false, true] {
        let mut menu = MenuRuntime::new(launcher, 2, "Player".into());
        menu.show_world();
        menu.open_pause();
        menu.show_after_disconnect();
        assert_eq!(menu.screen(), MenuScreen::Home);
    }
    let mut menu = MenuRuntime::new(true, 2, "Player".into());
    menu.request_connect("fixture.invalid".into());
    menu.cancel_join();
    assert_eq!(menu.screen(), MenuScreen::Play);
}
