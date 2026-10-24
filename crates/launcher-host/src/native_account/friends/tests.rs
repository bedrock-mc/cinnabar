use super::*;
use serde_json::json;

/// Original service-shaped metadata, with no account credential or remote endpoint.
fn activity() -> Value {
    json!({"relatedInfo":{"closed":false},"customProperties":{
        "hostName":"Synthetic host","ownerId":"456","worldName":"Synthetic world",
        "MemberCount":2,"MaxMemberCount":8,"BroadcastSetting":2,"TransportLayer":2,
        "SupportedConnections":[null,{"ConnectionType":999},{"ConnectionType":7,
            "NetherNetId":u64::MAX,"PmsgId":"01234567-89ab-cdef-0123-456789abcdef"}]}})
}

/// Exercises followed-world selection, discriminator-dependent signaling and tolerant per-row failure.
#[test]
fn native_friend_worlds_project_only_listed_usable_activities_without_duplicate_cards() {
    let valid = activity();
    let mut values = vec![valid.clone(), valid.clone()];
    for (name, replacement) in [
        ("ownerId", json!("123")),
        ("hostName", json!("")),
        ("MemberCount", json!(0)),
        ("MemberCount", json!("2")),
        ("RealmId", json!(7)),
        (
            "ExperienceId",
            json!("01234567-89ab-cdef-0123-456789abcdef"),
        ),
        (
            "ExperienceWorldId",
            json!("01234567-89ab-cdef-0123-456789abcdef"),
        ),
        ("FriendId", json!("synthetic-experience")),
        ("BroadcastSetting", json!(1)),
        ("TransportLayer", json!(0)),
        ("SupportedConnections", json!([{"ConnectionType":4}])),
        (
            "SupportedConnections",
            json!([{"ConnectionType":7,"NetherNetId":0,
            "PmsgId":"01234567-89ab-cdef-0123-456789abcdef"}]),
        ),
        (
            "SupportedConnections",
            json!([{"ConnectionType":7,"NetherNetId":"01",
            "PmsgId":"01234567-89ab-cdef-0123-456789abcdef"}]),
        ),
        (
            "SupportedConnections",
            json!([{"ConnectionType":7,"NetherNetId":7}]),
        ),
        (
            "SupportedConnections",
            json!([{"ConnectionType":3,"NetherNetId":7}]),
        ),
    ] {
        let mut value = valid.clone();
        value["customProperties"][name] = replacement;
        values.push(value);
    }
    let mut closed = valid.clone();
    closed["relatedInfo"]["closed"] = json!(true);
    values.push(closed);
    for (name, closed) in [
        ("Missing closed flag", None),
        ("Null closed flag", Some(Value::Null)),
    ] {
        let mut value = valid.clone();
        value["customProperties"]["worldName"] = json!(name);
        let info = value["relatedInfo"].as_object_mut().unwrap();
        info.remove("closed");
        if let Some(closed) = closed {
            info.insert("closed".into(), closed);
        }
        values.push(value);
    }
    let mut missing_state = valid.clone();
    missing_state["relatedInfo"] = Value::Null;
    values.push(missing_state);

    // Case-insensitive title fields and wider visibility retain the existing catalog rules.
    let mut direct = valid.clone();
    let properties = direct["customProperties"].as_object_mut().unwrap();
    properties.remove("worldName");
    properties.insert("WORLDNAME".into(), json!("Direct synthetic world"));
    properties.insert("BroadcastSetting".into(), json!(4));
    properties.insert(
        "SupportedConnections".into(),
        json!([{"ConnectionType":3,"RakNetGUID":"18446744073709551615"}]),
    );
    values.push(direct);
    let mut opaque = valid.clone();
    opaque["customProperties"]["worldName"] = json!("Opaque synthetic world");
    opaque["customProperties"]["SupportedConnections"] = json!([{"ConnectionType":7,
        "NetherNetId":"01234567-89ab-cdef-0123-456789abcdef",
        "PmsgId":"01234567-89ab-cdef-0123-456789abcdef"}]);
    values.push(opaque);
    let cards = cards(
        values
            .into_iter()
            .map(|value| serde_json::from_value(value).unwrap())
            .collect(),
        "123",
    );
    assert_eq!(cards.len(), 5);
    assert_eq!(cards[0].gamertag, "Synthetic host");
    assert_eq!(cards[0].world_name, "Synthetic world");
    assert_eq!(cards[0].xuid, "456");
    assert_eq!(cards[0].members, "2/8 players");
    assert_eq!(cards[0].max_members, 8);
    assert_eq!(cards[1].world_name, "Missing closed flag");
    assert_eq!(cards[2].world_name, "Null closed flag");
    assert_eq!(cards[3].world_name, "Direct synthetic world");
    assert_eq!(cards[4].world_name, "Opaque synthetic world");
}
