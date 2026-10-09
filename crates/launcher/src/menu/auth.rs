//! Account status reported to launcher screens.

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthState {
    SignedOut,
    Checking,
    AwaitingCode { uri: String, code: String },
    AwaitingXboxSignup { uri: String },
    Authenticated,
    Failed(String),
}

impl AuthState {
    /// Whether Microsoft sign-in or Xbox profile creation is waiting on the browser.
    pub fn awaiting_browser(&self) -> bool {
        matches!(
            self,
            Self::AwaitingCode { .. } | Self::AwaitingXboxSignup { .. }
        )
    }
}

/// An active helper owns its prompt; the launcher core owns completed account status.
pub fn select_auth<'a>(
    supervisor: Option<&'a AuthState>,
    control: Option<&'a AuthState>,
) -> &'a AuthState {
    match (supervisor, control) {
        (
            Some(
                state @ (AuthState::Checking
                | AuthState::AwaitingCode { .. }
                | AuthState::AwaitingXboxSignup { .. }
                | AuthState::Failed(_)),
            ),
            _,
        ) => state,
        (_, Some(state)) | (Some(state), None) => state,
        _ => &AuthState::SignedOut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_and_failed_prompts_outrank_stale_core_status() {
        for prompt in [
            AuthState::Checking,
            AuthState::AwaitingCode {
                uri: "https://example.invalid".into(),
                code: "TEST-CODE".into(),
            },
            AuthState::AwaitingXboxSignup {
                uri: "https://sisu.xboxlive.com/signup".into(),
            },
            AuthState::Failed("Try again.".into()),
        ] {
            for core in [AuthState::SignedOut, AuthState::Authenticated] {
                assert_eq!(select_auth(Some(&prompt), Some(&core)), &prompt);
            }
        }
        assert_eq!(
            select_auth(Some(&AuthState::Authenticated), Some(&AuthState::SignedOut)),
            &AuthState::SignedOut
        );
        assert_eq!(
            select_auth(Some(&AuthState::Authenticated), None),
            &AuthState::Authenticated
        );
        assert_eq!(select_auth(None, None), &AuthState::SignedOut);
    }
}
