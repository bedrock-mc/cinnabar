//! Fixed sign-in presentation for hidden developer captures, without account services.

use developer_control::{
    ENDPOINT_ENV, HIDDEN_WINDOW_ENV, SIGN_IN_FIXTURE_ENV, protocol::SignInFixtureState,
};

use super::{AuthState, MenuAction, MenuDialog, MenuRuntime, MenuScreen, MenuView};

const PLACEHOLDER_CODE: &str = "TEST-CODE";
const PLACEHOLDER_URI: &str = "https://example.invalid/sign-in";

/// Fixture startup requires both a hidden surface and the developer endpoint.
pub(super) fn startup() -> Option<SignInFixtureState> {
    if std::env::var_os(HIDDEN_WINDOW_ENV).is_none_or(|value| value != "1")
        || std::env::var_os(ENDPOINT_ENV).is_none()
    {
        return None;
    }
    let value = std::env::var(SIGN_IN_FIXTURE_ENV).ok()?;
    serde_json::from_value(serde_json::Value::String(value)).ok()
}

/// Fixture prompts contain only controlled copy and a fixed non-service URL.
pub(super) fn auth_state(state: SignInFixtureState) -> AuthState {
    match state {
        SignInFixtureState::Waiting => AuthState::Checking,
        SignInFixtureState::Opened | SignInFixtureState::BrowserFailed => AuthState::AwaitingCode {
            uri: PLACEHOLDER_URI.into(),
            code: PLACEHOLDER_CODE.into(),
        },
        SignInFixtureState::Success => AuthState::Authenticated,
        SignInFixtureState::Expired => {
            AuthState::Failed("Your sign-in code expired. Try again.".into())
        }
        SignInFixtureState::Error => AuthState::Failed("Sign-in could not complete.".into()),
    }
}

impl MenuRuntime {
    /// Fixture captures bypass authentication, account persistence, and browser actions.
    pub(super) fn fixture_active(&self) -> bool {
        self.sign_in_fixture.is_some()
    }

    /// Reports only the fixed capture state, without account or device-code data.
    pub(crate) fn sign_in_fixture_state(&self) -> Option<SignInFixtureState> {
        self.sign_in_fixture
    }

    /// A running account session cannot be switched into the placeholder fixture.
    pub(crate) fn apply_sign_in_fixture(
        &mut self,
        state: SignInFixtureState,
    ) -> Result<(), String> {
        if !self.fixture_active() {
            return Err(format!(
                "launch a hidden client with {SIGN_IN_FIXTURE_ENV} first"
            ));
        }
        self.sign_in_fixture = Some(state);
        self.dialog = Some(MenuDialog::Accounts);
        self.feeds.account_adding = true;
        self.sign_in_requested = true;
        self.visible = true;
        self.focused = 0;
        Ok(())
    }

    /// Starts from empty fixture feeds so saved identities never enter a capture.
    pub(super) fn fixture_view(&self) -> Option<MenuView> {
        use launcher::menu::sign_in::BrowserState;

        let state = self.sign_in_fixture?;
        let mut view = MenuView::new(true, "Offline Player".into());
        view.screen = MenuScreen::Home;
        view.dialog = self.dialog;
        let prompt = self.dialog == Some(MenuDialog::Accounts);
        view.feeds.account_adding = prompt;
        view.sign_in_requested = prompt;
        view.auth_state = if prompt {
            auth_state(state)
        } else {
            AuthState::SignedOut
        };
        view.sign_in_browser = match state {
            SignInFixtureState::Opened => BrowserState::Opened,
            SignInFixtureState::BrowserFailed => BrowserState::Failed,
            _ => BrowserState::Waiting,
        };
        view.hovered = self.hovered;
        view.pressed = self.pressed;
        view.focused_action = self.focus_actions().get(self.focused).copied();
        view.navigation_focus_visible = self.input_mode.navigation();
        view.gamepad_input = self.input_mode.gamepad();
        Some(view)
    }

