use super::*;
use bedrock_services::xbox::social;
use serde_json::{Value, json};

/// Supplies original synthetic title metadata and real member identities, without service traffic.
fn snapshot() -> Value {
    json!({
        "members":{
            "me":{"constants":{"system":{"xuid":"123"}}},
            "0":{"constants":{"system":{"xuid":"456"}}}
        },
        "properties":{"system":{"closed":false}, "custom":{
            "ownerId":"456", "protocol":protocol::PROTOCOL_VERSION,
            "TransportLayer":super::super::world_metadata::NETHERNET_TRANSPORT,
            "SupportedConnections":[
                {"ConnectionType":999},
                {"ConnectionType":super::super::world_metadata::JSON_RPC_SIGNALING,
                    "NetherNetId":42, "PmsgId":Uuid::from_u128(7)}
            ],
            "nonces":{"123":"synthetic-player-admission", "456":"another-player"}
        }}
    })
}

/// Builds the captured application identity, with no OAuth or proof material.
fn scope() -> Scope {
    Scope {
        generation: "synthetic-generation".into(),
        xuid: "123".into(),
    }
}

#[test]
fn joined_snapshot_requires_the_selected_host_and_actual_local_membership() {
    let description = serde_json::from_value(snapshot()).unwrap();
    let admission = world::admission(&description, &scope(), "456")
        .unwrap()
        .unwrap();
    let (recipient, nonce) = admission.into_parts();
    assert!(matches!(recipient, Recipient::Messaging(peer) if peer == Uuid::from_u128(7)));
    assert_eq!(nonce, "synthetic-player-admission");

    let mut value = snapshot();
    value["members"]["me"] = Value::Null;
    assert!(world::admission(&serde_json::from_value(value).unwrap(), &scope(), "456").is_err());
    let mut value = snapshot();
    value["properties"]["custom"]["ownerId"] = json!("789");
    assert!(world::admission(&serde_json::from_value(value).unwrap(), &scope(), "456").is_err());
    let mut value = snapshot();
    value["properties"]["custom"]["protocol"] = json!(protocol::PROTOCOL_VERSION + 1);
    assert!(world::admission(&serde_json::from_value(value).unwrap(), &scope(), "456").is_err());
}

#[test]
fn nonce_readiness_never_borrows_another_players_admission_or_accepts_an_empty_nonce() {
    let mut value = snapshot();
    value["properties"]["custom"]["nonces"]
        .as_object_mut()
        .unwrap()
        .remove("123");
    assert!(
        world::admission(&serde_json::from_value(value).unwrap(), &scope(), "456")
            .unwrap()
            .is_none()
    );
    let mut value = snapshot();
    value["properties"]["custom"]["nonces"]["123"] = json!("");
    assert!(world::admission(&serde_json::from_value(value).unwrap(), &scope(), "456").is_err());
    let mut value = snapshot();
    value["properties"]["custom"]["SupportedConnections"][1]["PmsgId"] = json!(Uuid::nil());
    assert!(world::admission(&serde_json::from_value(value).unwrap(), &scope(), "456").is_err());
}

#[test]
fn selection_keeps_the_actual_service_handle_and_reference_without_admitting_advertised_credentials()
 {
    let activity: social::Activity = serde_json::from_value(json!({
        "id":Uuid::from_u128(10), "type":"activity", "ownerXuid":"456",
        "sessionRef":{"scid":bedrock_services::minecraft::SERVICE_CONFIG_ID,
            "templateName":"MinecraftLobby", "name":"synthetic-world"},
        "customProperties":{"ownerId":"456", "nonces":{"123":"unjoined-nonce"}}
    }))
    .unwrap();
    let selected = FriendSelection::activity(&activity).unwrap();
    assert_eq!(selected.handle, Uuid::from_u128(10));
    assert_eq!(selected.host, "456");
    assert_eq!(selected.reference.name(), "synthetic-world");

    let invitation: mpsd::Invitation = serde_json::from_value(json!({
        "id":Uuid::from_u128(11), "senderXuid":"789", "invitedXuid":"123",
        "expiration":"2099-01-01T00:00:00Z",
        "sessionRef":{"scid":bedrock_services::minecraft::SERVICE_CONFIG_ID,
            "templateName":"MinecraftLobby", "name":"invited-world"}
    }))
    .unwrap();
    let selected = FriendSelection::invitation(&invitation, "456").unwrap();
    assert_eq!(selected.host, "456");
    assert_eq!(selected.recipient.as_deref(), Some("123"));
    assert_eq!(selected.reference.name(), "invited-world");
}

