use super::*;
mod hosted_upstream;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use bedrock_net::login::encode_authenticated_request;
use nethernet::identity::{MINECRAFT_AUDIENCE, jwt::Jws};
use p384::{ecdsa::SigningKey, pkcs8::EncodePublicKey};
use rsa::{
    RsaPrivateKey, pkcs1v15,
    sha2::Sha256,
    signature::{SignatureEncoding, Signer},
    traits::PublicKeyParts,
};
use serde_json::{Value, json, value::RawValue};
use std::{
    sync::OnceLock,
    time::{Duration, Instant, UNIX_EPOCH},
};

const ISSUER: &str = "https://synthetic.example/tenant/";
const XUID: &str = "123";
const SERVICE_SECONDS: u64 = 1_900_000_000;

mod lan;

/// Generates an original synthetic issuer once; no real credential or private fixture is loaded.
fn issuer() -> &'static pkcs1v15::SigningKey<Sha256> {
    static ISSUER: OnceLock<pkcs1v15::SigningKey<Sha256>> = OnceLock::new();
    ISSUER.get_or_init(|| {
        pkcs1v15::SigningKey::new(RsaPrivateKey::new(&mut rand::rng(), 1024).unwrap())
    })
}

/// The connection key is independent of the issuer key.
fn key() -> SigningKey {
    SigningKey::from_slice(&[1; 48]).unwrap()
}

/// Publishes the original synthetic issuer's public key under one stable key ID.
fn published_keys() -> Value {
    let public = issuer().as_ref();
    json!({"keys":[{"kty":"RSA","kid":"synthetic",
        "n":URL_SAFE_NO_PAD.encode(public.n().to_be_bytes()), "e":URL_SAFE_NO_PAD.encode(public.e().to_be_bytes())}]})
}

/// Builds bounded trusted public metadata from the synthetic issuer, retaining a non-retail issuer.
fn verifier() -> NativeTokenVerifier {
    let keys = serde_json::to_vec(&published_keys()).unwrap();
    NativeTokenVerifier::new(ISSUER.into(), &["RS256".into()], &keys).unwrap()
}

/// Keeps subject distinct from Xbox identity: the configured verifier does not equate these claims.
fn claims() -> Value {
    json!({"iss":ISSUER,"aud":MINECRAFT_AUDIENCE,"sub":"synthetic-subject","xid":XUID,
        "xname":"Synthetic","iat":SERVICE_SECONDS,"exp":SERVICE_SECONDS+3600,
        "cpk":STANDARD.encode(key().verifying_key().to_public_key_der().unwrap().as_bytes())})
}

/// Signs original public test claims through the existing RSA primitive.
fn signed(claims: Value, kid: &str) -> String {
    signed_header(claims, json!({"alg":"RS256","kid":kid}))
}

/// Signs issuer-authentic header-policy violations without conflating them with invalid signatures.
fn signed_header(claims: Value, header: Value) -> String {
    signed_payload(&serde_json::to_vec(&claims).unwrap(), header)
}

/// Preserves exact JSON number spelling while signing through the existing synthetic issuer.
fn signed_payload(payload: &[u8], header: Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
    let payload = URL_SAFE_NO_PAD.encode(payload);
    let input = format!("{header}.{payload}");
    let signature: pkcs1v15::Signature = issuer().sign(input.as_bytes());
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_vec()))
}

/// Replaces one temporal claim without normalizing its decimal or exponent representation.
fn signed_temporal_claim(field: &str, value: &str) -> String {
    let mut claims = claims()
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                serde_json::value::to_raw_value(value).unwrap(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    claims.insert(field.into(), RawValue::from_string(value.into()).unwrap());
    signed_payload(
        &serde_json::to_vec(&claims).unwrap(),
        json!({"alg":"RS256","kid":"synthetic"}),
    )
}

/// Preserves the response's original observation rather than creating a remaining-lifetime TTL.
fn credential(token: String, observed: Instant, lifetime: Duration) -> NativeCredential {
    let service = UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS);
    NativeCredential::new(token, service, service + lifetime, observed).unwrap()
}

/// Exercises the production qualification entry point for other native Login tests.
pub(super) fn authentication() -> NativeAuthentication {
    authentication_at(Instant::now())
}