    /// Fixture controls change presentation only; they never invoke account services.
    pub(super) fn activate_sign_in_fixture(&mut self, action: MenuAction) -> bool {
        if !self.fixture_active() {
            return false;
        }
        match action {
            MenuAction::OpenSignInLink => {
                let _ = self.apply_sign_in_fixture(SignInFixtureState::Opened);
            }
            MenuAction::StartSignIn | MenuAction::AddAccount => {
                let _ = self.apply_sign_in_fixture(SignInFixtureState::Waiting);
            }
            MenuAction::CancelSignIn | MenuAction::CloseSignIn | MenuAction::DismissDialog => {
                self.dialog = None;
                self.feeds.account_adding = false;
                self.sign_in_requested = false;
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_session_cannot_enable_fixture_commands() {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.sign_in_fixture = None;
        assert!(
            menu.apply_sign_in_fixture(SignInFixtureState::Opened)
                .is_err()
        );
        assert!(!menu.activate_sign_in_fixture(MenuAction::StartSignIn));
    }

    #[test]
    fn fixture_views_omit_account_content_and_actions_spawn_nothing() {
        let mut menu = MenuRuntime::new(true, 2, "Saved identity".into());
        menu.sign_in_fixture = Some(SignInFixtureState::Opened);
        menu.apply_sign_in_fixture(SignInFixtureState::Opened)
            .unwrap();
        menu.feeds
            .accounts
            .push(launcher::accounts::AccountProfile {
                id: "private-identity".into(),
                gamertag: "Private player".into(),
                picture_path: None,
            });
        let view = menu.fixture_view().unwrap();
        assert!(view.feeds.accounts.is_empty());
        assert_eq!(view.display_name, "Offline Player");
        assert!(
            matches!(view.auth_state, AuthState::AwaitingCode { code, .. } if code == PLACEHOLDER_CODE)
        );
        assert!(menu.activate_sign_in_fixture(MenuAction::StartSignIn));
        assert!(menu.auth_process.is_none());
        assert!(menu.accounts.operation.is_none());
        assert_eq!(menu.sign_in_fixture, Some(SignInFixtureState::Waiting));
    }

    #[test]
    fn cancelling_a_fixture_dismisses_the_prompt_without_services() {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.sign_in_fixture = Some(SignInFixtureState::Opened);
        menu.apply_sign_in_fixture(SignInFixtureState::Opened)
            .unwrap();
        menu.activate_sign_in_fixture(MenuAction::CancelSignIn);
        let view = menu.fixture_view().unwrap();
        assert!(!view.popup_open());
        assert_eq!(view.auth_state, AuthState::SignedOut);
        assert!(!menu.focus_actions().contains(&MenuAction::OpenSignInLink));
        assert!(menu.auth_process.is_none());
        assert!(menu.accounts.operation.is_none());
    }

    #[test]
    fn fixture_focus_and_retry_follow_the_live_sign_in_controls() {
        let mut menu = MenuRuntime::new(true, 2, "Offline Player".into());
        menu.sign_in_fixture = Some(SignInFixtureState::Opened);
        menu.apply_sign_in_fixture(SignInFixtureState::Opened)
            .unwrap();
        assert_eq!(
            menu.fixture_view().unwrap().focused_action,
            Some(MenuAction::OpenSignInLink)
        );
        menu.move_focus(1);
        assert_eq!(
            menu.fixture_view().unwrap().focused_action,
            Some(MenuAction::CancelSignIn)
        );
        menu.move_focus(-1);
        menu.activate_focused();
        assert_eq!(menu.sign_in_fixture, Some(SignInFixtureState::Opened));
        menu.apply_sign_in_fixture(SignInFixtureState::Expired)
            .unwrap();
        assert_eq!(
            menu.fixture_view().unwrap().focused_action,
            Some(MenuAction::StartSignIn)
        );
        menu.activate_focused();
        assert_eq!(menu.sign_in_fixture, Some(SignInFixtureState::Waiting));
        assert!(menu.auth_process.is_none());
        assert!(menu.accounts.operation.is_none());
    }
}
