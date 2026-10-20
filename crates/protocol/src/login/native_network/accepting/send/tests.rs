//! Cancellation and cipher continuity on actual signed accepted HTTP/DTLS/SCTP owners.

use super::*;
use bedrock_client::server::ServerHandshakeOptions;
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, DecodeOutcome,
    encryption::{BatchDecryptor, BatchEncryptor},
};
use bedrock_protocol_1_26_51::{
    packets::SetTime,
    wire::{self, Encode},
};
use nethernet::identity::{
    TokenVerifier,
    error::{IdentityError, Result as IdentityResult},
    jwt::{Claims, Jws},
};
use nethernet_tokio::{
    ClientIceConfig, ConnectionConfig, HttpServerConfig, HttpSignaling, HttpSignalingServer,
    NetherClient, NetherServer, SelfSignedIdentity, SendPermit, ServerIceConfig, ServerIdentity,
    SessionReceiver, SessionSender, TokenTrust,
};
use p384::{
    ecdsa::{SigningKey, VerifyingKey},
    elliptic_curve::Generate,
};
use std::{
    future::poll_fn,
    net::Ipv4Addr,
    pin::pin,
    sync::Arc,
    task::Poll,
    time::{Duration, SystemTime},
};

const TIMEOUT: Duration = Duration::from_secs(20);
const SERVER: &str = "synthetic-prepared-send-server";
const MARKER: &[u8] = b"prepared-send-reliable-marker";

#[tokio::test]
async fn actual_accepting_owner_keeps_the_original_incoming_packet_count_policy() {
    tokio::time::timeout(TIMEOUT, async {
        let mut pair = signed_pair().await;
        let limits = BatchLimits::default();
        let count = limits.max_packets + 1;
        let packets = vec![raw_packet(1); count];
        let mut messages = BatchCodec::new(
            BatchHeader::NetherNet,
            BatchLimits {
                max_packets: count,
                ..limits
            },
        )
        .encode(&packets)
        .unwrap();
        assert_eq!(messages.len(), 1);
        pair.client.send(messages.remove(0)).await.unwrap();
        let received = pair
            .wire
            .receive_raw(tokio::time::Instant::now().into_std() + TIMEOUT)
            .await;
        assert!(pair.finish().await, "original signed accepting owners leaked");
        let error = received.expect_err("the serving role lost its original incoming count policy");
        assert!(matches!(
            error.get_ref().and_then(|error| error.downcast_ref::<bedrock_net::BatchError>()),
            Some(bedrock_net::BatchError::TooManyPackets { maximum }) if *maximum == limits.max_packets
        ));
    })
    .await
    .expect("finite actual accepting count policy fixture stalled");
}

