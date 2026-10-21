//! Typed relay messages shared by transport selection and page presentation.
use crate::{Channel, CloseReason};
use bridge::{Account, ConnectProgress, contract::*};
use serde::{Deserialize, Serialize};

/// One channel prefix, with credentials carried only inside its bounded frame.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Hello {
    pub version: u32,
    pub channel: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resume: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub generation: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub join: u64,
}
impl Hello {
    /// Builds the channel declaration from the authoritative bridge contract.
    pub(crate) fn new(channel: Channel, resume: &str, generation: u64) -> Self {
        Self {
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
}

/// A rotated capability and its control generation, observed after a bounded frame arrives.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Capability {
    pub version: u32,
    pub resume: String,
    pub generation: u64,
    pub expires_seconds: u64,
}

/// A bounded request with no replay queue or transport-specific interpretation.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op")]
pub(crate) enum Request {
    #[serde(rename = "sign_in")]
    SignIn,
    #[serde(rename = "sign_out")]
    SignOut,
    #[serde(rename = "pong")]
    Pong { resume: String },
}

/// Known events require their typed payload; optional future event kinds are ignored.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub(crate) enum Event {
    #[serde(rename = "welcome")]
    Welcome {
        #[serde(flatten)]
        capability: Capability,
    },
    #[serde(rename = "ping")]
    Ping {
        #[serde(flatten)]
        capability: Capability,
    },
    #[serde(rename = "account")]
    Account { account: Account },
    #[serde(rename = "connect")]
    Connect { connect: ConnectProgress },
    #[serde(rename = "error")]
    Error { error: RefusalCode },
    #[serde(other)]
    Unknown,
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

/// Omits unused numeric fields from a channel Hello.
fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests {
    use super::*;
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
