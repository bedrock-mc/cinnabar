//! Shared Minecraft session-directory namespace and wire discriminators.

use serde_json::{Map, Value};

pub(super) const WORLD_TEMPLATE: &str = "MinecraftLobby";
pub(super) const NETHERNET_TRANSPORT: i64 = 2;
pub(super) const WEBSOCKET_SIGNALING: i64 = 3;
pub(super) const JSON_RPC_SIGNALING: i64 = 7;
pub(super) const FRIENDS_OF_FRIENDS_BROADCAST: i64 = 3;
pub(super) const FRIENDS_JOINABILITY: &str = "joinable_by_friends";

/// Go's title metadata decoding compares JSON field names without ASCII case sensitivity.
pub(super) fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).or_else(|| {
        object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}

/// Validates a canonical, nonzero Xbox identity without retaining malformed service text.
pub(super) fn xuid(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|id| id != 0 && id.to_string() == value)
}
