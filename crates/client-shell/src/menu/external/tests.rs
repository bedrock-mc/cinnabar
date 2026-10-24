use super::*;

#[test]
fn external_account_actions_never_start_native_helpers() {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.use_external_services(vec![], 1);
    assert_eq!(menu.view().screen, MenuScreen::Home);
    menu.activate(MenuAction::StartSignIn);
    assert_eq!(menu.take_account_intent(), Some(AccountIntent::SignIn));
    assert_eq!(menu.current_auth().as_ref(), &AuthState::Checking);
    assert!(menu.auth_process.is_none());
    menu.sync_external_account(
        AuthState::AwaitingCode {
            uri: "https://example.invalid".into(),
            code: "TEST".into(),
        },
        MenuProfile::default(),
        None,
    );
    let mut expected = None;
    launcher::menu::sign_in::SignInLink::default().open("TEST", true, |url, _| {
        expected = Some(url);
        true
    });
    assert_eq!(
        menu.external_sign_in_link(MenuAction::OpenSignInLink),
        expected
    );
    assert!(
        menu.external_sign_in_link(MenuAction::StartSignIn)
            .is_none()
    );
    menu.activate(MenuAction::CancelSignIn);
    assert_eq!(menu.take_account_intent(), Some(AccountIntent::Cancel));
    assert!(
        menu.external_sign_in_link(MenuAction::OpenSignInLink)
            .is_none()
    );
    menu.activate(MenuAction::SignOut);
    assert_eq!(menu.take_account_intent(), Some(AccountIntent::SignOut));
    assert!(menu.accounts.operation.is_none());
}

#[test]
fn external_menu_refuses_service_targets_and_keeps_addressed_join_intent() {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.use_external_services(vec![], 0);
    for address in [
        format!("{}1", bridge::contract::REALM_TARGET_PREFIX),
        format!("{}1", launcher::menu::FRIEND_ADDRESS_PREFIX),
        format!("{}1", launcher::menu::EXPERIENCE_ADDRESS_PREFIX),
    ] {
        menu.request_connect(address);
        assert!(menu.take_join_intent().is_none());
        assert!(menu.view().message.is_some());
        assert_eq!(menu.view().dialog, Some(launcher::menu::MenuDialog::Notice));
        menu.activate(MenuAction::DismissDialog);
    }
    menu.request_connect("example.invalid".into());
    let join = menu.take_join_intent().unwrap();
    assert_eq!(join.address, "example.invalid");
    assert!(join.auth_cache.is_none());
    assert!(!join.local_world);
}

#[test]
fn external_preferences_emit_only_edits_and_surface_storage_failures() {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.use_external_services(vec![], 0);
    assert!(menu.take_external_preferences().is_none());
    menu.servers.push(SavedServer {
        name: "Fixture".into(),
        address: "example.invalid".into(),
        favorite: false,
        last_joined_unix: 0,
    });
    menu.save_servers();
    let (servers, offset) = menu.take_external_preferences().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(offset, 0);
    assert!(menu.take_external_preferences().is_none());
    menu.gui_scale_offset = -1;
    assert_eq!(menu.take_external_preferences().unwrap().1, -1);
    assert!(menu.take_external_preferences().is_none());
    for screen in [MenuScreen::Home, MenuScreen::Settings, MenuScreen::Servers] {
        menu.screen = screen;
        let message = "Browser preferences could not be saved.";
        menu.show_notice(message);
        let view = menu.view();
        assert_eq!(view.dialog, Some(launcher::menu::MenuDialog::Notice));
        let popup = launcher::menu::dialogs::dialog(&view, view.dialog.unwrap(), &|_| None);
        assert_eq!(popup.body, message);
        menu.activate(MenuAction::DismissDialog);
        assert!(menu.view().dialog.is_none());
    }
}

#[test]
fn external_retry_survives_transport_loss_but_not_an_account_change() {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.use_external_services(vec![], 0);
    let profile = MenuProfile {
        gamertag: "Fixture".into(),
        ..MenuProfile::unavailable()
    };
    menu.sync_external_account(AuthState::Authenticated, profile.clone(), None);
    assert_eq!(menu.sign_in_status(), crate::menu::SignInStatus::SignedIn);
    assert!(!menu.view().catalog_loading);
    assert_eq!(
        menu.view().catalog_message.as_deref(),
        Some(SERVICE_JOINS_UNAVAILABLE)
    );
    menu.activate(MenuAction::RefreshCatalog);
    assert!(!menu.view().catalog_loading);
    assert!(menu.view().catalog_message.is_some());
    menu.request_connect("example.invalid".into());
    assert!(menu.take_join_intent().is_some());
    menu.show_join_failure("Fixture failure".into());
    assert!(!menu.view().catalog_loading);
    menu.sync_external_account(
        AuthState::Failed("Disconnected".into()),
        MenuProfile::default(),
        None,
    );
    assert!(menu.retry_target.is_some());
    menu.sync_external_account(AuthState::Authenticated, profile, None);
    assert!(menu.retry_target.is_some());
    menu.activate(MenuAction::SignOut);
    assert!(menu.retry_target.is_some());
    menu.sync_external_account(AuthState::SignedOut, MenuProfile::default(), None);
    assert!(menu.retry_target.is_none());
}
