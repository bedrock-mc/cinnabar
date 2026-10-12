//! The JSON a web relay's control socket (`core/webrelay/socket.go`) exchanges with a browser:
//! events in text frames, one per frame, and the two operations a browser sends back.

use serde::Deserialize;

use crate::{Account, ConnectProgress};

/// One event the relay's control socket sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ControlEvent {
    /// The session this socket attached to, created or resumed; its ID is the browser's
    /// credential for `/v1/session`.
    Session(String),
    /// The session's sign-in state.
    Account(Account),
    /// The join the relay is preparing, or `None` once it has handed the session over.
    Connect(Option<ConnectProgress>),
    /// Why the relay refused the last request, such as `rate_limited` or `session_busy`.
    Refused(String),
}

/// An operation a browser asks of its relay session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlOp {
    /// Starts Microsoft device-code sign-in; the code arrives as an [`ControlEvent::Account`].
    SignIn,
    /// Signs out, cancelling a sign-in still waiting for its code.
    SignOut,
}

impl ControlOp {
    /// The request's text frame.
    pub(crate) const fn frame(self) -> &'static str {
        match self {
            Self::SignIn => r#"{"op":"sign_in"}"#,
            Self::SignOut => r#"{"op":"sign_out"}"#,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Wire {
    Session { id: String },
    Account { account: Account },
    Connect { connect: Option<serde_json::Value> },
    Error { error: String },
}

/// Parses one text frame; `None` for an event this client does not know or a malformed one.
pub(crate) fn parse_event(text: &str) -> Option<ControlEvent> {
    Some(match serde_json::from_str::<Wire>(text).ok()? {
        Wire::Session { id } => ControlEvent::Session(id),
        Wire::Account { account } => ControlEvent::Account(account),
        // The relay ends a join with an empty stage rather than leaving the object out.
        Wire::Connect { connect } => ControlEvent::Connect(
            connect
                .filter(|connect| {
                    connect
                        .get("stage")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|stage| !stage.is_empty())
                })
                .and_then(|connect| serde_json::from_value(connect).ok()),
        ),
        Wire::Error { error } => ControlEvent::Refused(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthState, ConnectStage};

    #[test]
    fn relay_control_events_parse_as_the_relay_sends_them() {
        assert_eq!(
            parse_event(r#"{"type":"session","id":"ab12"}"#),
            Some(ControlEvent::Session("ab12".into()))
        );
        let Some(ControlEvent::Account(account)) = parse_event(
            r#"{"type":"account","account":{"state":"awaiting_code","verification_uri":"https://microsoft.com/link","user_code":"ABCD1234"}}"#,
        ) else {
            panic!("account event");
        };
        assert_eq!(account.state, AuthState::AwaitingCode);
        assert_eq!(account.user_code.as_deref(), Some("ABCD1234"));
        let Some(ControlEvent::Connect(Some(progress))) = parse_event(
            r#"{"type":"connect","connect":{"stage":"packs","packs_done":1,"packs_total":3,"received_bytes":10,"total_bytes":50}}"#,
        ) else {
            panic!("connect event");
        };
        assert_eq!(progress.stage, ConnectStage::Packs);
        assert_eq!(progress.total_bytes, 50);
        assert_eq!(
            parse_event(r#"{"type":"connect","connect":{"stage":""}}"#),
            Some(ControlEvent::Connect(None))
        );
        assert_eq!(
            parse_event(r#"{"type":"error","error":"session_busy"}"#),
            Some(ControlEvent::Refused("session_busy".into()))
        );
        assert_eq!(parse_event(r#"{"type":"later_event"}"#), None);
    }
}
