//! Host-owned consent status for server experiences.

use server_experience::session::State;

/// Builds plain trusted text; pack data can only fill labeled values. Only an unanswered
/// offer prompts, over whatever is on screen: the join holds its loading screen meanwhile.
pub fn chrome(session: &server_experience::session::Session) -> (Option<String>, bool) {
    let text = match &session.state {
        State::Inert | State::Disabled => return (None, false),
        State::Offered(offer) => {
            let packages = offer
                .offer
                .packages
                .iter()
                .map(|package| {
                    format!(
                        "{} ({} bytes)\nPublisher: {}",
                        package.id, package.bytes, package.publisher_key,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let identity = if session.key_changed {
                "Server key changed: new approval required."
            } else {
                "First-use key pinning does not verify the operator's identity."
            };
            format!(
                concat!(
                    "Server: {}\nKey: {}\n{}\n{}\n",
                    "Permissions: {:?}\nMemory limit: {} bytes; GPU limit: {} bytes\n",
                    "Media/download hosts: {}\nThese hosts see your IP address.\n",
                    "Fallback: {}\nServer code is untrusted. F9 disables it immediately.",
                ),
                offer.offer.audience,
                offer.offer.server_key,
                identity,
                packages,
                offer.offer.scope.permissions,
                offer.offer.scope.memory_bytes,
                offer.offer.scope.gpu_bytes,
                offer
                    .offer
                    .scope
                    .origins
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                offer.offer.fallback,
            )
        }
        State::Awaiting(_) => "Cinnabar: verifying server experience. F9: disable".into(),
        State::Granted(_) => session.notice.clone().unwrap_or_else(|| {
            "Cinnabar: experience approved; runtime unavailable. F9: disable".into()
        }),
    };
    (Some(text), matches!(session.state, State::Offered(_)))
}

/// Leaves all ordinary input untouched when there is no offered experience.
pub fn can_disable(state: &State) -> bool {
    matches!(
        state,
        State::Offered(_) | State::Awaiting(_) | State::Granted(_)
    )
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn vanilla_session_does_not_claim_disable_key_or_draw_chrome() {
        let session = server_experience::session::Session::default();
        assert!(!can_disable(&session.state));
        assert_eq!(chrome(&session), (None, false));
        assert!(!can_disable(&State::Disabled));
    }
}
