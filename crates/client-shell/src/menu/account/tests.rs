use super::*;
use launcher::menu::{MenuAction, MenuDialog};

#[test]
fn new_account_prompts_take_primary_focus_and_repeats_keep_cancel_selected() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    let code = |value: &str| AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: value.into(),
    };
    menu.focused = 7;
    menu.apply_control_auth(code("FIRST"));
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
    menu.move_focus(1);
    menu.apply_control_auth(code("FIRST"));
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
    menu.apply_control_auth(code("SECOND"));
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
    menu.move_focus(1);
    menu.apply_control_auth(AuthState::Failed("Try again.".into()));
    assert_eq!(menu.view().focused_action, Some(MenuAction::StartSignIn));
}

#[test]
fn completed_add_account_control_preserves_cancel_focus() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.dialog = Some(MenuDialog::Accounts);
    menu.feeds.account_adding = true;
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    menu.move_focus(1);
    assert_eq!(menu.focused, 1);
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
    menu.apply_control_auth(AuthState::Authenticated);
    assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
}

#[test]
fn failed_helper_start_survives_signed_out_account_status() {
    assert_failed_start_prompt_survives(AuthState::SignedOut);
}

#[test]
fn failed_helper_start_survives_authenticated_account_status() {
    assert_failed_start_prompt_survives(AuthState::Authenticated);
}

/// Account status cannot dismiss a supervisor-start failure or restore an unfinished Add account.
fn assert_failed_start_prompt_survives(status: AuthState) {
    for adding in [false, true] {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.feeds.account_adding = adding;
        menu.dialog = adding.then_some(MenuDialog::Accounts);
        start_sign_in_without_attached_services(&mut menu);
        let failure = menu.current_auth().into_owned();
        assert!(matches!(failure, AuthState::Failed(_)));
        menu.apply_control_auth(status.clone());
        menu.poll_accounts();
        assert_eq!(menu.current_auth().as_ref(), &failure);
        assert!(menu.sign_in_requested);
        assert_eq!(menu.view().focused_action, Some(MenuAction::StartSignIn));
        assert!(menu.accounts.operation.is_none());
        assert_eq!(menu.control_auth.as_ref(), Some(&status));
        let hidden_prompt = AuthState::AwaitingCode {
            uri: "https://example.invalid".into(),
            code: "HIDDEN-CODE".into(),
        };
        menu.apply_control_auth(hidden_prompt.clone());
        menu.update_sign_in_browser(false);
        assert_eq!(
            menu.sign_in_browser.state(&hidden_prompt),
            launcher::menu::sign_in::BrowserState::Waiting
        );
        menu.apply_control_auth(status.clone());
        menu.stop_sign_in();
        assert!(!menu.sign_in_requested);
        assert_eq!(menu.current_auth().as_ref(), &status);
        start_sign_in_without_attached_services(&mut menu);
        assert!(matches!(menu.current_auth().as_ref(), AuthState::Failed(_)));
    }
}

/// Advances an Add account through its service owner when no original services are attached.
fn start_sign_in_without_attached_services(menu: &mut MenuRuntime) {
    menu.start_sign_in();
    #[cfg(feature = "native-networking")]
    if menu.feeds.account_adding {
        use bevy::{ecs::world::CommandQueue, prelude::*};

        let mut world = World::new();
        let mut queue = CommandQueue::default();
        super::super::launcher_services::LauncherServices::default().drive(
            &mut Commands::new(&mut queue, &world),
            menu,
            true,
            None,
        );
        queue.apply(&mut world);
    }
}

#[test]
fn cancelled_account_prompts_do_not_return_on_repeated_status() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    let code = AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    };
    menu.apply_control_auth(code.clone());
    menu.activate(MenuAction::CancelSignIn);
    menu.focused = 1;
    for state in [code, AuthState::Failed("Try again.".into())] {
        menu.apply_control_auth(state);
        menu.update_sign_in_browser(false);
        assert_eq!(menu.focused, 1);
        assert_eq!(menu.view().auth_state, AuthState::SignedOut);
        assert!(!menu.view().popup_open());
    }
    menu.apply_control_auth(AuthState::Authenticated);
    assert_eq!(menu.view().auth_state, AuthState::Authenticated);
    menu.start_sign_in();
    assert!(!menu.sign_in_cancelled);
    assert!(menu.sign_in_requested);
}

#[test]
fn cached_validation_keeps_home_focus_until_a_device_code_arrives() {
    let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
    menu.focused = 1;
    for state in [AuthState::Checking, AuthState::Failed("Try again.".into())] {
        menu.apply_control_auth(state);
        assert_eq!(menu.focused, 1);
        assert!(!menu.view().sign_in_prompt_open());
        assert!(menu.sign_in_focus().is_none());
    }
    menu.apply_control_auth(AuthState::AwaitingCode {
        uri: "https://example.invalid".into(),
        code: "TEST-CODE".into(),
    });
    assert!(menu.view().sign_in_prompt_open());
    assert_eq!(menu.view().focused_action, Some(MenuAction::OpenSignInLink));
}

#[test]
fn only_validated_authentication_selects_the_cache_for_a_connection() {
    let layout = launcher::test_support::checkout();
    assert_eq!(validated_auth_cache(&layout, None), None);
    assert_eq!(
        validated_auth_cache(&layout, Some(&AuthState::SignedOut)),
        None
    );
    assert_eq!(
        validated_auth_cache(&layout, Some(&AuthState::Checking)),
        None
    );
    let failed = AuthState::Failed("validation failed".to_owned());
    assert_eq!(validated_auth_cache(&layout, Some(&failed)), None);
    assert_eq!(
        validated_auth_cache(&layout, Some(&AuthState::Authenticated)),
        Some(layout.auth_cache())
    );
}

#[test]
fn xbox_signup_owns_focus_and_can_be_cancelled() {
    use {crate::menu::MenuRuntime, launcher::menu::MenuAction};
    let mut menu = MenuRuntime::new(true, 2, "Fixture Player".into());
    menu.apply_control_auth(AuthState::AwaitingXboxSignup {
        uri: "https://sisu.xboxlive.com/signup?signature=fixture".into(),
    });
    assert!(menu.view().sign_in_prompt_open());
    assert_eq!(
        menu.sign_in_focus(),
        Some(vec![MenuAction::OpenSignInLink, MenuAction::CancelSignIn])
    );
    menu.activate(MenuAction::CancelSignIn);
    assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
    assert!(!menu.view().sign_in_prompt_open());
}
