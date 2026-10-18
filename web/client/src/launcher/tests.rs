use super::*;
use preferences::{MAX_PREFERENCE_BYTES, MAX_SERVERS, Preferences};
use serde_json::{Value, json};
use web_transport::session::RequestRefusal;

/// Builds a typed account event from a wire fixture.
fn account_event(value: Value) -> ControlEvent {
    ControlEvent::Account(serde_json::from_value(value).unwrap())
}

/// Builds a typed join-progress event from a wire fixture.
fn connect_event(value: Value) -> ControlEvent {
    ControlEvent::Connect(serde_json::from_value(value).unwrap())
}

/// Delivers a relay account snapshot through the real typed event boundary.
fn account(adapter: &mut RelayAccount, state: &str) {
    adapter.observe(&account_event(json!({"state": state,
        "verification_uri": "https://example.invalid/device", "user_code": "TEST-CODE",
        "gamertag": "Fixture"})));
}

#[test]
fn launcher_cancel_before_code_cancels_the_pending_relay_flow() {
    let mut adapter = RelayAccount::default();
    adapter.request(AccountIntent::SignIn);
    assert_eq!(adapter.state(), AuthState::Checking);
    assert_eq!(adapter.take_operation(), Some(AccountOperation::SignIn));
    adapter.request(AccountIntent::Cancel);
    assert_eq!(adapter.take_operation(), Some(AccountOperation::SignOut));
    account(&mut adapter, "awaiting_code");
    assert_eq!(adapter.state(), AuthState::SignedOut);
    assert!(adapter.take_operation().is_none());
    adapter.request(AccountIntent::SignIn);
    account(&mut adapter, "awaiting_code");
    assert!(matches!(adapter.state(), AuthState::AwaitingCode { .. }));
}

#[test]
fn launcher_refusal_survives_identical_snapshots_and_retry_clears_it() {
    let mut adapter = RelayAccount::default();
    account(&mut adapter, "signed_out");
    adapter.request(AccountIntent::SignIn);
    adapter.observe(&ControlEvent::RequestRefused(RequestRefusal::Refused(
        bridge::contract::RefusalCode::SessionBusy,
    )));
    account(&mut adapter, "signed_out");
    assert!(matches!(adapter.state(), AuthState::Failed(_)));
    adapter.request(AccountIntent::SignIn);
    assert_eq!(adapter.state(), AuthState::Checking);
    assert_eq!(adapter.take_operation(), Some(AccountOperation::SignIn));
    account(&mut adapter, "signed_in");
    assert_eq!(adapter.state(), AuthState::Authenticated);
    assert_eq!(adapter.profile().gamertag, "Fixture");
    adapter.request(AccountIntent::Cancel);
    // Dismissing a completed prompt must not sign out a completed account.
    assert_eq!(adapter.take_operation(), None);
    adapter.request(AccountIntent::SignOut);
    assert_eq!(adapter.take_operation(), Some(AccountOperation::SignOut));
    account(&mut adapter, "signed_out");
    assert_eq!(adapter.state(), AuthState::SignedOut);
    assert!(adapter.profile().gamertag.is_empty());
}

#[test]
fn launcher_disconnect_and_new_join_retire_old_presentation() {
    let mut adapter = RelayAccount::default();
    account(&mut adapter, "signed_in");
    adapter.observe(&connect_event(
        json!({"stage":"packs", "packs_done":1, "packs_total":2}),
    ));
    assert!(adapter.join_stage().is_some());
    adapter.begin_join();
    assert!(adapter.join_stage().is_none());
    adapter.disconnected();
    assert!(matches!(adapter.state(), AuthState::Failed(_)));
    assert!(adapter.profile().gamertag.is_empty());
    account(&mut adapter, "signed_in");
    assert_eq!(adapter.state(), AuthState::Authenticated);
    adapter.disconnected();
    account(&mut adapter, "signed_out");
    assert_eq!(adapter.state(), AuthState::SignedOut);
    assert!(adapter.operation_error().is_none());
}

#[test]
fn launcher_idempotent_sign_in_keeps_the_known_prompt_or_identity() {
    for state in ["awaiting_code", "signed_in"] {
        let mut adapter = RelayAccount::default();
        account(&mut adapter, state);
        let presented = adapter.state();
        adapter.request(AccountIntent::SignIn);
        assert_eq!(adapter.take_operation(), Some(AccountOperation::SignIn));
        assert_eq!(adapter.state(), presented);
        adapter.request(AccountIntent::SignIn);
        assert_eq!(adapter.state(), presented);
    }
}

#[test]
fn launcher_handoff_clears_progress_for_the_menu_generating_stage() {
    let mut adapter = RelayAccount::default();
    adapter.observe(&connect_event(
        json!({"stage":"packs", "packs_done":1, "packs_total":2}),
    ));
    assert!(adapter.join_stage().is_some());
    adapter.observe(&connect_event(json!({"stage":""})));
    assert!(adapter.join_stage().is_none());
}

#[test]
fn launcher_refused_sign_out_keeps_the_confirmed_account() {
    let mut adapter = RelayAccount::default();
    account(&mut adapter, "signed_in");
    adapter.request(AccountIntent::SignOut);
    adapter.take_operation();
    adapter.observe(&ControlEvent::RequestRefused(RequestRefusal::Refused(
        bridge::contract::RefusalCode::SessionBusy,
    )));
    assert_eq!(adapter.state(), AuthState::Authenticated);
    assert_eq!(adapter.profile().gamertag, "Fixture");
    assert!(adapter.operation_error().is_some());
}

