// Generated from crates/server-experience/schema/extension.json; do not edit.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    rename_all = "snake_case",
    tag = "kind",
    deny_unknown_fields,
    content = "body"
)]
pub enum ExperienceControl {
    Epoch {
        session: String,
        world_epoch: u64,
    },
    WorldAck {
        session: String,
        world_epoch: u64,
        requested_epoch: u64,
    },
}
