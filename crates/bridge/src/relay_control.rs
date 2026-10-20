//! Canonical relay wire models shared by the browser and actual native gateway consumer.
use crate::{Account, ConnectProgress, contract::*};
use serde::{Deserialize, Serialize};

/// One channel prefix, with credentials carried only inside its bounded frame.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Hello {
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

/// A rotated capability and its control generation, observed after a bounded frame arrives.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    pub version: u32,
    pub resume: String,
    pub generation: u64,
    pub expires_seconds: u64,
}

/// A bounded request with no replay queue or transport-specific interpretation.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op")]
pub enum Request {
    #[serde(rename = "sign_in")]
    SignIn,
    #[serde(rename = "sign_out")]
    SignOut,
    #[serde(rename = "pong")]
    Pong { resume: String },
    /// An unsupported well-formed operation receives the existing bounded refusal.
    #[serde(other)]
    Unknown,
}

/// Known events require their typed payload; optional future event kinds are ignored.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Event {
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

/// Omits unused numeric fields from a channel Hello.
fn is_zero(value: &u64) -> bool {
    *value == 0
}
