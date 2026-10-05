use super::*;
use bytes::BytesMut;
use std::time::{Duration, Instant};
use valentine::bedrock::codec::BedrockCodec;

const TERRAIN_FRAME_BYTES: usize = 512 * 1024;
const PROFILE_SAMPLES: usize = 129;
const PROFILE_ENV: &str = "CINNABAR_LOGIN_CPU_PROFILE";

fn terrain_frame(payload_bytes: usize) -> RawPacket {
    let mut payload = BytesMut::new();
    McpePacketName::LevelChunkPacket
        .encode(&mut payload)
        .unwrap();
    payload.resize(payload.len() + payload_bytes, 0);
    let mut frame = BytesMut::new();
    valentine::protocol::wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    let mut cursor = frame.freeze();
    crate::raw::decode_packet_raw(&mut cursor).expect("framed packet")
}

#[test]
fn deferred_whole_frame_keeps_its_existing_allocation() {
    let raw = terrain_frame(TERRAIN_FRAME_BYTES);
    let original = raw.inner_frame().clone();
    let mut deferred = DeferredPackets::default();
    deferred.push(raw).unwrap();
    let retained = deferred.into_packets().pop().unwrap();
    assert_eq!(retained.inner_frame(), &original);
    assert_eq!(retained.inner_frame().as_ptr(), original.as_ptr());
}

fn assert_signed_by(token: &str, key: &SecretKey) {
    let segments = token.split('.').collect::<Vec<_>>();
    assert_eq!(segments.len(), 3);
    let header = jsonwebtoken::decode_header(token).unwrap();
    let public_der = STANDARD.decode(header.x5u.unwrap()).unwrap();
    let public = PublicKey::from_public_key_der(&public_der).unwrap();
    assert_eq!(public, key.public_key());
    let signature_bytes = URL_SAFE_NO_PAD.decode(segments[2]).unwrap();
    let signature = Signature::from_slice(&signature_bytes).unwrap();
    VerifyingKey::from(&public)
        .verify(
            format!("{}.{}", segments[0], segments[1]).as_bytes(),
            &signature,
        )
        .unwrap();
}

#[test]
fn login_tokens_share_the_clients_identity_signing_key() {
    let config = ClientHandshakeConfig::random("127.0.0.1:19132".parse().unwrap(), "Fixture");
    let (chain, client) = crate::auth::client::generate_self_signed_chain(
        &config.identity_key,
        &config.display_name,
        config.uuid,
        None,
    )
    .unwrap();
    let identity: serde_json::Value = serde_json::from_str(&chain).unwrap();
    assert_signed_by(identity["chain"][0].as_str().unwrap(), &config.identity_key);
    assert_signed_by(&client, &config.identity_key);

    let mojang_header = URL_SAFE_NO_PAD.encode(br#"{"x5u":"fixture"}"#);
    let mojang_chain =
        serde_json::json!({"chain": [format!("{mojang_header}.e30.fixture")]}).to_string();
    let (chain, client) = crate::auth::client::encode_with_mojang_chain(
        &config.identity_key,
        &config.display_name,
        config.uuid,
        &mojang_chain,
        None,
    )
    .unwrap();
    let outer: serde_json::Value = serde_json::from_str(&chain).unwrap();
    let identity: serde_json::Value =
        serde_json::from_str(outer["Certificate"].as_str().unwrap()).unwrap();
    assert_signed_by(identity["chain"][0].as_str().unwrap(), &config.identity_key);
    assert_signed_by(&client, &config.identity_key);
}

fn report_samples(stage: &str, mut samples: Vec<Duration>) {
    let cold = samples.remove(0);
    samples.sort_unstable();
    eprintln!(
        "LOGIN_CPU_SAMPLE stage={stage} samples={} cold_us={:.3} median_us={:.3} p95_us={:.3}",
        samples.len(),
        cold.as_secs_f64() * 1e6,
        samples[samples.len() / 2].as_secs_f64() * 1e6,
        samples[samples.len() * 95 / 100].as_secs_f64() * 1e6,
    );
}

#[test]
fn offline_login_cpu_profile() {
    if std::env::var_os(PROFILE_ENV).is_none() {
        eprintln!("SKIP offline_login_cpu_profile: missing {PROFILE_ENV}");
        return;
    }
    let config = ClientHandshakeConfig::random("127.0.0.1:19132".parse().unwrap(), "Fixture");
    let mut signing = Vec::new();
    for _ in 0..PROFILE_SAMPLES {
        let started = Instant::now();
        let output = crate::auth::client::generate_self_signed_chain(
            &config.identity_key,
            &config.display_name,
            config.uuid,
            None,
        )
        .unwrap();
        std::hint::black_box(output);
        signing.push(started.elapsed());
    }
    report_samples("login_payload", signing);

    let raw = terrain_frame(TERRAIN_FRAME_BYTES);
    let mut deferral = Vec::new();
    for _ in 0..PROFILE_SAMPLES {
        let started = Instant::now();
        let mut queue = DeferredPackets::default();
        for _ in 0..16 {
            queue.push(raw.clone()).unwrap();
        }
        std::hint::black_box(queue.into_packets());
        deferral.push(started.elapsed());
    }
    report_samples("deferred_8_mib", deferral);
}