/// A joined legacy-only world supplies the same original player's nonce without a JSON-RPC fallback.
#[test]
fn joined_legacy_snapshot_admits_its_own_member_nonce() {
    let mut value = snapshot();
    value["properties"]["custom"]["SupportedConnections"] = json!([
        {"ConnectionType":super::super::world_metadata::WEBSOCKET_SIGNALING,
            "RakNetGUID":"42"}
    ]);
    let description = serde_json::from_value(value).unwrap();
    let result = world::admission(&description, &scope(), "456");
    assert!(
        result.is_ok(),
        "a verified joined legacy-only world was refused before preparing its original service route"
    );
    let admission = result.unwrap().expect("joined player's original nonce");
    assert_eq!(admission.into_parts().1, "synthetic-player-admission");
}

/// Dialect and route come from the first usable joined connection, not an unrelated advertised ID.
#[test]
fn joined_connection_order_and_exact_legacy_route_remain_part_of_admission() {
    let mut value = snapshot();
    let connections = value["properties"]["custom"]["SupportedConnections"]
        .as_array_mut()
        .unwrap();
    connections.insert(
        1,
        json!({
            "ConnectionType":super::super::world_metadata::WEBSOCKET_SIGNALING,
            "RakNetGUID":"42", "NetherNetId":"ignored", "PmsgId":Uuid::from_u128(8)
        }),
    );
    let admission = world::admission(
        &serde_json::from_value(value.clone()).unwrap(),
        &scope(),
        "456",
    )
    .unwrap()
    .unwrap();
    assert!(matches!(admission.recipient(), Recipient::Legacy(peer) if peer == "42"));
    let original = (
        admission.recipient().clone(),
        Sha256::digest(admission.nonce.as_bytes()).into(),
    );
    assert!(admission.matches_identity(&original));
    for change in ["route", "dialect", "nonce"] {
        let mut changed = value.clone();
        match change {
            "route" => {
                changed["properties"]["custom"]["SupportedConnections"][1]["RakNetGUID"] =
                    json!("43")
            }
            "dialect" => {
                changed["properties"]["custom"]["SupportedConnections"]
                    .as_array_mut()
                    .unwrap()
                    .remove(1);
            }
            "nonce" => {
                changed["properties"]["custom"]["nonces"]["123"] = json!("replacement-nonce")
            }
            _ => unreachable!(),
        }
        let changed = world::admission(&serde_json::from_value(changed).unwrap(), &scope(), "456")
            .unwrap()
            .unwrap();
        assert!(
            !changed.matches_identity(&original),
            "{change} retained old admission"
        );
    }
}

/// A type3 connection must use its own valid string RakNetGUID; malformed neighbors cannot redirect it.
#[test]
fn unusable_legacy_fields_do_not_replace_the_joined_jsonrpc_route() {
    for legacy in [
        json!(null),
        json!(0),
        json!("0"),
        json!("0007"),
        json!("host:19132"),
    ] {
        let mut value = snapshot();
        value["properties"]["custom"]["SupportedConnections"]
            .as_array_mut()
            .unwrap()
            .insert(
                1,
                json!({
                    "ConnectionType":super::super::world_metadata::WEBSOCKET_SIGNALING,
                    "RakNetGUID":legacy, "NetherNetId":42, "PmsgId":Uuid::from_u128(8)
                }),
            );
        let admission = world::admission(&serde_json::from_value(value).unwrap(), &scope(), "456")
            .unwrap()
            .unwrap();
        assert!(
            matches!(admission.recipient(), Recipient::Messaging(peer) if *peer == Uuid::from_u128(7))
        );
    }
}

/// Activity handles and reference spelling cannot split the selected world's presentation identity.
#[test]
fn friend_world_identity_tracks_the_validated_reference_without_exposing_its_locator() {
    let (_directory, account) = super::super::profile::tests::account();
    let selected = |name: &str, template: &str, handle| {
        let activity = serde_json::from_value(json!({
            "id": Uuid::from_u128(handle), "type": "activity", "ownerXuid": "456",
            "sessionRef": {
                "scid": bedrock_services::minecraft::SERVICE_CONFIG_ID,
                "templateName": template, "name": name
            },
            "customProperties": {"ownerId": "456"}
        }))
        .unwrap();
        NativeFriendSelection::capture(
            Arc::clone(&account),
            "original".into(),
            "123".into(),
            FriendSelection::activity(&activity).unwrap(),
        )
    };
    let original = selected("private-fixture-world", "MinecraftLobby", 1);
    let refreshed = selected("PRIVATE-FIXTURE-WORLD", "minecraftlobby", 2);
    assert!(
        original
            .selection
            .reference
            .same(&refreshed.selection.reference)
    );
    let identity = original.world_identity();
    assert_eq!(identity, refreshed.world_identity());
    assert_ne!(
        identity,
        selected("another-private-fixture-world", "MinecraftLobby", 3).world_identity()
    );
    assert!(identity.starts_with("friend:"));
    assert!(!identity.contains(original.selection.reference.name()));
    assert!(!identity.chars().any(char::is_control));
}
