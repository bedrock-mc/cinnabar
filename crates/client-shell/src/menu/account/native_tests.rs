use super::*;
use launcher_host::auth::test_support::native_state;

/// Failed and expired checks of a selected account never emit a signed-out native join intent.
#[test]
fn native_saved_account_failure_and_timeout_return_to_the_menu() {
    for state in [
        AuthState::Failed("Synthetic validation failed.".into()),
        AuthState::Checking,
    ] {
        let mut menu = MenuRuntime::new(true, 2, "Synthetic".into()).with_native_sign_in(true);
        menu.auth_process = Some(native_state(state));
        menu.auth_attempted = true;
        menu.feeds.account_active_id = Some("123".into());
        menu.request_connect("synthetic.example:19132".into());
        let now = std::time::Instant::now();
        menu.intents.join_sign_in_deadline = Some(now);
        assert!(menu.take_join_intent().is_none());
        assert!(menu.intents.join.is_none());
        assert!(
            menu.view()
                .disconnect_message
                .as_deref()
                .is_some_and(|message| message.contains("selected account"))
        );
    }

    let mut menu = MenuRuntime::new(true, 2, "Signed out".into()).with_native_sign_in(true);
    menu.auth_process = Some(native_state(AuthState::SignedOut));
    menu.auth_attempted = true;
    menu.request_connect("synthetic.example:19132".into());
    assert!(menu.take_join_intent().unwrap().auth_cache.is_none());
}

/// Verified native direct and Realm joins do not wait for an unrelated Go account core.
#[test]
fn verified_native_join_does_not_wait_for_the_account_core() {
    for target in [
        "synthetic.example:19132".into(),
        format!("{}7", bridge::contract::REALM_TARGET_PREFIX),
    ] {
        let mut menu = MenuRuntime::new(true, 2, "Synthetic".into()).with_native_sign_in(true);
        menu.auth_process = Some(native_state(AuthState::Authenticated));
        menu.auth_attempted = true;
        menu.feeds.account_active_id = Some("123".into());
        menu.account_core = Some(super::super::launcher_services::AccountCore::default());
        menu.request_connect(target);
        menu.intents.join_sign_in_deadline =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(10));
        let intent = menu
            .take_join_intent()
            .expect("native readiness does not use the Go core");
        assert_eq!(intent.auth_cache, Some(menu.layout.auth_cache()));
    }
}

/// A one-shot Go catalog arriving late cannot overwrite either native account-backed section.
#[test]
fn native_catalog_rejects_late_one_shot_go_social_rows() {
    let catalog: CatalogFile = serde_json::from_value(serde_json::json!({
        "realms":[{"name":"legacy","state":"OPEN"}],
        "friends":[{"gamertag":"legacy","world_name":"legacy","xuid":"456","members":1,"max_members":2}]
    })).unwrap();
    let mut menu = MenuRuntime::new(true, 2, "Synthetic".into()).with_native_sign_in(true);
    let mut native_realm = catalog.realms[0].clone();
    native_realm.name = "native".into();
    menu.realms.replace(vec![native_realm]);
    let mut native_friend: launcher::menu::view::MenuFriendCard = catalog.friends[0].clone().into();
    native_friend.world_name = "native".into();
    menu.friends.replace(vec![native_friend]);
    menu.apply_catalog(catalog.clone());
    assert_eq!(menu.realms[0].name, "native");
    assert_eq!(menu.friends[0].world_name, "native");

    let mut go = MenuRuntime::new(true, 2, "Synthetic".into());
    go.apply_catalog(catalog);
    assert_eq!(go.realms[0].name, "legacy");
    assert_eq!(go.friends[0].world_name, "legacy");
}
