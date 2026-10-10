use std::fs;
use launcher::menu::auth::AuthState;
use launcher_host::auth::test_support::{event_child_holding, supervisor};

    #[test]
    fn retry_waits_for_helper_cleanup_with_a_preparing_prompt() {
        use {crate::menu::MenuRuntime, launcher::menu::MenuAction};
        let (child, directory) = event_child_holding(&[]);
        let supervisor = supervisor(child, AuthState::Failed("Try again.".into()), true);
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.auth_process = Some(supervisor);
        menu.sign_in_requested = true;
        menu.control_auth = Some(AuthState::Authenticated);
        menu.start_sign_in();
        assert!(menu.auth_restart_requested);
        assert_eq!(menu.view().auth_state, AuthState::Checking);
        assert_eq!(menu.view().focused_action, Some(MenuAction::CancelSignIn));
        drop(menu);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_failed_prompt_survives_stale_core_status_until_explicit_cancel() {
        use {crate::menu::MenuRuntime, launcher::menu::MenuAction};
        let (child, directory) = event_child_holding(&[]);
        let supervisor = supervisor(child, AuthState::Failed("Your sign-in code expired. Try again.".into()), true);
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.auth_process = Some(supervisor);
        menu.sign_in_requested = true;
        menu.control_auth = Some(AuthState::SignedOut);
        assert!(matches!(menu.view().auth_state, AuthState::Failed(_)));
        assert_eq!(menu.view().focused_action, Some(MenuAction::StartSignIn));
        menu.activate(MenuAction::CancelSignIn);
        assert_eq!(menu.view().auth_state, AuthState::SignedOut);
        drop(menu);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancelled_completed_add_account_does_not_validate_a_missing_main_cache() {
        use {
            crate::menu::MenuRuntime,
            launcher::menu::{MenuAction, MenuDialog},
        };
        let (child, directory) = event_child_holding(&[]);
        let supervisor = supervisor(child, AuthState::Authenticated, true);
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.auth_process = Some(supervisor);
        menu.feeds.account_adding = true;
        menu.accounts.pending_ready = true;
        menu.dialog = Some(MenuDialog::Accounts);
        menu.activate(MenuAction::CancelSignIn);
        menu.account_operation_job()();
        menu.poll_accounts();
        assert!(menu.auth_process.is_none());
        assert!(menu.launcher_auth_cache().is_none());
        assert_eq!(menu.current_auth().as_ref(), &AuthState::SignedOut);
        drop(menu);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancelled_cache_validation_cannot_offer_another_device_code() {
        use crate::menu::MenuRuntime;
        let (child, directory) = event_child_holding(&[]);
        let supervisor = supervisor(child, AuthState::AwaitingCode {
            uri: "https://example.invalid".into(),
            code: "TEST-CODE".into(),
        }, false);
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.auth_process = Some(supervisor);
        menu.auth_attempted = true;
        menu.sign_in_cancelled = true;
        menu.poll_catalog(true);
        let supervisor = menu.auth_process.as_ref().unwrap();
        assert!(supervisor.test_cancel_requested());
        assert_eq!(supervisor.state(), &AuthState::SignedOut);
        assert!(!menu.view().popup_open());
        drop(menu);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn profile_sign_in_focus_outranks_stale_control_auth() {
        use {
            crate::menu::MenuRuntime,
            launcher::menu::{MenuAction, MenuScreen},
        };

        for state in [
            AuthState::Checking,
            AuthState::AwaitingCode {
                uri: "https://example.invalid/device".into(),
                code: "FIXTURE".into(),
            },
        ] {
            for control in [AuthState::SignedOut, AuthState::Authenticated] {
                let (child, directory) = event_child_holding(&[]);
                let supervisor = supervisor(child, state.clone(), false);
                let mut menu = MenuRuntime::new(true, 2, "Fixture Player".into());
                menu.screen = MenuScreen::Profile;
                menu.auth_process = Some(supervisor);
                menu.sign_in_requested = true;
                menu.control_auth = Some(control);
                menu.feeds.profile.loaded = true;
                menu.feeds.profile.friends = Some(1);
                assert_eq!(menu.view().auth_state, state);
                let actions = menu.focus_actions();
                let expected = if matches!(state, AuthState::AwaitingCode { .. }) {
                    vec![MenuAction::OpenSignInLink, MenuAction::CancelSignIn]
                } else {
                    vec![MenuAction::CancelSignIn]
                };
                assert_eq!(actions, expected);
                // The previous signed-out Profile action occupied index one.
                menu.focused = 1;
                menu.activate_focused();
                let supervisor = menu.auth_process.as_ref().unwrap();
                assert!(supervisor.test_cancel_requested());
                assert_eq!(supervisor.state(), &AuthState::SignedOut);
                assert!(!menu.auth_restart_requested);
                drop(menu);
                fs::remove_dir_all(directory).unwrap();
            }
        }
    }

    #[test]
    fn profile_sign_in_focus_outranks_stale_control_auth_and_resets_transition() {
        use {crate::menu::MenuRuntime, launcher::menu::MenuScreen};

        let (child, directory) = event_child_holding(&[]);
        let supervisor = supervisor(child, AuthState::Checking, false);
        let mut menu = MenuRuntime::new(true, 2, "Fixture Player".into());
        menu.screen = MenuScreen::Profile;
        menu.auth_process = Some(supervisor);
        menu.sign_in_requested = true;
        menu.focused = 1;
        menu.start_sign_in();
        assert_eq!(menu.focused, 0);
        assert!(menu.auth_restart_requested);
        assert!(menu.auth_process.as_ref().unwrap().test_cancel_requested());
        drop(menu);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn signed_out_profile_exposes_sign_in_without_gating_offline_servers() {
        use {
            crate::menu::MenuRuntime,
            launcher::menu::{MenuAction, MenuScreen},
        };

        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        menu.activate(MenuAction::Navigate(MenuScreen::Profile));
        let view = menu.view();
        assert_eq!(view.auth_state, AuthState::SignedOut);
        assert!(!view.catalog_loading);
        assert!(menu.focus_actions().contains(&MenuAction::StartSignIn));
        menu.activate(MenuAction::Navigate(MenuScreen::Servers));
        assert!(menu.focus_actions().contains(&MenuAction::PlayAddServer));
    }