#[test]
fn launcher_cancel_after_disconnect_retires_the_retained_flow() {
    for code_arrived in [false, true] {
        let mut adapter = RelayAccount::default();
        adapter.request(AccountIntent::SignIn);
        adapter.take_operation();
        if code_arrived {
            account(&mut adapter, "awaiting_code");
        }
        adapter.disconnected();
        adapter.request(AccountIntent::Cancel);
        assert_eq!(adapter.take_operation(), Some(AccountOperation::SignOut));
        account(&mut adapter, "awaiting_code");
        assert_eq!(adapter.state(), AuthState::SignedOut);
    }
}

#[test]
fn launcher_fresh_session_retires_old_flow_and_restores_only_unsent_intent() {
    let mut adapter = RelayAccount::default();
    adapter.request(AccountIntent::SignIn);
    adapter.take_operation();
    adapter.disconnected();
    adapter.fresh_session();
    account(&mut adapter, "signed_out");
    assert_eq!(adapter.state(), AuthState::SignedOut);
    adapter.request(AccountIntent::Cancel);
    assert_eq!(adapter.take_operation(), None);
    adapter.fresh_session();
    adapter.restore_operation(AccountOperation::SignIn);
    assert_eq!(adapter.state(), AuthState::Checking);
    adapter.request(AccountIntent::Cancel);
    assert_eq!(adapter.take_operation(), Some(AccountOperation::SignOut));
}

#[test]
fn launcher_coalesced_cancel_and_retry_keeps_the_unretired_prompt() {
    let mut adapter = RelayAccount::default();
    account(&mut adapter, "awaiting_code");
    let prompt = adapter.state();
    adapter.request(AccountIntent::Cancel);
    assert_eq!(adapter.state(), AuthState::SignedOut);
    adapter.request(AccountIntent::SignIn);
    assert_eq!(adapter.take_operation(), Some(AccountOperation::SignIn));
    assert_eq!(adapter.state(), prompt);
}

#[test]
fn launcher_retry_refusal_survives_a_delayed_cancellation_snapshot() {
    let mut adapter = RelayAccount::default();
    account(&mut adapter, "awaiting_code");
    adapter.request(AccountIntent::Cancel);
    adapter.take_operation();
    adapter.request(AccountIntent::SignIn);
    adapter.take_operation();
    adapter.observe(&ControlEvent::RequestRefused(RequestRefusal::RetrySignIn));
    account(&mut adapter, "signed_out");
    assert!(matches!(adapter.state(), AuthState::Failed(_)));
    adapter.request(AccountIntent::SignIn);
    assert_eq!(adapter.state(), AuthState::Checking);
}

#[test]
fn launcher_rejects_unsafe_or_incomplete_prompts() {
    let mut adapter = RelayAccount::default();
    for uri in [
        "javascript:alert(1)",
        "http://example.invalid",
        "https://name:secret@example.invalid",
    ] {
        adapter.observe(&account_event(
            json!({"state":"awaiting_code", "verification_uri":uri, "user_code":"TEST"}),
        ));
        assert!(matches!(adapter.state(), AuthState::Failed(_)));
    }
    adapter.observe(&account_event(json!({"state":"awaiting_code", "verification_uri":"https://example.invalid", "user_code":"TEST&other=value"})));
    assert!(matches!(adapter.state(), AuthState::Failed(_)));
}

#[test]
fn launcher_addressed_servers_get_default_port_and_other_targets_are_refused() {
    let port = bridge::contract::DEFAULT_BEDROCK_PORT;
    for (input, expected) in [
        ("example.invalid", format!("example.invalid:{port}")),
        ("[2001:db8::1]", format!("[2001:db8::1]:{port}")),
        ("2001:db8::1", format!("[2001:db8::1]:{port}")),
        ("example.invalid:20000", "example.invalid:20000".into()),
    ] {
        assert_eq!(addressed_target(input), Ok(ConnectTarget::RakNet(expected)));
    }
    for input in [
        "",
        "realm_id/1",
        "friend/1",
        "gathering/1",
        "example.invalid:0",
        "example.invalid:99999",
        "example.invalid:abc",
        "https://example.invalid",
        "name@example.invalid",
        "[bad]",
        "a b",
    ] {
        assert!(addressed_target(input).is_err(), "{input}");
    }
}

#[test]
fn launcher_preferences_round_trip_and_bound_untrusted_storage() {
    let mut preferences = Preferences {
        gui_scale_offset: 1,
        servers: vec![::launcher::menu::view::SavedServer {
            name: "Fixture".into(),
            address: "example.invalid".into(),
            favorite: true,
            last_joined_unix: 0,
        }],
    };
    assert_eq!(
        Preferences::decode(&preferences.encode().unwrap()).unwrap(),
        preferences
    );
    assert!(Preferences::decode(&" ".repeat(MAX_PREFERENCE_BYTES + 1)).is_err());
    assert!(
        Preferences::decode("{\"gui_scale_offset\":1,\"servers\":[],\"token\":\"secret\"}")
            .is_err()
    );
    preferences.servers = vec![preferences.servers[0].clone(); MAX_SERVERS + 1];
    assert!(preferences.encode().is_err());
    preferences.servers.clear();
    preferences
        .servers
        .push(::launcher::menu::view::SavedServer {
            name: String::new(),
            address: "example.invalid".into(),
            favorite: false,
            last_joined_unix: 0,
        });
    assert!(preferences.encode().is_err());
}
