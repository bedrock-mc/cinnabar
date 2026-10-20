use super::{ClientDataRefusal, RawClientData};
use crate::{
    ClientRandomId, LoginSettings, NativeDeviceProfile, PROTOCOL_VERSION, login_client_data,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr};

#[derive(Deserialize)]
struct Inputs {
    bracketed_port: String,
    decoded_geometry: String,
}

/// Numeric address framing is rejected before the explicit named-service resolution exception.
#[test]
fn metadata_limits_reject_brackets_in_port_before_service_lookup() {
    let inputs = inputs();
    let profile = NativeDeviceProfile::fresh();
    let mut claims = baseline();
    assert!(
        profile
            .validate_relay(raw(&claims), PROTOCOL_VERSION)
            .is_ok()
    );
    claims["ServerAddress"] = json!(inputs.bracketed_port);
    let result = profile.validate_relay(raw(&claims), PROTOCOL_VERSION);
    assert!(
        matches!(result, Err(ClientDataRefusal::Generic)),
        "SplitHostPort must reject a bracket in the port before service lookup: {result:?}",
    );
}

/// The real metadata owner admits a small valid object beyond recursive Value's default depth.
#[test]
fn metadata_limits_accept_valid_decoded_skin_json_above_value_depth() {
    let inputs = inputs();
    let profile = NativeDeviceProfile::fresh();
    let mut claims = baseline();
    assert!(
        profile
            .validate_relay(raw(&claims), PROTOCOL_VERSION)
            .is_ok()
    );
    claims["SkinGeometryData"] = json!(STANDARD.encode(inputs.decoded_geometry));
    let result = profile.validate_relay(raw(&claims), PROTOCOL_VERSION);
    assert!(
        result.is_ok(),
        "valid decoded skin object must not inherit recursive Value's lower depth limit: {result:?}",
    );
}

/// These two bounded synthetic inputs are shared with the actual pinned Go consumer.
fn inputs() -> Inputs {
    serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/native-client-data-limits.json"
    ))
    .unwrap()
}

/// Uses the actual claim builder and a numeric loopback label without authorizing a socket.
fn baseline() -> Value {
    let mut claims = login_client_data(
        "Fixture",
        ClientRandomId::new(1).unwrap(),
        None,
        &LoginSettings::default(),
    );
    claims["ServerAddress"] = json!(
        SocketAddr::from((Ipv4Addr::LOCALHOST, bridge::contract::DEFAULT_BEDROCK_PORT,))
            .to_string()
    );
    claims
}

/// Exercises the actual raw type gate before the public consuming metadata transition.
fn raw(claims: &Value) -> RawClientData {
    RawClientData::from_raw(&serde_json::value::to_raw_value(claims).unwrap()).unwrap()
}
