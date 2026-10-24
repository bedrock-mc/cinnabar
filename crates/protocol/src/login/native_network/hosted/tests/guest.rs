//! Real signed Login bindings without another account, backend or transport owner.

use super::super::Guest;
use crate::login::native_network::tests::provider::{
    SERVICE_SECONDS, XUID, claims,
    hosted_upstream::{TIMEOUT, client_receive, client_send, finish_send},
    key, signed, verifier,
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use bedrock_client::{
    handshake::{ClientHandshake, HandshakeOptions, SettingsEvent},
    server::{
        AcceptedLogin, ServerApprovalEvent, ServerHandshakeError, ServerLoginEvent,
        ServerSettingsEvent, VerifiedLogin,
    },
};
use bedrock_client_tokio::server::test_support::{SignedPair, signed_pair_with_key};
use bedrock_net::{BatchCodec, BatchHeader, BatchLimits};
use bytes::Bytes;
use p384::{
    ecdsa::{Signature, SigningKey, signature::Signer as _},
    pkcs8::EncodePublicKey,
};
use serde_json::{json, value::RawValue};
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const NONCE: &str = "original-hosted-binding-nonce";

#[tokio::test]
async fn hosted_guest_binds_original_signed_client_bytes_before_utf8_repair() {
    tokio::time::timeout(TIMEOUT, async {
        let signer = Arc::new(key());
        let data = metadata(None);
        let offset = data.get().bytes().position(|byte| byte == b'!').unwrap();
        let mut guest: Option<Guest> = None;
        let mut interpreted = Vec::new();
        let mut original = Vec::new();
        let mut players = Vec::new();
        let mut cleanup = Vec::new();
        let mut authority = Vec::new();
        let mut same_original = false;
        let mut replacement_matches = false;
        for byte in [0xfe, 0xff] {
            let mut payload = data.get().as_bytes().to_vec();
            payload[offset] = byte;
            let (pair, login) = signed_login(
                Arc::clone(&signer),
                Arc::clone(&signer),
                &data,
                Some(&payload),
            )
            .await;
            let (accepted, approved) = approve(login.unwrap());
            let login = accepted.login();
            interpreted.push(login.client_data().get().to_owned());
            original.push(login.client_data_json_bytes().to_vec());
            let player = login.player_identity().unwrap();
            players.push((player.uuid(), player.display_name().to_owned()));
            authority.push(
                approved
                    && login.xuid() == XUID
                    && login.nonce() == NONCE
                    && login.signing_key() == signer.verifying_key()
                    && login.exchange_key().as_p384() == Some(signer.verifying_key())
                    && login.client_data_json_bytes() == payload.as_slice(),
            );
            match &guest {
                Some(guest) => replacement_matches = guest.matches(login),
                None => {
                    let original = Guest::new(login).unwrap();
                    same_original = original.matches(login);
                    guest = Some(original);
                }
            }
            drop(accepted);
            cleanup.push(pair.finish().await);
        }
        drop(guest);
        assert!(cleanup.into_iter().all(|joined| joined));
        assert!(authority.into_iter().all(|verified| verified));
        assert!(same_original);
        assert_eq!(interpreted[0], interpreted[1]);
        assert_ne!(original[0], original[1]);
        assert_eq!(players[0], players[1]);
        assert!(
            !replacement_matches,
            "a different original signed payload reused the hosted guest's repaired UTF8 view"
        );
    })
    .await
    .expect("finite actual hosted byte-binding fixture stalled");
}

#[tokio::test]
async fn hosted_guest_keeps_provider_signer_separate_from_exchange_and_transport() {
    tokio::time::timeout(TIMEOUT, async {
        let signer = Arc::new(key());
        let exchange = Arc::new(SigningKey::from_slice(&[2; 48]).unwrap());
        let replacement = Arc::new(SigningKey::from_slice(&[3; 48]).unwrap());
        let data = metadata(Some(&exchange));
        let (wrong_pair, wrong) =
            signed_login(Arc::clone(&signer), Arc::clone(&signer), &data, None).await;
        let wrong_transport = matches!(wrong, Err(ServerHandshakeError::TransportKeyMismatch));
        let wrong_cleanup = wrong_pair.finish().await;

        let (pair, login) =
            signed_login(Arc::clone(&exchange), Arc::clone(&signer), &data, None).await;
        let (accepted, approved) = approve(login.unwrap());
        let login = accepted.login();
        let roles = login.signing_key() == signer.verifying_key()
            && login.public_key() == signer.verifying_key()
            && login.exchange_key().as_p384() == Some(exchange.verifying_key())
            && login.xuid() == XUID;
        let guest = Guest::new(login).unwrap();
        let same_original = guest.matches(login);
        let original_payload = login.client_data_json_bytes().to_vec();
        let player = login.player_identity().unwrap();
        let original_player = (player.uuid(), player.display_name().to_owned());
        drop(accepted);
        let cleanup = pair.finish().await;

        let (replacement_pair, login) =
            signed_login(Arc::clone(&exchange), Arc::clone(&replacement), &data, None).await;
        let (accepted, replacement_approved) = approve(login.unwrap());
        let login = accepted.login();
        let replacement_roles = login.signing_key() == replacement.verifying_key()
            && login.exchange_key().as_p384() == Some(exchange.verifying_key())
            && login.xuid() == XUID
            && login.client_data_json_bytes() == original_payload.as_slice()
            && login.player_identity().is_some_and(|player| {
                (player.uuid(), player.display_name())
                    == (original_player.0, original_player.1.as_str())
            });
        let replacement_matches = guest.matches(login);
        drop(accepted);
        drop(guest);
        let replacement_cleanup = replacement_pair.finish().await;

        assert!(wrong_cleanup && cleanup && replacement_cleanup);
        assert!(
            wrong_transport,
            "the metadata signer replaced its signed transport key"
        );
        assert!(approved && replacement_approved && roles && replacement_roles && same_original);
        assert!(
            !replacement_matches,
            "a different provider signer reused the same exchange key and metadata"
        );
    })
    .await
    .expect("finite actual hosted signer/exchange fixture stalled");
}

/// Supplies valid synthetic metadata while leaving the opaque byte marker uninterpreted.
fn metadata(exchange: Option<&SigningKey>) -> Box<RawValue> {
    let mut data = json!({"DeviceOS":1,"GameVersion":crate::GAME_VERSION,"LanguageCode":"en_US",
        "SkinId":"synthetic","ThirdPartyName":"Probe","Nonce":NONCE,"opaque":"!"});
    if let Some(exchange) = exchange {
        data["identityPublicKey"] = STANDARD
            .encode(
                exchange
                    .verifying_key()
                    .to_public_key_der()
                    .unwrap()
                    .as_bytes(),
            )
            .into();
    }
    serde_json::value::to_raw_value(&data).unwrap()
}

/// Drives the original signed association/settings/codec owners before observing a real Login.
async fn signed_login(
    transport: Arc<SigningKey>,
    signer: Arc<SigningKey>,
    data: &RawValue,
    payload: Option<&[u8]>,
) -> (SignedPair, Result<VerifiedLogin, ServerHandshakeError>) {
    let deadline = Instant::now() + TIMEOUT;
    let (mut pair, handshake) =
        signed_pair_with_key(transport, crate::MAX_DEFERRED_PACKET_BYTES).await;
    let utc = service_time();
    let provider = verifier();
    let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
    let (client, request) = ClientHandshake::new(
        Arc::clone(&signer),
        HandshakeOptions {
            batch_encryption: false,
            ..Default::default()
        },
    )
    .unwrap()
    .start()
    .unwrap();
    client_send(&pair.client, &codec, vec![request.into()]).await;
    let ServerSettingsEvent::Settings(settings) = handshake
        .receive(
            pair.wire.receive(deadline).await.unwrap(),
            &provider,
            Instant::now(),
            utc,
        )
        .unwrap()
    else {
        panic!("actual signed settings continuation missing");
    };
    finish_send(
        pair.wire
            .prepare_packets(vec![settings.packet().clone().into()], deadline)
            .unwrap(),
    )
    .await;
    pair.wire
        .compression_flushed(settings.compression())
        .unwrap();
    let server = settings.flushed(Instant::now()).unwrap();
    let settings = client_receive(&mut pair.client, &codec).await.remove(0);
    let SettingsEvent::Ready {
        login: client,
        compression,
    } = client.receive(settings).unwrap()
    else {
        panic!("actual client settings continuation missing");
    };
    let codec = codec.negotiate(compression);
    let mut claims = claims();
    claims["cpk"] = STANDARD
        .encode(
            signer
                .verifying_key()
                .to_public_key_der()
                .unwrap()
                .as_bytes(),
        )
        .into();
    let (_, mut packet) = client
        .login_authenticated(&signed(claims, "synthetic"), data)
        .unwrap();
    if let Some(payload) = payload {
        packet.connection_request =
            replace_client_payload(&packet.connection_request, &signer, payload);
    }
    client_send(&pair.client, &codec, vec![packet.into()]).await;
    let result = server.receive(
        pair.wire.receive(deadline).await.unwrap(),
        &provider,
        Instant::now(),
        utc,
    );
    let result = result.map(|event| match event {
        ServerLoginEvent::Verified(login) => login,
        ServerLoginEvent::Unhandled { .. } => panic!("actual signed Login was not consumed"),
    });
    (pair, result)
}

/// Signs only replacement synthetic payload bytes; the original framed issuer envelope/header survive.
fn replace_client_payload(request: &[u8], signer: &SigningKey, payload: &[u8]) -> Bytes {
    let envelope = usize::try_from(u32::from_le_bytes(request[..4].try_into().unwrap())).unwrap();
    let offset = 4 + envelope;
    let length = usize::try_from(u32::from_le_bytes(
        request[offset..offset + 4].try_into().unwrap(),
    ))
    .unwrap();
    let original = std::str::from_utf8(&request[offset + 4..offset + 4 + length]).unwrap();
    let header = original.split_once('.').unwrap().0;
    let input = format!("{header}.{}", URL_SAFE_NO_PAD.encode(payload));
    let signature: Signature = signer.sign(input.as_bytes());
    let client = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
    let mut request = request[..offset].to_vec();
    request.extend_from_slice(&i32::try_from(client.len()).unwrap().to_le_bytes());
    request.extend_from_slice(client.as_bytes());
    Bytes::from(request)
}

/// Retains a real caller denial, then carries the same Login/grant through provider revalidation.
fn approve(login: VerifiedLogin) -> (AcceptedLogin<u8>, bool) {
    let deadline = login.deadline();
    let refused = login
        .authorize(|_| Err::<u8, _>(io::ErrorKind::PermissionDenied))
        .err()
        .expect("actual caller denial lost its verified Login");
    let (login, cause) = refused.into_parts();
    let retained = login.deadline() == deadline;
    let approved = login.authorize(|_| Ok::<_, io::Error>(7_u8)).unwrap();
    let ServerApprovalEvent::Ready(ready) = approved
        .accept(&verifier(), None, Instant::now(), service_time())
        .unwrap()
    else {
        panic!("DTLS-only owner unexpectedly required a second batch cipher");
    };
    let accepted = ready
        .finish(&verifier(), Instant::now(), service_time())
        .unwrap();
    let unchanged = retained
        && cause == io::ErrorKind::PermissionDenied
        && *accepted.grant() == 7
        && accepted.deadline() == deadline;
    (accepted, unchanged)
}

/// Reuses the original synthetic provider's service UTC without changing the transport clock.
fn service_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS)
}