/// Raw-only absent identity does not turn malformed tokens or fingerprint proofs into unsigned answers.
#[test]
fn explicit_raw_answer_policy_keeps_present_provider_and_fingerprint_checks() {
    use bedrock_net::messaging::Recipient;
    use nethernet::identity::{SdpIdentity, TokenIdentity, TokenTrust, verify_answer_with_policy};
    let peer = uuid::Uuid::from_u128(7);
    let address = format!(
        "{}{}/{}",
        bridge::contract::NETHERNET_TARGET_PREFIX,
        bridge::contract::NETHERNET_SIGNALING_JSONRPC,
        peer
    );
    let target = crate::NativeRawTarget::parse(&address).unwrap().unwrap();
    let raw = authentication::MessagingTarget::Raw(target);
    assert_eq!(raw.nonce(), "");
    let service = authentication::MessagingTarget::Service(Recipient::Messaging(peer));
    assert!(!service.allow_anonymous_answers());
    let now = UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS);
    let trust = TokenTrust::Provider(std::sync::Arc::new(verifier()));
    let absent = "v=0\r\na=fingerprint:sha-256 AB:CD\r\nm=application 9\r\n";
    assert!(matches!(
        verify_answer_with_policy(absent, Some(&trust), raw.allow_anonymous_answers(), now),
        Ok(None)
    ));
    assert!(
        verify_answer_with_policy(absent, Some(&trust), service.allow_anonymous_answers(), now)
            .is_err()
    );
    let identity = TokenIdentity::new(
        std::sync::Arc::new(key()),
        signed(claims(), "synthetic"),
        now,
    )
    .unwrap();
    let valid = identity.augment(absent, now).unwrap();
    assert!(matches!(
        verify_answer_with_policy(&valid, Some(&trust), raw.allow_anonymous_answers(), now),
        Ok(Some(_))
    ));
    let untrusted = TokenIdentity::new(
        std::sync::Arc::new(key()),
        signed(claims(), "unknown-key"),
        now,
    )
    .unwrap()
    .augment(absent, now)
    .unwrap();
    for answer in [
        format!("{absent}a=identity:\r\n"),
        format!("{absent}a= identity \t\r\n"),
        format!("{absent}a=identity:not-an-assertion\r\n"),
        valid.replace("AB:CD", "AB:CE"),
        untrusted,
    ] {
        assert!(
            verify_answer_with_policy(&answer, Some(&trust), raw.allow_anonymous_answers(), now)
                .is_err()
        );
    }
}

/// Simulates holding an already-qualified handoff without sleeping or changing a production clock.
fn authentication_at(observed: Instant) -> NativeAuthentication {
    let client =
        RawValue::from_string(r#"{"ThirdPartyName":"stale","DeviceOS":1}"#.into()).unwrap();
    NativeAuthentication::new(
        key(),
        credential(
            signed(claims(), "synthetic"),
            observed,
            Duration::from_secs(60),
        ),
        &client,
        &verifier(),
        XUID,
        observed,
    )
    .unwrap()
}

/// A remote peer has its own Xbox identity and proof; issuer policy must not substitute the local account.
#[test]
fn native_messaging_peer_policy_preserves_remote_proof_and_fingerprint_binding() {
    use nethernet::identity::{SdpIdentity, TokenIdentity, TokenTrust, validate_sdp};
    let now = UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS);
    let peer_key = std::sync::Arc::new(SigningKey::from_slice(&[2; 48]).unwrap());
    let mut peer = claims();
    peer["xid"] = json!("456");
    peer["cpk"] = json!(
        STANDARD.encode(
            peer_key
                .verifying_key()
                .to_public_key_der()
                .unwrap()
                .as_bytes()
        )
    );
    let token = signed(peer, "synthetic");
    let identity =
        TokenIdentity::new(std::sync::Arc::clone(&peer_key), token.clone(), now).unwrap();
    assert!(std::ptr::eq(identity.signing_key(), peer_key.as_ref()));
    let sdp = identity
        .augment(
            "v=0\r\na=fingerprint:sha-256 AB:CD\r\nm=application 9\r\n",
            now,
        )
        .unwrap();
    let trust = TokenTrust::Provider(std::sync::Arc::new(verifier()));
    let checked = validate_sdp(&sdp, &trust, now).unwrap();
    assert_eq!(checked.xuid(), Some("456"));
    assert_eq!(
        checked.client_public_key().unwrap(),
        *peer_key.verifying_key()
    );
    assert!(validate_sdp(&sdp.replace("AB:CD", "EF:01"), &trust, now).is_err());
    assert!(
        NativeAuthentication::new(
            key(),
            credential(token, Instant::now(), Duration::from_secs(60)),
            &RawValue::from_string("{}".into()).unwrap(),
            &verifier(),
            XUID,
            Instant::now()
        )
        .is_err()
    );
}