#[tokio::test]
async fn cancelled_borrows_retain_reservation_permit_receipt_and_cipher_order() {
    tokio::time::timeout(TIMEOUT, async {
        let mut pair = signed_pair().await;
        let deadline = tokio::time::Instant::now().into_std() + TIMEOUT;
        // Exercise the shared Connection cipher owner directly; public from_nether remains DTLS-only.
        let key = zeroize::Zeroizing::new(rand::random::<[u8; 32]>());
        pair.wire
            .connection
            .enable_encryption((
                BatchEncryptor::new(*key),
                BatchDecryptor::new(rand::random()),
            ))
            .unwrap();
        let mut decryptor = BatchDecryptor::new(*key);
        let first = raw_packet(1);
        let second = raw_packet(2);
        let mut held = fill_capacity(&pair.sender).await;
        let empty_complete = {
            let mut empty = pair.wire.prepare_packets(Vec::new(), deadline).unwrap();
            let complete = empty.next().await.unwrap().is_none();
            empty.finish().unwrap();
            complete
        };
        let reserve_pending = {
            let mut sending = pair.wire.prepare_raw(first.clone(), deadline).unwrap();
            let mut next = pin!(sending.next());
            poll_fn(|cx| Poll::Ready(next.as_mut().poll(cx).is_pending())).await
        };
        let mut later = Box::pin(pair.sender.reserve_send(None));
        let later_pending = poll_fn(|cx| Poll::Ready(later.as_mut().poll(cx).is_pending())).await;
        drop(held.pop());
        let same_reservation_granted = {
            let mut sending = pair.wire.resume_send().unwrap();
            let granted = {
                let mut next = pin!(sending.next());
                poll_fn(|cx| {
                    Poll::Ready(matches!(next.as_mut().poll(cx), Poll::Ready(Ok(Some(_)))))
                })
                .await
            };
            drop(later);
            {
                let _capacity = sending.next().await.unwrap().unwrap();
            }
            granted
        };
        let permit_retained = {
            let mut extra = pin!(pair.sender.reserve_send(None));
            poll_fn(|cx| Poll::Ready(extra.as_mut().poll(cx).is_pending())).await
        };
        let receipt_pending = {
            let mut sending = pair.wire.resume_send().unwrap();
            sending.next().await.unwrap().unwrap().commit().unwrap();
            // Current-thread execution has not yielded since synchronous commit, so the real actor reply is pending.
            let mut next = pin!(sending.next());
            poll_fn(|cx| Poll::Ready(next.as_mut().poll(cx).is_pending())).await
        };
        let mut sending = pair.wire.resume_send().unwrap();
        let original_deadline = sending.deadline();
        let completed = sending.next().await.unwrap().is_none();
        sending.finish().unwrap();
        let first_received =
            decode_ciphertext(&mut decryptor, pair.client.recv().await.unwrap().unwrap());

        {
            let mut sending = pair.wire.prepare_raw(second.clone(), deadline).unwrap();
            sending.next().await.unwrap().unwrap().commit().unwrap();
        }
        let settlement_pending = {
            let mut retirement = pair.wire.begin_send_retirement(deadline);
            let mut finish = pin!(retirement.finish());
            poll_fn(|cx| Poll::Ready(finish.as_mut().poll(cx).is_pending())).await
        };
        let outcome = pair
            .wire
            .begin_send_retirement(deadline + TIMEOUT)
            .finish()
            .await
            .unwrap();
        let repeated = pair
            .wire
            .begin_send_retirement(deadline + TIMEOUT)
            .finish()
            .await
            .unwrap();
        let second_received =
            decode_ciphertext(&mut decryptor, pair.client.recv().await.unwrap().unwrap());
        pair.wire.resume_send().unwrap().finish().unwrap();
        let sealed = pair.wire.prepare_raw(raw_packet(3), deadline).is_err();
        drop(held);
        pair.sender
            .send_before(Bytes::from_static(MARKER), deadline)
            .await
            .unwrap();
        let marker = pair.client.recv().await.unwrap().unwrap();
        let cleanup = pair.finish().await;
        assert!(
            cleanup,
            "actual original transport cleanup did not complete"
        );
        assert!(
            reserve_pending && later_pending && same_reservation_granted && permit_retained,
            "cancellation lost the actual fair reservation or retained permit"
        );
        assert!(
            receipt_pending && settlement_pending,
            "the actual committed receipt was not observed pending"
        );
        assert!(empty_complete && completed && original_deadline == deadline && sealed);
        assert_eq!(outcome, NativeAcceptingSendOutcome::Complete);
        assert_eq!(repeated, outcome);
        assert_eq!(first_received, [first]);
        assert_eq!(second_received, [second]);
        assert_eq!(marker.as_ref(), MARKER);
    })
    .await
    .expect("finite actual prepared-send cancellation/cipher fixture stalled");
}

#[tokio::test]
async fn original_granted_capacity_cutoff_cannot_be_renewed_or_commit_after_retirement() {
    tokio::time::timeout(TIMEOUT, async {
        let mut pair = signed_pair().await;
        tokio::time::pause();
        let cutoff = tokio::time::Instant::now().into_std() + Duration::from_millis(10);
        let expired = {
            let mut sending = pair.wire.prepare_raw(raw_packet(1), cutoff).unwrap();
            let capacity = sending.next().await.unwrap().unwrap();
            tokio::time::advance(Duration::from_millis(11)).await;
            capacity.commit().unwrap_err().kind()
        };
        let first = pair
            .wire
            .begin_send_retirement(cutoff + TIMEOUT)
            .finish()
            .await
            .unwrap_err()
            .kind();
        let repeated = pair
            .wire
            .begin_send_retirement(cutoff + TIMEOUT + TIMEOUT)
            .finish()
            .await
            .unwrap_err()
            .kind();
        let sealed = pair
            .wire
            .prepare_raw(raw_packet(2), cutoff + TIMEOUT)
            .is_err();
        tokio::time::resume();
        pair.sender.send(Bytes::from_static(MARKER)).await.unwrap();
        let marker = pair.client.recv().await.unwrap().unwrap();
        let cleanup = pair.finish().await;
        assert!(
            cleanup,
            "actual original transport cleanup did not complete"
        );
        assert_eq!(expired, io::ErrorKind::TimedOut);
        assert_eq!(first, expired);
        assert_eq!(repeated, expired);
        assert!(sealed);
        assert_eq!(
            marker.as_ref(),
            MARKER,
            "an expired original packet preceded the reliable marker"
        );
    })
    .await
    .expect("finite actual prepared-send cutoff fixture stalled");
}

