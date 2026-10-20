//! Actual original membership and the same signed reliable transport's bounded command queue.

use super::*;
use crate::native_account::ACQUISITION_TIMEOUT;
use bedrock_client::server::ServerHandshakeOptions;
use bedrock_net::{BatchCodec, BatchHeader, BatchLimits, DecodeOutcome};
use bytes::Bytes;
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
use protocol::NativeAcceptingIo;
use std::{future::poll_fn, net::Ipv4Addr, pin::pin, task::Poll, time::SystemTime};

const SERVER: &str = "synthetic-hosted-send-server";
const MARKER: &[u8] = b"owned-reliable-admission-marker";

#[tokio::test]
async fn revoked_original_member_cannot_commit_after_external_send_capacity_wait() {
    tokio::time::timeout(ACQUISITION_TIMEOUT, async {
        let fixture = NativeHostFixture::new();
        let namespace = tempfile::tempdir().unwrap();
        let config = minecraft::Config::desktop(
            protocol::GAME_VERSION,
            u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            metadata::service_device_id("123").unwrap(),
        );
        let HostedAuthority {
            account: services,
            identity,
            mut peer,
        } = HostedAuthority::acquire_revocation(authentication_policy(), config).await;
        let store = AccountStore::new(namespace.path().join("synthetic-oauth.json"));
        let credentials = services.credentials();
        let expiry = oauth_expiry(credentials.expires_at()).unwrap();
        let checkpoint = services.checkpoint().unwrap();
        let pending = transaction("reserve external send fixture sign-in", || {
            store.try_begin_native_sign_in("host-external-send-generation", None)
        });
        transaction("publish external send fixture OAuth", || {
            pending.try_publish(
                credentials.access_token(),
                credentials.refresh_token(),
                &expiry,
            )
        });
        transaction("publish external send fixture checkpoint", || {
            pending.try_publish_checkpoint(checkpoint.as_bytes())
        });
        transaction("commit external send fixture sign-in", || {
            pending.try_commit(
                &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
            )
        });
        let account = Arc::new(NativeAccount::new(
            store,
            namespace.path().join("synthetic-device.json"),
            Some(identity.xuid().into()),
        ));
        install(&account, services, &identity);
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let mut host = fixture
            .manager
            .native_hosted_world(Arc::clone(&account), deadline)
            .unwrap();
        {
            let directory = host.directory.as_mut().unwrap();
            let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
            let HostedMembership {
                socket,
                subscription,
                binding,
                session,
            } = peer
                .host(
                    operation.minecraft().unwrap(),
                    &directory.reference,
                    directory.session_subscription,
                    directory.scope.as_ref().unwrap().rta_generation().unwrap(),
                    "456",
                )
                .await;
            directory.socket = Some(socket);
            directory.subscription = Some(subscription);
            directory.subscribed = true;
            directory.binding = Some(binding);
            directory.session = Some(session);
            directory.host_name = identity.gamertag().into();
            directory.approvals = Some(
                Approvals::new(identity.xuid().into(), directory.opening.max_players()).unwrap(),
            );
            directory.observe().unwrap();
            let approvals = directory.approvals.as_mut().unwrap();
            for owner in approvals.missing() {
                approvals.issue(&owner, &[7; NONCE_BYTES]).unwrap();
            }
            let revision = approvals.revision();
            directory.pending = Some(ApprovalWrite {
                revision,
                document: directory.properties().unwrap(),
            });
            peer.set_properties_bound(
                operation.minecraft().unwrap(),
                directory.session.as_mut().unwrap(),
                binding,
                &directory.pending.as_ref().unwrap().document,
            )
            .await;
            directory.observe().unwrap();
            directory.confirm_pending().unwrap();
            operation.persist().unwrap();
        }
        let nonce = host
            .directory
            .as_ref()
            .unwrap()
            .approvals
            .as_ref()
            .unwrap()
            .document()
            .unwrap()["456"]
            .as_str()
            .unwrap()
            .to_owned();
        let approval = host.check_approval("456", &nonce).unwrap();
        let SignedPair {
            mut wire,
            sender,
            mut client,
            mut listener,
            signaling,
        } = signed_pair(deadline).await;
        let prefix = raw_packet(1);
        let forbidden = raw_packet(2);
        host.recheck_approval(&approval).unwrap();
        host.send_member_raw(&approval, &mut wire, prefix.clone(), deadline)
            .await
            .unwrap();
        let prefix_message = client.recv().await.unwrap().unwrap();
        let prefix_matches = decode(prefix_message) == [prefix];

        let mut held = Vec::new();
        fill_capacity(&sender, &mut held).await;
        peer.revoke_member_ready(host.directory.as_mut().unwrap().socket.as_mut().unwrap())
            .await;
        // The real Resync is ready; the original cached grant has not observed it yet.
        host.recheck_approval(&approval).unwrap();
        fill_capacity(&sender, &mut held).await;
        let reserved_capacity = !held.is_empty();
        let (initially_pending, rejected, released_capacity) = {
            let mut sending =
                pin!(host.send_member_raw(&approval, &mut wire, forbidden.clone(), deadline));
            let first = poll_fn(|cx| Poll::Ready(sending.as_mut().poll(cx))).await;
            let initially_pending = first.is_pending();
            let released = held.pop();
            let released_capacity = released.is_some();
            drop(released);
            let outcome = match first {
                Poll::Ready(outcome) => outcome,
                Poll::Pending => sending.await,
            };
            (initially_pending, outcome.is_err(), released_capacity)
        };
        let revoked = host.recheck_approval(&approval).is_err();
        drop(held);

        // Both writes use this exact ordered reliable stream. Observe through the marker
        // before retirement, so close delivery and EOF cannot stand in for packet absence.
        sender
            .send_before(Bytes::from_static(MARKER), deadline)
            .await
            .unwrap();
        let mut dispatched = false;
        loop {
            let message = client.recv().await.unwrap().unwrap();
            if message.as_ref() == MARKER {
                break;
            }
            dispatched |= decode(message).iter().any(|packet| packet == &forbidden);
        }

        let mut retired = wire.begin_close();
        let mut client = client.close();
        retired.finish_close(deadline).await.unwrap();
        let server_drained = matches!(retired.receive_raw(deadline).await,
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof);
        let client_reliable_drained = drain(&mut client.reliable).await;
        let client_unreliable_drained = drain(&mut client.unreliable).await;
        drop(retired);
        drop(client);
        drop(sender);
        listener.close().await.unwrap();
        drop(listener);
        let signaling_joined = match Arc::try_unwrap(signaling) {
            Ok(signaling) => {
                signaling.shutdown().await;
                true
            }
            Err(signaling) => {
                drop(signaling);
                false
            }
        };
        {
            let directory = host.directory.as_mut().unwrap();
            let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
            peer.leave_bound(
                operation.minecraft().unwrap(),
                directory.session.as_mut().unwrap(),
                directory.binding.unwrap(),
            )
            .await;
            operation.persist().unwrap();
            peer.finish(directory.socket.as_mut().unwrap()).await;
        }
        host.close(deadline).await.unwrap();
        assert!(fixture.manager.native_host_cleanup().unwrap().is_none());
        assert!(!fixture.runner.last().stopped());
        assert!(
            prefix_matches,
            "the original current-member packet did not round trip"
        );
        assert!(
            reserved_capacity && released_capacity,
            "no actual send capacity was retained"
        );
        assert!(
            initially_pending,
            "the retained original send completed before capacity release"
        );
        assert!(
            server_drained
                && client_reliable_drained
                && client_unreliable_drained
                && signaling_joined,
            "original transport owners did not finish their explicit cleanup"
        );
        assert!(
            rejected && revoked,
            "actual original membership was not revoked"
        );
        assert!(
            !dispatched,
            "revoked original membership committed an external reliable packet"
        );
    })
    .await
    .expect("finite original membership and actual external send fixture stalled");
}

