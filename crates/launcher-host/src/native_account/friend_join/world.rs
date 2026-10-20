//! Interpretation of title metadata from a complete joined directory snapshot.

use super::super::world_metadata::{
    JSON_RPC_SIGNALING, NETHERNET_TRANSPORT, WEBSOCKET_SIGNALING, field,
};
use super::{FriendAdmission, Scope, invalid};
use bedrock_net::messaging::Recipient;
use bedrock_services::xbox::mpsd;
use serde_json::{Map, Value};
use std::io;
use uuid::Uuid;

/// Extracts admission only from current local/host membership and this account's host-issued nonce.
pub(super) fn admission(
    description: &mpsd::Description,
    scope: &Scope,
    host: &str,
) -> io::Result<Option<FriendAdmission>> {
    let mut local = false;
    let mut owner = false;
    for member in description.members.values().flatten() {
        if member.xuid() == Some(scope.xuid.as_str()) {
            local = true;
        }
        if member.xuid() == Some(host) {
            owner = true;
        }
    }
    if !local || !owner {
        return Err(invalid("friend-world membership is unavailable"));
    }
    let properties = description
        .properties
        .as_ref()
        .ok_or_else(|| invalid("friend-world properties are missing"))?;
    if properties
        .system
        .as_ref()
        .and_then(|system| system.get("closed"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err(invalid("friend world closed admission"));
    }
    let custom = properties
        .custom
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("friend-world metadata is missing"))?;
    if text(custom, "ownerId") != Some(host)
        || integer(custom, "TransportLayer") != Some(NETHERNET_TRANSPORT)
        || integer(custom, "protocol") != Some(i64::from(protocol::PROTOCOL_VERSION))
    {
        return Err(invalid(
            "friend-world identity, transport or version changed",
        ));
    }
    let connections = field(custom, "SupportedConnections")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("friend world has no connections"))?;
    let recipient = connections
        .iter()
        .filter_map(Value::as_object)
        .find_map(|connection| {
            if integer(connection, "ConnectionType") == Some(WEBSOCKET_SIGNALING) {
                return text(connection, "RakNetGUID")
                    .filter(|peer| usable_network_id(peer))
                    .map(|peer| Recipient::Legacy(peer.to_owned()));
            }
            if integer(connection, "ConnectionType") != Some(JSON_RPC_SIGNALING) {
                return None;
            }
            let network = field(connection, "NetherNetId")?;
            let usable = match network {
                Value::String(value) => usable_network_id(value),
                Value::Number(value) => value.as_u64().is_some_and(|value| value != 0),
                _ => false,
            };
            usable
                .then(|| {
                    text(connection, "PmsgId")
                        .and_then(|value| Uuid::parse_str(value).ok())
                        .filter(|value| !value.is_nil())
                        .map(Recipient::Messaging)
                })
                .flatten()
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "friend world has no supported signaling connection",
            )
        })?;
    let Some(nonces) = field(custom, "nonces") else {
        return Ok(None);
    };
    let nonces = nonces
        .as_object()
        .ok_or_else(|| invalid("invalid friend-world nonce map"))?;
    let Some(nonce) = nonces.get(&scope.xuid) else {
        return Ok(None);
    };
    let nonce = nonce
        .as_str()
        .filter(|nonce| {
            !nonce.is_empty()
                && nonce.len() <= protocol::MAX_NATIVE_JOIN_NONCE_BYTES
                && !nonce.chars().any(char::is_control)
        })
        .ok_or_else(|| invalid("invalid friend-world admission nonce"))?;
    Ok(Some(FriendAdmission {
        scope: scope.clone(),
        recipient,
        nonce: zeroize::Zeroizing::new(nonce.to_owned()),
    }))
}

/// Accepts only nonzero canonical decimal network IDs or nonnil UUID identifiers.
fn usable_network_id(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|id| id != 0 && id.to_string() == value)
        || Uuid::parse_str(value).is_ok_and(|id| !id.is_nil())
}

/// Borrows a supplied title string without coercing other JSON types.
pub(super) fn text<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    field(object, name)?.as_str()
}

/// Reads a supplied signed integer without accepting fractions or numeric strings.
fn integer(object: &Map<String, Value>, name: &str) -> Option<i64> {
    field(object, name)?.as_i64()
}