#[tokio::test]
async fn failed_original_receipt_is_cached_without_repolling_its_completed_future() {
    tokio::time::timeout(TIMEOUT, async {
        let mut pair = signed_pair().await;
        let deadline = tokio::time::Instant::now().into_std() + TIMEOUT;
        let first = {
            let mut sending = pair.wire.prepare_raw(raw_packet(1), deadline).unwrap();
            sending.next().await.unwrap().unwrap().commit().unwrap();
            // No yield follows commit: actual global close reaches the actor before its queued command.
            pair.sender.start_close();
            sending
                .next()
                .await
                .err()
                .expect("original actor refused the closed command")
                .kind()
        };
        let repeated = pair
            .wire
            .resume_send()
            .unwrap()
            .next()
            .await
            .err()
            .expect("original failure is retained")
            .kind();
        let retired = pair
            .wire
            .begin_send_retirement(deadline)
            .finish()
            .await
            .expect_err("failed receipt cannot prove complete")
            .kind();
        let retired_again = pair
            .wire
            .begin_send_retirement(deadline + TIMEOUT)
            .finish()
            .await
            .expect_err("settlement failure is retained")
            .kind();
        let cleanup = pair.finish().await;
        assert!(
            cleanup,
            "actual original transport cleanup did not complete"
        );
        assert_eq!(repeated, first);
        assert_eq!(retired, first);
        assert_eq!(retired_again, first);
    })
    .await
    .expect("finite actual failed receipt fixture stalled");
}

pub(in crate::login::native_network) struct Pair {
    pub(in crate::login::native_network) wire: NativeAcceptingIo,
    pub(in crate::login::native_network) sender: SessionSender,
    pub(in crate::login::native_network) client: NetherClient,
    pub(in crate::login::native_network) listener: NetherServer,
    pub(in crate::login::native_network) signaling: Arc<HttpSignalingServer>,
}

impl Pair {
    /// Retires and drains actual owners, then joins listener/HTTP tasks; no close token is used as join.
    pub(in crate::login::native_network) async fn finish(self) -> bool {
        let Self {
            wire,
            sender,
            client,
            mut listener,
            signaling,
        } = self;
        let deadline = tokio::time::Instant::now().into_std() + TIMEOUT;
        let mut wire = wire.begin_close();
        let mut client = client.close();
        wire.finish_close(deadline).await.unwrap();
        let server_done = matches!(wire.receive_raw(deadline).await, Err(error) if error.kind() == io::ErrorKind::UnexpectedEof);
        let reliable = drain(&mut client.reliable).await;
        let unreliable = drain(&mut client.unreliable).await;
        drop(wire);
        drop(client);
        drop(sender);
        listener.close().await.unwrap();
        drop(listener);
        let joined = match Arc::try_unwrap(signaling) {
            Ok(signaling) => {
                signaling.shutdown().await;
                true
            }
            Err(signaling) => {
                drop(signaling);
                false
            }
        };
        server_done && reliable && unreliable && joined
    }
}

/// Establishes the real signed association before tests retain any application queue capacity.
async fn signed_pair() -> Pair {
    signed_pair_with_key(Arc::new(SigningKey::generate()))
        .await
        .0
}

