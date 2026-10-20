//! Shared Go edits through the actual gateway's raw preparation and refusal boundary.

use super::*;
use futures::SinkExt;
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{collections::BTreeMap, net::SocketAddr};
use tokio::sync::mpsc;

const OPAQUE_NAME: &str = "FutureClientData";
const OPAQUE_VALUE: &str = r#"{"integer":9007199254740993,"overflow":1e1000,"text":"\u0061"}"#;
const BASELINE_NAME: &str = "Fixture";

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Accepted,
    OutdatedClient,
    OutdatedServer,
    InvalidSkin,
    InvalidName,
    Generic,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    edits: BTreeMap<String, Box<RawValue>>,
    protocol_delta: i32,
    outcome: Outcome,
}

struct Observation {
    prepared: Option<RawClientData>,
    outbound: Vec<Bytes>,
    result: io::Result<()>,
    cleanup: io::Result<()>,
    socket_open: bool,
}

/// Actual preparation/refusal must match the frozen Go corpus before any transport is authorized.
#[tokio::test]
async fn native_gateway_semantic_refusals_match_the_go_admission_corpus() {
    tokio::time::timeout(test_time::DEFAULT_TIMEOUT, async {
        let cases: Vec<Case> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/protocol/tests/fixtures/native-client-data-validation.json"
        )))
        .unwrap();
        let mut observations = Vec::with_capacity(cases.len());
        for case in cases {
            let observation = observe(&case).await;
            observations.push((case, observation));
        }
        let mut mismatches = Vec::new();
        // Every case's actual stream, writer, lease and registry owners have retired.
        for (case, observation) in observations {
            let actual = outcome(&observation);
            if actual != case.outcome {
                mismatches.push(format!(
                    "{}: actual {actual:?}, want {:?}",
                    case.name, case.outcome
                ));
            }
            assert!(
                observation.result.is_err(),
                "absent target must not connect"
            );
            assert!(observation.cleanup.is_ok(), "idempotent cleanup failed");
            assert!(
                !observation.socket_open,
                "metadata allocated an upstream socket"
            );
            if let Some(prepared) = observation.prepared {
                assert_eq!(prepared.display_name().unwrap(), requested_name(&case));
                let fields: BTreeMap<String, Box<RawValue>> =
                    serde_json::from_str(&serde_json::to_string(&prepared).unwrap()).unwrap();
                assert_eq!(fields[OPAQUE_NAME].get(), OPAQUE_VALUE);
                assert_eq!(read::<u32>(&fields, "DeviceOS"), NATIVE_ACCOUNT_DEVICE_OS);
                assert_eq!(
                    read::<String>(&fields, "DeviceModel"),
                    NATIVE_ACCOUNT_DEFAULT_MODEL
                );
                let profile = serde_json::json!({
                    "os": read::<u32>(&fields, "DeviceOS"),
                    "model": read::<String>(&fields, "DeviceModel"),
                    "id": read::<String>(&fields, "DeviceId"),
                });
                assert!(
                    protocol::NativeDeviceProfile::parse(&serde_json::to_vec(&profile).unwrap())
                        .is_ok(),
                    "actual prepared device profile is invalid"
                );
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    })
    .await
    .expect("actual gateway semantic fixture stalled");
}

/// Runs the real lease and framed stream to preparation or refusal, then retires every owner.
async fn observe(case: &Case) -> Observation {
    let registry = Registry::new();
    let attachment = registry
        .open_control(Ipv4Addr::LOCALHOST.into(), "", Instant::now())
        .unwrap();
    let lease = attachment
        .session
        .begin_play(attachment.generation, 1)
        .await
        .unwrap();
    let mut playing = Playing {
        socket: None,
        lease,
    };
    let (stream, browser) = tokio::io::duplex(RELAY_CONTROL_BYTES);
    let (read, write) = tokio::io::split(stream);
    let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(
        read,
        write,
        bridge::MAX_FRAME_LEN,
        RELAY_QUEUED_FRAME_BYTES,
    );
    let (prepared, mut observed) = mpsc::channel(1);
    let deadline = Instant::now()
        .checked_add(protocol::NATIVE_JOIN_TIMEOUT)
        .unwrap();
    let serving = PREPARED_CLAIMS.scope(prepared, playing.serve(reader, frames, deadline));
    let browser = async {
        let mut browser = Framed::new(browser, BridgeCodec::with_max(bridge::MAX_FRAME_LEN));
        browser.send(connect_frame(case)).await.unwrap();
        let mut outbound = Vec::new();
        while let Some(frame) = browser.next().await {
            outbound.push(frame.unwrap());
        }
        outbound
    };
    let (result, (), outbound) = tokio::join!(serving, writer, browser);
    let prepared = observed.try_recv().ok();
    let socket_open = playing.socket.is_some();
    let cleanup = playing.close().await.and(playing.close().await);
    drop(playing);
    drop(attachment);
    registry.close();
    Observation {
        prepared,
        outbound,
        result,
        cleanup,
        socket_open,
    }
}

