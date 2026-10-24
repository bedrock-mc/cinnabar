//! The consumed v1 server fields; extensions and unknown message kinds remain harmless.

use super::policy::Advertised;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Inbound {
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) nonce: String,
    pub(super) code: String,
    pub(super) xuid: String,
    pub(super) uuid: String,
    pub(super) id: Option<u32>,
    pub(super) cinnabar: Vec<String>,
    pub(super) cinnabar_uuids: Vec<String>,
    pub(super) channel: String,
    pub(super) version: String,
    pub(super) build: u64,
    pub(super) published_at: String,
    pub(super) loadout_revision: u64,
    #[serde(deserialize_with = "deserialize_owned_cosmetics")]
    pub(super) owned_cosmetics: Option<Vec<String>>,
    pub(super) emote_id: Option<String>,
    pub(super) elapsed_ms: u32,
    pub(super) valid_for_ms: u64,
    pub(super) revision: u64,
    pub(super) slots: BTreeMap<String, Option<String>>,
    #[serde(flatten)]
    pub(super) parameters: Advertised,
}

impl Inbound {
    /// Ignores non-JSON keepalives and malformed/unused service values without retaining raw bodies.
    pub(super) fn parse(text: &str) -> Option<Self> {
        if !text.trim_start().starts_with('{') {
            return None;
        }
        serde_json::from_str(text).ok()
    }
}

/// Preserves null string elements as invalid IDs so the whole ownership snapshot is withdrawn.
fn deserialize_owned_cosmetics<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Vec<Option<String>>>::deserialize(deserializer)
        .map(|ids| ids.map(|ids| ids.into_iter().map(|id| id.unwrap_or_default()).collect()))
}
