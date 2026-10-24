//! Account status reported to launcher screens.

/// An interactive account request consumed by the host that owns sign-in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountIntent {
    SignIn,
    Cancel,
    SignOut,
}

#[derive(Clone, Eq, PartialEq)]
pub enum AuthState {
    SignedOut,
    Checking,
    AwaitingCode { uri: String, code: String },
    AwaitingXboxSignup { uri: String },
    Authenticated,
    Failed(String),
}

impl std::fmt::Debug for AuthState {
    /// The hosted profile URL authenticates its browser and is never a diagnostic field.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignedOut => formatter.write_str("SignedOut"),
            Self::Checking => formatter.write_str("Checking"),
            Self::AwaitingCode { uri, code } => formatter
                .debug_struct("AwaitingCode")
                .field("uri", uri)
                .field("code", code)
                .finish(),
            Self::AwaitingXboxSignup { .. } => formatter
                .debug_struct("AwaitingXboxSignup")
                .finish_non_exhaustive(),
            Self::Authenticated => formatter.write_str("Authenticated"),
            Self::Failed(message) => formatter.debug_tuple("Failed").field(message).finish(),
        }
    }
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
        let signed = AuthState::AwaitingXboxSignup {
            uri: "https://example.invalid/signup?sig=private-signature".into(),
        };
        assert!(!format!("{signed:?}").contains("private-signature"));
    }
}
