use super::*;
use launcher_host::auth::test_support::native_state;

/// Failed and expired checks of a selected account never emit a signed-out native join intent.
#[test]
fn native_saved_account_failure_and_timeout_return_to_the_menu() {
    for state in [
        AuthState::Failed("Synthetic validation failed.".into()),
        AuthState::Checking,
    ] {
        let mut menu = MenuRuntime::new(true, 2, "Synthetic".into());
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

    let mut menu = MenuRuntime::new(true, 2, "Signed out".into());
    menu.auth_process = Some(native_state(AuthState::SignedOut));
    menu.auth_attempted = true;
    menu.request_connect("synthetic.example:19132".into());
    assert!(menu.take_join_intent().unwrap().auth_cache.is_none());
}

/// Verified direct and Realm joins retain their original validated account cache.
#[test]
fn verified_native_join_retains_the_validated_cache() {
    for target in [
        "synthetic.example:19132".into(),
        format!("{}7", bridge::contract::REALM_TARGET_PREFIX),
    ] {
        let mut menu = MenuRuntime::new(true, 2, "Synthetic".into());
        menu.auth_process = Some(native_state(AuthState::Authenticated));
        menu.auth_attempted = true;
        menu.feeds.account_active_id = Some("123".into());
        menu.request_connect(target);
        menu.intents.join_sign_in_deadline =
            Some(std::time::Instant::now() + std::time::Duration::from_secs(10));
        let intent = menu
            .take_join_intent()
            .expect("native readiness does not use the Go core");
        assert_eq!(intent.auth_cache, Some(menu.layout.auth_cache()));
    }
}

/// The original saved-account check remains pending until its native supervisor settles.
#[test]
fn connect_during_cached_check_waits_and_retains_the_validated_cache() {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.auth_process = Some(native_state(AuthState::Checking));
    menu.auth_attempted = true;
    menu.request_connect("fixture.invalid:19132".into());
    assert!(menu.take_join_intent().is_none());
    assert_eq!(menu.owned_auth(), Some(&AuthState::Checking));
    menu.auth_process = Some(native_state(AuthState::Authenticated));
    let joined = menu.take_join_intent().expect("the original check settled");
    assert_eq!(joined.auth_cache, Some(menu.layout.auth_cache()));
    assert_eq!(menu.sign_in_status(), SignInStatus::SignedIn);
}

/// A cached check's fallback prompt never opens a browser or grants an unsigned join.
#[test]
fn held_join_cancels_a_native_fallback_prompt_before_browser_dispatch() {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.auth_process = Some(native_state(AuthState::Checking));
    menu.auth_attempted = true;
    menu.request_connect("fixture.invalid:19132".into());
    let prompt = AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    };
    menu.auth_process = Some(native_state(prompt.clone()));
    menu.poll_account(true);
    assert_eq!(
        menu.sign_in_browser.state(&prompt),
        launcher::menu::sign_in::BrowserState::Waiting
    );
    assert_eq!(menu.owned_auth(), Some(&AuthState::SignedOut));
    assert!(menu.take_join_intent().is_none());
    assert!(menu.intents.join.is_none());
}