struct SignedPair {
    wire: NativeAcceptingIo,
    sender: SessionSender,
    client: NetherClient,
    listener: NetherServer,
    signaling: Arc<HttpSignalingServer>,
}

/// Wraps the real association with the existing accepting codec for the original admission controls.
async fn signed_pair(deadline: Instant) -> SignedPair {
    let SignedAssociation {
        accepted,
        key,
        client,
        listener,
        signaling,
    } = signed_association(Arc::new(SigningKey::generate())).await;
    let sender = accepted.session.sender();
    let (wire, _handshake) =
        NativeAcceptingIo::from_nether(accepted, key, ServerHandshakeOptions::default(), deadline)
            .await
            .unwrap();
    SignedPair {
        wire,
        sender,
        client,
        listener,
        signaling,
    }
}

pub(in crate::native_account::hosted_world) struct SignedAssociation {
    pub(in crate::native_account::hosted_world) accepted: nethernet_tokio::AcceptedSession,
    pub(in crate::native_account::hosted_world) key: Arc<SigningKey>,
    pub(in crate::native_account::hosted_world) client: NetherClient,
    pub(in crate::native_account::hosted_world) listener: NetherServer,
    pub(in crate::native_account::hosted_world) signaling: Arc<HttpSignalingServer>,
}

