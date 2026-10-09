use super::*;
use launcher::menu::realm_membership::Stage;

/// Builds a signed-in launcher on the Realms tab without an account service.
fn menu() -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.control_auth = Some(AuthState::Authenticated);
    menu.enter(MenuScreen::Social);
    menu
}

/// Returns a synthetic invitation preview with a stable Realm target.
fn realm() -> MenuRealmCard {
    MenuRealmCard {
        name: "Fixture Realm".into(),
        target: "realm_id/7".into(),
        state: "OPEN".into(),
        address: String::new(),
        owner: "Fixture Owner".into(),
        online_players: 0,
        max_players: 10,
        days_left: 0,
        expired: false,
        member: true,
    }
}

#[test]
fn realm_membership_requires_preview_then_confirmation_before_publishing() {
    let mut menu = menu();
    menu.activate_realm_membership(Action::Open);
    menu.edit_text("https://realms.gg/fixture-code");
    menu.activate_realm_membership(Action::Accept);
    assert!(menu.realm_membership.request.is_none());
    menu.activate_realm_membership(Action::Verify);
    let ticket = menu.realm_membership.ticket;
    menu.receive_realm_membership(ticket, false, Ok(("fixture-code".into(), realm())));
    assert_eq!(
        menu.realm_membership.state.as_ref().unwrap().stage,
        Stage::Confirm
    );
    assert!(menu.realms.is_empty());
    menu.activate_realm_membership(Action::Accept);
    let ticket = menu.realm_membership.ticket;
    menu.receive_realm_membership(ticket, true, Ok(("fixture-code".into(), realm())));
    assert_eq!(
        menu.realm_membership.state.as_ref().unwrap().stage,
        Stage::Complete
    );
    assert_eq!(menu.realms, [realm()]);
    assert!(!menu.is_connecting());
}

#[test]
fn realm_membership_back_cancels_preview_and_discards_its_late_response() {
    let mut menu = menu();
    menu.activate_realm_membership(Action::Open);
    menu.edit_text("fixture-code");
    menu.activate_realm_membership(Action::Verify);
    let ticket = menu.realm_membership.ticket;
    menu.go_back();
    assert!(menu.realm_membership.state.is_none());
    assert!(menu.realm_membership.cancel);
    menu.activate_realm_membership(Action::Open);
    menu.receive_realm_membership(ticket, false, Ok(("fixture-code".into(), realm())));
    assert_eq!(
        menu.realm_membership.state.as_ref().unwrap().stage,
        Stage::Code
    );
    assert!(menu.realms.is_empty());
}

#[test]
fn realm_membership_errors_retry_and_acceptance_cannot_be_dismissed() {
    let mut menu = menu();
    menu.activate_realm_membership(Action::Open);
    menu.edit_text("fixture-code");
    menu.activate_realm_membership(Action::Verify);
    let ticket = menu.realm_membership.ticket;
    menu.receive_realm_membership(ticket, false, Err(()));
    assert!(menu.realm_membership.state.as_ref().unwrap().can_verify());
    assert!(
        menu.realm_membership
            .state
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
    menu.activate_realm_membership(Action::Verify);
    let ticket = menu.realm_membership.ticket;
    menu.receive_realm_membership(ticket, false, Ok(("fixture-code".into(), realm())));
    menu.activate_realm_membership(Action::Accept);
    menu.go_back();
    assert_eq!(
        menu.realm_membership.state.as_ref().unwrap().stage,
        Stage::Joining
    );
    let ticket = menu.realm_membership.ticket;
    menu.receive_realm_membership(ticket, true, Err(()));
    assert_eq!(
        menu.realm_membership.state.as_ref().unwrap().stage,
        Stage::Confirm
    );
    assert!(menu.realms.is_empty());
}

#[test]
fn realm_membership_signed_out_open_and_late_acceptance_are_rejected() {
    let mut menu = menu();
    menu.control_auth = Some(AuthState::SignedOut);
    menu.activate_realm_membership(Action::Open);
    assert!(menu.realm_membership.state.is_none());
    menu.activate_realm_membership(Action::Open);
    let ticket = menu.realm_membership.ticket;
    menu.control_auth = Some(AuthState::SignedOut);
    menu.receive_realm_membership(ticket, true, Ok(("fixture-code".into(), realm())));
    assert!(menu.realms.is_empty());
}