/// The actual public Login entry rejects a delayed handoff before any endpoint work.
#[tokio::test]
async fn native_provider_expired_handoff_cannot_start_a_connection() {
    let settings = crate::LoginSettings::default();
    let observed = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
    let result = LoginSequence::connect_native_authenticated(
        &NativeConnectionTarget::new("unresolved.invalid:0".into(), NativeTransport::RakNet),
        SessionLogin {
            display_name: "Synthetic",
            client_random_id: crate::ClientRandomId::new(1).unwrap(),
            cache: None,
            skin: None,
            settings: &settings,
            tap: None,
            #[cfg(not(target_arch = "wasm32"))]
            relay: None,
            target: None,
        },
        authentication_at(observed),
        |_, _| (),
    )
    .await;
    assert!(
        matches!(result, Err(ProtocolError::Authentication(error)) if error.kind() == io::ErrorKind::PermissionDenied)
    );
}

/// A matching key ID cannot make malformed RSA material a successful cached verifier.
#[test]
fn native_provider_rejects_malformed_matching_key_before_cache_admission() {
    let token = signed(claims(), "synthetic");
    assert!(verifier().recognizes(&token).unwrap());
    for (field, value) in [("n", "***"), ("e", "***"), ("n", ""), ("e", "")] {
        let mut keys = published_keys();
        keys["keys"][0][field] = json!(value);
        let result = NativeTokenVerifier::new(
            ISSUER.into(),
            &["RS256".into()],
            &serde_json::to_vec(&keys).unwrap(),
        );
        assert!(result.is_err(), "admitted malformed RSA {field}");
    }
}

/// Equivalent temporal representations retain second truncation and the original hard deadline.
#[test]
fn native_provider_numeric_dates_preserve_representations_and_deadlines() {
    let observed = Instant::now();
    let client = RawValue::from_string("{}".into()).unwrap();
    let verifier = verifier();
    for field in ["exp", "iat", "nbf"] {
        let seconds = SERVICE_SECONDS + if field == "exp" { 5 } else { 60 };
        for value in [
            seconds.to_string(),
            format!("{seconds}.9"),
            format!("{seconds}9e-1"),
        ] {
            let authentication = NativeAuthentication::new(
                key(),
                credential(
                    signed_temporal_claim(field, &value),
                    observed,
                    Duration::from_secs(60),
                ),
                &client,
                &verifier,
                XUID,
                observed,
            )
            .unwrap_or_else(|error| panic!("rejected {field}={value}: {error}"));
            let lifetime = Duration::from_secs(if field == "exp" { 5 } else { 60 });
            assert_eq!(authentication.expires, observed + lifetime);
        }
    }
}

/// NumericDate compatibility must retain controlled rejection of invalid values and time bounds.
#[test]
fn native_provider_numeric_dates_reject_invalid_values_and_time_bounds() {
    let observed = Instant::now();
    let client = RawValue::from_string("{}".into()).unwrap();
    let verifier = verifier();
    for field in ["exp", "iat", "nbf"] {
        let boundary = SERVICE_SECONDS + if field == "exp" { 0 } else { 61 };
        for value in [
            "\"invalid\"".into(),
            "true".into(),
            "{}".into(),
            "[]".into(),
            "1e100".into(),
            "-1e100".into(),
            "9223372036854775808".into(),
            "-9223372036854775809".into(),
            format!("{boundary}.9"),
            format!("{boundary}9e-1"),
        ] {
            let result = NativeAuthentication::new(
                key(),
                credential(
                    signed_temporal_claim(field, &value),
                    observed,
                    Duration::from_secs(60),
                ),
                &client,
                &verifier,
                XUID,
                observed,
            );
            assert!(
                matches!(result, Err(error) if error.kind() == io::ErrorKind::PermissionDenied),
                "accepted invalid {field}={value}"
            );
        }
    }
}

