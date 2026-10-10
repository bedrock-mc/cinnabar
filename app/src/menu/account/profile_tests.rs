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
        menu.catalog_started = true;
        menu.enter(launcher::menu::MenuScreen::Profile);
        assert!(
            menu.feeds.profile_refresh_requested,
            "opening must request Profile"
        );
        menu.poll_catalog(false);
        assert!(
            menu.view().feeds.profile.unavailable,
            "no Profile worker must show unavailable, launcher={launcher}"
        );
        menu.activate(launcher::menu::MenuAction::RefreshProfile);
        menu.poll_catalog(false);
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
    // Presence alone triggers validation. No credentials or real core executable exist here.
    std::fs::write(&auth_path, []).unwrap();
    let mut menu = MenuRuntime::new_with_layout(
        false,
        Some(2),
        "Fixture".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    );
    menu.enter(launcher::menu::MenuScreen::Profile);
    menu.poll_catalog(true);
    assert!(
        menu.auth_attempted,
        "an attached signed-out core suppressed cached validation"
    );
    assert!(
        menu.auth_process.is_none(),
        "fixture must not start a real authentication helper"
    );
    std::fs::remove_file(auth_path).unwrap();
}

