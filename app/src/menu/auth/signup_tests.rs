use super::*;

#[test]
fn xbox_signup_resumes_cached_and_device_sign_in() {
    for device in [false, true] {
        let mut state = AuthState::Checking;
        let mut terminal = false;
        let mut seen = false;
        apply_event(
            &mut state,
            &mut terminal,
            &mut seen,
            br#"{"v":1,"event":"checking_cache"}"#,
        );
        if device {
            apply_event(&mut state, &mut terminal, &mut seen, br#"{"v":1,"event":"device_code","verification_uri":"https://example.test/device","user_code":"TEST"}"#);
        }
        apply_event(&mut state, &mut terminal, &mut seen, br#"{"v":1,"event":"xbox_signup","signup_url":"https://sisu.xboxlive.com/signup?signature=fixture"}"#);
        assert!(matches!(state, AuthState::AwaitingXboxSignup { .. }));
        assert!(!terminal);
        let success = if device {
            br#"{"v":1,"event":"authenticated","method":"device_code"}"#.as_slice()
        } else {
            br#"{"v":1,"event":"authenticated","method":"cached"}"#.as_slice()
        };
        apply_event(&mut state, &mut terminal, &mut seen, success);
        assert_eq!(state, AuthState::Authenticated);
        assert!(terminal);
    }
}

#[test]
fn unsafe_or_premature_xbox_signup_is_rejected() {
    for (seen, url) in [
        (false, "https://sisu.xboxlive.com/signup"),
        (true, "file:///private"),
        (true, "https://user:pass@example.test/signup"),
    ] {
        let mut state = AuthState::Checking;
        let mut terminal = false;
        let mut seen = seen;
        let line = format!(r#"{{"v":1,"event":"xbox_signup","signup_url":"{url}"}}"#);
        apply_event(&mut state, &mut terminal, &mut seen, line.as_bytes());
        assert!(matches!(state, AuthState::Failed(_)));
        assert!(terminal);
    }
}

#[test]
fn xbox_signup_owns_focus_and_can_be_cancelled() {
    use super::super::{MenuAction, MenuRuntime};
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