/// Signature, issuer, account/key identity and temporal claims are all part of the real handoff.
#[test]
fn native_provider_rejects_untrusted_identity_and_temporal_contracts() {
    let observed = Instant::now();
    let client = RawValue::from_string("{}".into()).unwrap();
    let verifier = verifier();
    for (field, value) in [
        ("iss", json!("https://another.example/")),
        ("aud", json!("another-service")),
        ("aud", json!([MINECRAFT_AUDIENCE, 7])),
        ("sub", json!("")),
        ("sub", Value::Null),
        ("xid", json!("456")),
        ("cpk", json!("not-a-key")),
        ("exp", json!(SERVICE_SECONDS)),
        ("exp", Value::Null),
        ("nbf", json!(SERVICE_SECONDS + 61)),
        ("iat", json!(SERVICE_SECONDS + 61)),
        ("leguuid", json!("invalid")),
        ("xname", json!("1Invalid")),
    ] {
        let mut claims = claims();
        claims[field] = value;
        let result = NativeAuthentication::new(
            key(),
            credential(
                signed(claims, "synthetic"),
                observed,
                Duration::from_secs(60),
            ),
            &client,
            &verifier,
            XUID,
            observed,
        );
        assert!(result.is_err(), "accepted invalid {field}");
    }
    let token = signed(claims(), "not-published");
    assert!(!verifier.recognizes(&token).unwrap());
    assert!(
        NativeAuthentication::new(
            key(),
            credential(token, observed, Duration::from_secs(60)),
            &client,
            &verifier,
            XUID,
            observed
        )
        .is_err()
    );
    let token = signed(claims(), "synthetic");
    let (input, signature) = token.rsplit_once('.').unwrap();
    let mut bytes = URL_SAFE_NO_PAD.decode(signature).unwrap();
    bytes[0] ^= 1;
    let untrusted = format!("{input}.{}", URL_SAFE_NO_PAD.encode(bytes));
    assert!(
        NativeAuthentication::new(
            key(),
            credential(untrusted, observed, Duration::from_secs(60)),
            &client,
            &verifier,
            XUID,
            observed
        )
        .is_err()
    );
    let other = SigningKey::from_slice(&[2; 48]).unwrap();
    assert!(
        NativeAuthentication::new(
            other,
            credential(token, observed, Duration::from_secs(60)),
            &client,
            &verifier,
            XUID,
            observed
        )
        .is_err()
    );
    let mut optional = claims();
    optional["nbf"] = Value::Null;
    optional["iat"] = Value::Null;
    optional["leguuid"] = Value::Null;
    assert!(
        NativeAuthentication::new(
            key(),
            credential(signed(optional, ""), observed, Duration::from_secs(60)),
            &client,
            &verifier,
            XUID,
            observed
        )
        .is_ok()
    );
    for header in [
        json!({"alg":"RS256","kid":"synthetic","crit":["unknown"]}),
        json!({"alg":"RS256","kid":"synthetic","b64":false}),
        json!({"alg":"HS256","kid":"synthetic"}),
    ] {
        let service = UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS);
        assert!(
            NativeCredential::new(
                signed_header(claims(), header),
                service,
                service + Duration::from_secs(60),
                observed
            )
            .is_err()
        );
    }
}

