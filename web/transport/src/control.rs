//! Typed relay messages shared by transport selection and page presentation.
use crate::{Channel, CloseReason};
use bridge::contract::*;

use bridge::relay_control::{Event, Hello};

/// Builds the browser's declaration using its already-selected reliable channel.
pub(crate) fn hello(channel: Channel, resume: &str, generation: u64) -> Hello {
    Hello {
        version: RELAY_VERSION,
        channel: channel.name().into(),
        channels: if matches!(channel, Channel::Control) {
            [RELAY_CHANNEL_CONTROL, RELAY_CHANNEL_PLAY]
                .map(str::to_owned)
                .into()
        } else {
            Vec::new()
        },
        resume: resume.into(),
        generation,
        join: match channel {
            Channel::Control => 0,
            Channel::Play { join } => join,
        },
    }
}
/// Parses one bounded frame, retaining optional-field forward compatibility.
pub(crate) fn parse(frame: &[u8]) -> Result<Event, CloseReason> {
    serde_json::from_slice(frame).map_err(|_| CloseReason::Protocol)
}

/// Explains a failed control handshake without exposing provider or transport error strings.
pub fn connection_refusal_message(code: RefusalCode) -> &'static str {
    match code {
        RefusalCode::ResumeRefused => {
            "Session expired or refused. Start a new session to sign in again."
        }
        RefusalCode::AdmissionRefused => {
            "The relay cannot accept another connection right now. Use Connect relay to retry."
        }
        RefusalCode::SessionBusy => {
            "This session is already in use. Retry when the other connection closes."
        }
        RefusalCode::Refused => "The relay refused this request.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge::relay_control::Request;
    use serde_json::Value;

    #[test]
    fn shared_go_fixtures_round_trip_through_the_typed_contract() {
        let contract: Value =
            serde_json::from_str(include_str!("../../../crates/bridge/data/contract.json"))
                .unwrap();
        for fixture in contract["relay"]["fixtures"].as_array().unwrap() {
            let encoded = if fixture.get("type").is_some() {
                serde_json::to_value(serde_json::from_value::<Event>(fixture.clone()).unwrap())
                    .unwrap()
            } else if fixture.get("op").is_some() {
                serde_json::to_value(serde_json::from_value::<Request>(fixture.clone()).unwrap())
                    .unwrap()
            } else {
                serde_json::to_value(serde_json::from_value::<Hello>(fixture.clone()).unwrap())
                    .unwrap()
            };
            assert_eq!(&encoded, fixture);
        }
        assert_eq!(
            parse(br#"{"type":"error","error":"future_private_detail"}"#).unwrap(),
            Event::Error {
                error: RefusalCode::Refused
            }
        );
        assert_eq!(
            parse(br#"{"type":"optional","anything":1}"#).unwrap(),
            Event::Unknown
        );
        assert!(parse(br#"{"type":"account","account":{}}"#).is_err());
    }
}