/// Uses the ordinary Connect schema while retaining every corpus and opaque value lexeme.
fn connect_frame(case: &Case) -> Bytes {
    let random = protocol::ClientRandomId::new(1).unwrap();
    let baseline = protocol::login_client_data(
        BASELINE_NAME,
        random,
        None,
        &protocol::LoginSettings::default(),
    );
    let mut fields: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(&serde_json::to_string(&baseline).unwrap()).unwrap();
    let metadata_address =
        SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_BEDROCK_PORT)).to_string();
    fields.insert(
        "ServerAddress".into(),
        serde_json::value::to_raw_value(&metadata_address).unwrap(),
    );
    for (name, value) in &case.edits {
        fields.insert(name.clone(), value.clone());
    }
    fields.insert(
        OPAQUE_NAME.into(),
        RawValue::from_string(OPAQUE_VALUE.into()).unwrap(),
    );
    let request = session_wire::ConnectRequest {
        join_profile_id: None,
        protocol: protocol::PROTOCOL_VERSION
            .checked_add(case.protocol_delta)
            .unwrap(),
        target: None,
        client_cache: true,
        client_data: serde_json::value::to_raw_value(&fields).unwrap(),
    };
    let mut frame = vec![KIND_CONNECT];
    serde_json::to_writer(&mut frame, &request).unwrap();
    Bytes::from(frame)
}

/// Projects only real terminal messages or actual preparation, never diagnostic error strings.
fn outcome(observation: &Observation) -> Outcome {
    if observation.prepared.is_some() {
        assert!(
            observation.outbound.is_empty(),
            "prepared metadata also emitted a terminal"
        );
        return Outcome::Accepted;
    }
    let Some(frame) = observation.outbound.first() else {
        return Outcome::Generic;
    };
    assert_eq!(
        observation.outbound.len(),
        1,
        "refusal emitted a nonterminal prefix"
    );
    let CoreMessage::Disconnect(disconnect) =
        session_wire::decode_core_message(frame.clone()).unwrap()
    else {
        panic!("metadata refusal emitted a non-disconnect frame");
    };
    match disconnect.message.as_str() {
        DISCONNECT_KEY_OUTDATED_CLIENT => Outcome::OutdatedClient,
        DISCONNECT_KEY_OUTDATED_SERVER => Outcome::OutdatedServer,
        DISCONNECT_KEY_INVALID_SKIN => Outcome::InvalidSkin,
        DISCONNECT_KEY_INVALID_NAME => Outcome::InvalidName,
        DISCONNECT_KEY_CANT_CONNECT => Outcome::Generic,
        _ => panic!("actual admission returned an unclassified refusal key"),
    }
}

/// Reads only the test's specifically asserted field without parsing unrelated raw values.
fn read<T: serde::de::DeserializeOwned>(fields: &BTreeMap<String, Box<RawValue>>, name: &str) -> T {
    serde_json::from_str(fields[name].get()).unwrap()
}

/// Keeps expected presentation tied to the corpus edit or the actual baseline builder input.
fn requested_name(case: &Case) -> Option<String> {
    Some(case.edits.get("ThirdPartyName").map_or_else(
        || BASELINE_NAME.to_owned(),
        |value| serde_json::from_str(value.get()).unwrap(),
    ))
}