/// Establishes the same signed host-only association before either actual protocol consumer takes it.
async fn signed_association(client_key: Arc<SigningKey>) -> SignedAssociation {
    let now = SystemTime::now();
    let lifetime = ACQUISITION_TIMEOUT;
    let key = Arc::new(SigningKey::generate());
    signed_association_at(client_key, key, now, lifetime).await
}

/// Reuses the same real signed transport while retaining the active host's original synthetic key.
pub(in crate::native_account::hosted_world) async fn signed_association_with_key(
    client_key: Arc<SigningKey>,
    key: Arc<SigningKey>,
) -> SignedAssociation {
    signed_association_at(client_key, key, SystemTime::now(), ACQUISITION_TIMEOUT).await
}

/// Keeps the original concrete transport configuration and clock inputs in one fixture owner.
async fn signed_association_at(
    client_key: Arc<SigningKey>,
    key: Arc<SigningKey>,
    now: SystemTime,
    lifetime: std::time::Duration,
) -> SignedAssociation {
    let client_trust = trust(*client_key.verifying_key(), None);
    let signaling = Arc::new(
        HttpSignalingServer::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            HttpServerConfig {
                network_id: SERVER.into(),
                signaler: nethernet::signaling::http::config::HttpSignalerConfig {
                    token_trust: Some(client_trust),
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
                ServerIdentity::from_key((*key).clone(), SERVER, now, Some(lifetime)).unwrap(),
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
        HttpSignaling::new("synthetic-hosted-send-client".into()).unwrap(),
        format!("http://{}", signaling.local_addr()),
        ConnectionConfig {
            identity: Some(Arc::new(
                SelfSignedIdentity::from_key(client_key, now, lifetime).unwrap(),
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
    SignedAssociation {
        accepted,
        key,
        client,
        listener,
        signaling,
    }
}

struct ExpectedToken {
    key: VerifyingKey,
    issuer: Option<&'static str>,
}

impl TokenVerifier for ExpectedToken {
    fn verify(&self, token: &str, now: SystemTime) -> IdentityResult<Claims> {
        let jws = Jws::parse(token)?;
        if jws.header.alg != "ES384" {
            return Err(IdentityError::Untrusted(
                "unexpected fixture algorithm".into(),
            ));
        }
        jws.verify_es384(&self.key)?;
        let claims = jws.claims()?;
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

/// Trusts only the two original synthetic proof keys; no production trust fallback is used.
fn trust(key: VerifyingKey, issuer: Option<&'static str>) -> TokenTrust {
    TokenTrust::Provider(Arc::new(ExpectedToken { key, issuer }))
}

/// Holds each available public permit until another real reservation is Pending.
async fn fill_capacity(sender: &SessionSender, held: &mut Vec<SendPermit>) {
    loop {
        let mut reservation = pin!(sender.reserve_send(None));
        match poll_fn(|cx| Poll::Ready(reservation.as_mut().poll(cx))).await {
            Poll::Ready(Ok(permit)) => held.push(permit),
            Poll::Ready(Err(error)) => panic!("actual reservation failed: {error}"),
            Poll::Pending => return,
        }
    }
}

/// Uses existing protocol fixture/encoder owners, without exposing generated packet types here.
fn raw_packet(radius: i32) -> Bytes {
    let packet = protocol::test_support::packets::chunk_radius_updated_packet(radius);
    let batch = protocol::encode(&packet).unwrap();
    let DecodeOutcome::Packets { mut packets, .. } =
        BatchCodec::new(BatchHeader::RakNet, BatchLimits::default())
            .decode(batch)
            .unwrap()
    else {
        panic!("a plaintext fixture packet was dropped");
    };
    assert_eq!(packets.len(), 1);
    packets.next().unwrap()
}

/// Decodes the same uncompressed Nether envelope before any application settings are installed.
fn decode(message: Bytes) -> Vec<Bytes> {
    match BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default())
        .decode(message)
        .unwrap()
    {
        DecodeOutcome::Packets { packets, .. } => packets.collect(),
        DecodeOutcome::Dropped(_) => panic!("an original plaintext message was dropped"),
    }
}

/// Drains the original receive FIFO through its explicit terminal and records receive errors.
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

mod output;

pub(in crate::native_account::hosted_world) mod paired;