/// Retains the original handshake for real provider Login/startup controls on this association.
pub(in crate::login::native_network) async fn signed_pair_with_key(
    client_key: Arc<SigningKey>,
) -> (Pair, bedrock_client::server::ServerHandshake) {
    let now = SystemTime::now();
    let key = Arc::new(SigningKey::generate());
    let signaling = Arc::new(
        HttpSignalingServer::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            HttpServerConfig {
                network_id: SERVER.into(),
                signaler: nethernet::signaling::http::config::HttpSignalerConfig {
                    token_trust: Some(trust(*client_key.verifying_key(), None)),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
    let mut listener = NetherServer::bind_with(
        Arc::clone(&signaling),
        ConnectionConfig {
            identity: Some(Arc::new(
                ServerIdentity::from_key((*key).clone(), SERVER, now, Some(TIMEOUT)).unwrap(),
            )),
            server_ice: Some(ServerIceConfig {
                turn_streams: Vec::new(),
                turn_tls: None,
                ice: Default::default(),
                bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
            }),
            attempts: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let client = NetherClient::connect_with(
        HttpSignaling::new("synthetic-prepared-send-client".into()).unwrap(),
        format!("http://{}", signaling.local_addr()),
        ConnectionConfig {
            identity: Some(Arc::new(
                SelfSignedIdentity::from_key(client_key, now, TIMEOUT).unwrap(),
            )),
            server_trust: Some(trust(*key.verifying_key(), Some(SERVER))),
            client_ice: Some(ClientIceConfig {
                turn_streams: Vec::new(),
                turn_tls: None,
                ice: Default::default(),
                bind_addr: Some((Ipv4Addr::LOCALHOST, 0).into()),
            }),
            attempts: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let accepted = listener.accept().await.unwrap();
    let sender = accepted.session.sender();
    let (wire, handshake) = NativeAcceptingIo::from_nether(
        accepted,
        key,
        ServerHandshakeOptions::default(),
        tokio::time::Instant::now().into_std() + TIMEOUT,
    )
    .await
    .unwrap();
    (
        Pair {
            wire,
            sender,
            client,
            listener,
            signaling,
        },
        handshake,
    )
}

struct ExpectedToken {
    key: VerifyingKey,
    issuer: Option<&'static str>,
}

impl TokenVerifier for ExpectedToken {
    fn verify(&self, token: &str, now: SystemTime) -> IdentityResult<Claims> {
        let token = Jws::parse(token)?;
        if token.header.alg != "ES384" {
            return Err(IdentityError::Untrusted(
                "unexpected fixture algorithm".into(),
            ));
        }
        token.verify_es384(&self.key)?;
        let claims = token.claims()?;
        claims.check_expiry(now)?;
        if claims.client_public_key()? != self.key
            || claims.issuer() != self.issuer
            || !claims.audience().is_empty()
        {
            return Err(IdentityError::Untrusted(
                "unexpected fixture identity".into(),
            ));
        }
        Ok(claims)
    }
}

/// Trusts the original local proof key and fixture issuer, without a production policy fallback.
fn trust(key: VerifyingKey, issuer: Option<&'static str>) -> TokenTrust {
    TokenTrust::Provider(Arc::new(ExpectedToken { key, issuer }))
}

/// Retains actual public permits until another real reservation is Pending.
async fn fill_capacity(sender: &SessionSender) -> Vec<SendPermit> {
    let mut held = Vec::new();
    loop {
        let mut reservation = pin!(sender.reserve_send(None));
        match poll_fn(|cx| Poll::Ready(reservation.as_mut().poll(cx))).await {
            Poll::Ready(Ok(permit)) => held.push(permit),
            Poll::Ready(Err(error)) => panic!("actual reservation failed: {error}"),
            Poll::Pending => return held,
        }
    }
}

/// Serializes legal distinct payloads using the generated packet/header owner.
fn raw_packet(time: i32) -> Bytes {
    let mut bytes = Vec::new();
    let mut writer = wire::Writer::with_limit(&mut bytes, BatchLimits::default().max_decoded_bytes);
    writer
        .write_var_u32(wire::PacketHeader::new(SetTime::ID).to_raw())
        .unwrap();
    SetTime {
        time: wire::ZigZag32(time),
    }
    .encode(&mut writer)
    .unwrap();
    Bytes::from(bytes)
}

/// Uses one original peer decryptor across both exact wire messages.
fn decode_ciphertext(decryptor: &mut BatchDecryptor, message: Bytes) -> Vec<Bytes> {
    let plaintext = decryptor.decrypt(message).unwrap();
    match BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default())
        .decode(plaintext)
        .unwrap()
    {
        DecodeOutcome::Packets { packets, .. } => packets.collect(),
        DecodeOutcome::Dropped(_) => panic!("plaintext fixture message was dropped"),
    }
}

/// Drains original receive queues through their terminal and records unexpected receive errors.
async fn drain(receiver: &mut SessionReceiver) -> bool {
    let mut orderly = true;
    loop {
        match receiver.recv().await {
            Ok(Some(_)) => {}
            Ok(None) => return orderly,
            Err(_) => orderly = false,
        }
    }
}
