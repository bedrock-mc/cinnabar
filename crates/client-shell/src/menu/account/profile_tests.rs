use super::*;

#[test]
fn profile_loading_ends_without_a_feed_worker_in_both_startup_modes() {
    for launcher in [false, true] {
        let mut menu = MenuRuntime::new_with_layout(
            launcher,
            Some(2),
            "Fixture".into(),
            launcher::test_support::scratch("profile-no-worker"),
            crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
        );
        menu.control_auth = Some(AuthState::Authenticated);
        menu.enter(launcher::menu::MenuScreen::Profile);
        assert!(
            menu.feeds.profile_refresh_requested,
            "opening must request Profile"
        );
        menu.poll_account(false);
        assert!(
            menu.view().feeds.profile.unavailable,
            "no Profile worker must show unavailable, launcher={launcher}"
        );
        menu.activate(launcher::menu::MenuAction::RefreshProfile);
        menu.poll_account(false);
        assert!(
            menu.view().feeds.profile.unavailable,
            "retry without a worker hung"
        );
    }
}

#[test]
fn profile_account_feed_does_not_prevent_cached_validation_in_direct_mode() {
    let layout = launcher::test_support::scratch("profile-cached-validation");
    let auth_path = layout.auth_cache();
    std::fs::create_dir_all(auth_path.parent().unwrap()).unwrap();
    // An empty cache triggers validation; this fixture has no runtime or credentials.
    std::fs::write(&auth_path, []).unwrap();
    let mut menu = MenuRuntime::new_with_layout(
        false,
        Some(2),
        "Fixture".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    );
    menu.enter(launcher::menu::MenuScreen::Profile);
    menu.poll_account(true);
    assert!(
        menu.auth_attempted,
        "an attached signed-out service suppressed cached validation"
    );
    assert!(
        menu.owned_auth().is_none(),
        "fixture has no authentication runtime"
    );
    std::fs::remove_file(auth_path).unwrap();
}
