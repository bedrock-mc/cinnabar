use crate::login::client_data::{NativeClaims, NativeDeviceProfile};
use bedrock_net::login::client_data::RawClientData;
use serde_json::{json, value::RawValue};

/// The actual native decoder retains opaque numeric/string lexemes and a distinct explicit zero.
#[test]
fn native_connect_preserves_raw_values_and_full_wire_random_ids() {
    for random in [0, i64::MIN, i64::MAX] {
        let json = format!(
            r#"{{"protocol":{},"client_data":{{"Unknown":1e99999,"ClientRandomId":{random},"Nested":{{ "x":1.2300e+04 }},"GameVersion":"{}","ThirdPartyName":"Fixture"}}}}"#,
            crate::PROTOCOL_VERSION,
            crate::GAME_VERSION
        );
        let frame = [vec![bridge::contract::KIND_CONNECT], json.into_bytes()].concat();
        let request = crate::session_wire::decode_native_connect(&frame).unwrap();
        assert_eq!(request.client_data.random_id().unwrap(), Some(random));
        assert_eq!(
            request.client_data.game_version().unwrap().as_deref(),
            Some(crate::GAME_VERSION)
        );
        let output = serde_json::to_string(&request).unwrap();
        assert!(output.contains(r#""Unknown":1e99999"#));
        assert!(output.contains(r#""Nested":{ "x":1.2300e+04 }"#));
        // The existing default Value decoder keeps its original numeric-domain contract.
        assert!(crate::session_wire::decode_connect(&frame).is_err());
    }
    let missing = RawClientData::from_value(&json!({})).unwrap();
    assert_eq!(missing.random_id().unwrap(), None);
    for invalid in ["null", "[]", "7", "\"opaque\""] {
        let raw = RawValue::from_string(invalid.into()).unwrap();
        assert!(RawClientData::from_raw(&raw).is_err());
    }
}

/// Canonical device/identity/transport overlays remove aliases without decoding unrelated values.
#[test]
fn raw_owned_overlays_replace_all_aliases_and_preserve_opaque_claims() {
    let raw = RawValue::from_string(r#"{"Unknown":1e99999,"ClientRandomId":0,"ThirdPartyName":"old","thirdpartyname":"stale","ſelfSignedId":"stale","selfsignedid":"also stale","ſerverAddreſſ":"stale","SERVERADDRESS":"stale","nonce":"stale","NONCE":"stale","PlatformType":0}"#.into()).unwrap();
    let mut data = RawClientData::from_raw(&raw).unwrap();
    let profile = NativeDeviceProfile::fresh();
    profile.apply_relay_device(&mut data).unwrap();
    NativeDeviceProfile::apply_relay_identity(&mut data, "Offline", profile.relay_identity(0))
        .unwrap();
    let mut claims = NativeClaims::Relay(data);
    claims.set_display_name("Verified").unwrap();
    claims
        .set_transport("selected.example:19132", "original-nonce")
        .unwrap();
    let output = claims.into_raw().unwrap();
    let data = RawClientData::from_raw(&output).unwrap();
    assert_eq!(data.display_name().unwrap().as_deref(), Some("Verified"));
    assert_eq!(data.random_id().unwrap(), Some(0));
    assert_eq!(
        data.read::<String>("ServerAddress").unwrap().as_deref(),
        Some("selected.example:19132")
    );
    assert_eq!(
        data.read::<String>("Nonce").unwrap().as_deref(),
        Some("original-nonce")
    );
    assert!(uuid::Uuid::parse_str(&data.read::<String>("SelfSignedId").unwrap().unwrap()).is_ok());
    for canonical in ["ThirdPartyName", "SelfSignedId", "ServerAddress", "Nonce"] {
        let names = data
            .entries()
            .filter(|(name, _)| *name == canonical)
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        assert_eq!(names, [canonical]);
    }
    assert_eq!(
        data.entries()
            .find(|(name, _)| *name == "Unknown")
            .unwrap()
            .1
            .get(),
        "1e99999"
    );
    assert!(!output.get().contains("stale"));
    assert!(!format!("{data:?}").contains("original-nonce"));
}

/// Desktop construction and the default Connect DTO retain the original map bytes mechanically.
#[test]
fn canonical_desktop_signing_and_default_connect_bytes_are_unchanged() {
    let random = crate::ClientRandomId::new(-7).unwrap();
    let mut original =
        crate::login_client_data("Original", random, None, &crate::LoginSettings::default());
    NativeDeviceProfile::fresh()
        .apply(&mut original, random, "Account")
        .unwrap();
    let raw = serde_json::value::to_raw_value(&original).unwrap();
    let mut claims = NativeClaims::canonical(&raw).unwrap();
    claims.set_display_name("Verified").unwrap();
    claims
        .set_transport("selected.example:19132", "original-nonce")
        .unwrap();
    original["ThirdPartyName"] = json!("Verified");
    original["ServerAddress"] = json!("selected.example:19132");
    original["Nonce"] = json!("original-nonce");
    assert_eq!(
        claims.into_raw().unwrap().get().as_bytes(),
        serde_json::to_vec(&original).unwrap()
    );
    let request = crate::session_wire::ConnectRequest {
        join_profile_id: None,
        protocol: crate::PROTOCOL_VERSION,
        target: None,
        client_cache: false,
        client_data: original,
    };
    let encoded = crate::session_wire::encode_connect(&request).unwrap();
    let decoded = crate::session_wire::decode_connect(&encoded).unwrap();
    assert_eq!(
        crate::session_wire::encode_connect(&decoded).unwrap(),
        encoded
    );
}
