use bridge::contract;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

/// Decodes and re-encodes a wire value through the public Rust type.
fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) {
    let decoded: T = serde_json::from_value(value.clone()).expect("decode contract value");
    assert_eq!(serde_json::to_value(decoded).unwrap(), *value);
}

#[test]
fn wire_enums_accept_every_manifest_spelling() {
    let manifest: Value = serde_json::from_str(include_str!("../data/contract.json")).unwrap();
    for entry in manifest["enums"].as_array().unwrap() {
        for variant in entry["variants"].as_array().unwrap() {
            let value = if entry.get("content").is_some() {
                json!({"kind": variant["value"], "value": "target"})
            } else {
                variant["value"].clone()
            };
            match entry["rust_name"].as_str().unwrap() {
                "GameMode" => round_trip::<bridge::GameMode>(&value),
                "Backend" => round_trip::<bridge::Backend>(&value),
                "WorldState" => round_trip::<bridge::WorldState>(&value),
                "Transport" => round_trip::<bridge::Transport>(&value),
                "Lifecycle" => round_trip::<bridge::Lifecycle>(&value),
                "PackOffer" => round_trip::<bridge::PackOffer>(&value),
                "PackAcquisition" => round_trip::<bridge::PackAcquisition>(&value),
                "PackDownstreamOutcome" => round_trip::<bridge::PackDownstreamOutcome>(&value),
                "PackApplication" => round_trip::<bridge::PackApplication>(&value),
                "RefusalCode" => round_trip::<contract::RefusalCode>(&value),
                "ConnectTarget" => round_trip::<bridge::ConnectTarget>(&value),
                "AuthState" => round_trip::<bridge::AuthState>(&value),
                "ConnectStage" => round_trip::<bridge::ConnectStage>(&value),
                name => panic!("add wire coverage for the new contract enum {name}"),
            }
        }
    }
}

#[test]
fn join_stage_extensions_keep_the_connecting_fallback() {
    assert_eq!(
        serde_json::from_value::<bridge::ConnectStage>(json!("future_stage")).unwrap(),
        bridge::ConnectStage::Connecting,
    );
    assert!(serde_json::from_value::<bridge::AuthState>(json!("future_state")).is_err());
}

#[test]
fn generated_version_and_port_follow_their_sources() {
    let target: Value = serde_json::from_str(include_str!("../../../assets/bedrock-target.json"))
        .expect("read pinned Bedrock target");
    assert_eq!(
        contract::GAME_VERSION,
        target["game_version"].as_str().unwrap()
    );
    assert_eq!(
        contract::DEFAULT_BEDROCK_PORT_TEXT,
        contract::DEFAULT_BEDROCK_PORT.to_string()
    );
}

#[test]
fn endpoints_use_the_shared_platform_names() {
    let directory = std::path::Path::new("bridge-contract-test");
    let (control, session) = if cfg!(windows) {
        (
            contract::CONTROL_WINDOWS_ENDPOINT_NAME,
            contract::SESSION_WINDOWS_ENDPOINT_NAME,
        )
    } else {
        (
            contract::CONTROL_UNIX_ENDPOINT_NAME,
            contract::SESSION_UNIX_ENDPOINT_NAME,
        )
    };
    assert_eq!(
        bridge::control_endpoint_path(directory),
        directory.join(control)
    );
    assert_eq!(
        bridge::session_endpoint_path(directory),
        directory.join(session)
    );
}
