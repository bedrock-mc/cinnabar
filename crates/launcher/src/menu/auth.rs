//! Account status reported to launcher screens.

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthState {
    SignedOut,
    Checking,
    AwaitingCode { uri: String, code: String },
    Authenticated,
    Failed(String),
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