/// Provider/transport delay cannot extend the mint's hard validity or change the Login proof key.
#[test]
fn native_provider_preserves_hard_expiry_and_the_actual_login_token_and_key() {
    let observed = Instant::now();
    let client = RawValue::from_string(r#"{"ThirdPartyName":"stale"}"#.into()).unwrap();
    let token = signed(claims(), "synthetic");
    let authentication = NativeAuthentication::new(
        key(),
        credential(token.clone(), observed, Duration::from_secs(5)),
        &client,
        &verifier(),
        XUID,
        observed + Duration::from_secs(4),
    )
    .unwrap();
    assert_eq!(authentication.expires, observed + Duration::from_secs(5));
    assert_eq!(authentication.claims.display_name().unwrap(), "Synthetic");
    assert!(
        authentication::ensure_current(authentication.expires, observed + Duration::from_secs(5))
            .is_err()
    );
    assert!(
        NativeAuthentication::new(
            key(),
            credential(token.clone(), observed, Duration::from_secs(5)),
            &client,
            &verifier(),
            XUID,
            observed + Duration::from_secs(5)
        )
        .is_err()
    );

    let client = serde_json::value::to_raw_value(&authentication.claims).unwrap();
    let request = encode_authenticated_request(
        &authentication.key,
        &authentication.token,
        &client,
        bedrock_net::BatchLimits::default().max_decoded_bytes,
    )
    .unwrap();
    let length = u32::from_le_bytes(request[..4].try_into().unwrap()) as usize;
    let envelope: Value = serde_json::from_slice(&request[4..4 + length]).unwrap();
    assert_eq!(envelope["Token"], token);
    let rest = &request[4 + length..];
    let length = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
    let client = Jws::parse(std::str::from_utf8(&rest[4..4 + length]).unwrap()).unwrap();
    client.verify_es384(key().verifying_key()).unwrap();
    assert!(
        client
            .verify_es384(SigningKey::from_slice(&[2; 48]).unwrap().verifying_key())
            .is_err()
    );
}

/// The actual provider qualifier and Login signer retain raw values under the original proof/key.
#[test]
fn native_provider_raw_claims_keep_canonical_identity_and_signed_opaque_values() {
    let observed = Instant::now();
    let client = RawValue::from_string(r#"{"Unknown":1e99999,"ClientRandomId":0,"ThirdPartyName":"stale","thirdpartyname":"alias","ſerverAddreſſ":"stale","nonce":"stale"}"#.into()).unwrap();
    let authentication = NativeAuthentication::new_relay(
        key(),
        credential(
            signed(claims(), "synthetic"),
            observed,
            Duration::from_secs(5),
        ),
        crate::RawClientData::from_raw(&client).unwrap(),
        &verifier(),
        XUID,
        observed,
    )
    .unwrap();
    assert_eq!(authentication.expires, observed + Duration::from_secs(5));
    assert_eq!(authentication.claims.display_name().unwrap(), "Synthetic");
    let mut handoff = authentication.into_handoff().unwrap();
    handoff
        .claims
        .set_transport("selected.example:19132", "original-nonce")
        .unwrap();
    let raw = handoff.claims.into_raw().unwrap();
    let request = encode_authenticated_request(
        &handoff.key,
        &handoff.token,
        &raw,
        bedrock_net::BatchLimits::default().max_decoded_bytes,
    )
    .unwrap();
    let length = u32::from_le_bytes(request[..4].try_into().unwrap()) as usize;
    let rest = &request[4 + length..];
    let length = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
    let client = Jws::parse(std::str::from_utf8(&rest[4..4 + length]).unwrap()).unwrap();
    client.verify_es384(handoff.key.verifying_key()).unwrap();
    assert_eq!(client.payload, [raw.get().as_bytes(), b"\n"].concat());
    let projected = crate::RawClientData::from_raw(&raw).unwrap();
    assert_eq!(
        projected.display_name().unwrap().as_deref(),
        Some("Synthetic")
    );
    assert_eq!(projected.random_id().unwrap(), Some(0));
    assert!(raw.get().contains(r#""Unknown":1e99999"#));
    assert!(!raw.get().contains("stale"));
    assert!(!raw.get().contains("alias"));
}

/// Host SDP and accepting Login share the original proof allocation and provider-bound hard validity.
#[test]
fn native_hosting_identity_keeps_selected_proof_and_service_validity() {
    use crate::NativeHostingIdentity;
    use nethernet::identity::{Identity, validate_sdp};
    use std::sync::Arc;

    let observed = Instant::now();
    let token = signed(claims(), "synthetic");
    let proof = Arc::new(key());
    let verifier = verifier();
    let lifetime = Duration::from_secs(60);
    let host = NativeHostingIdentity::new(
        Arc::clone(&proof),
        credential(token.clone(), observed, lifetime),
        &verifier,
        XUID,
        observed,
    )
    .unwrap();
    assert!(Arc::ptr_eq(&proof, &host.signing_key()));
    assert_eq!(host.expires_at(), observed + lifetime);
    let sdp = host
        .sdp_identity()
        .augment(
            "v=0\r\na=fingerprint:sha-256 AB:CD\r\nm=application 9\r\n",
            UNIX_EPOCH,
        )
        .unwrap();
    assert_eq!(Identity::from_sdp(&sdp).unwrap().assertion.token, token);
    let checked = validate_sdp(&sdp, &host.offer_trust(), UNIX_EPOCH).unwrap();
    assert_eq!(checked.xuid(), Some(XUID));
    assert_eq!(checked.client_public_key().unwrap(), *proof.verifying_key());
    assert!(
        validate_sdp(
            &sdp.replace("AB:CD", "EF:01"),
            &host.offer_trust(),
            UNIX_EPOCH
        )
        .is_err()
    );
    for (proof, xuid, now) in [
        (
            Arc::new(SigningKey::from_slice(&[2; 48]).unwrap()),
            XUID,
            observed,
        ),
        (Arc::clone(&proof), "456", observed),
        (Arc::clone(&proof), XUID, observed + lifetime),
    ] {
        assert!(
            NativeHostingIdentity::new(
                proof,
                credential(token.clone(), observed, lifetime),
                &verifier,
                xuid,
                now,
            )
            .is_err()
        );
    }
}
