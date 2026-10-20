//! Public advertised activation over the original signed account, real service sockets and world process.

use super::*;
use crate::{local_worlds::test_support::paired::World, native_account::ACQUISITION_TIMEOUT};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bedrock_services::test_support::{
    HOSTED_ACTIVATION_KEY_ID, HOSTED_WORLD_ISSUER, HostedSocketTerminal,
};
use p384::{ecdsa::VerifyingKey, pkcs8::DecodePublicKey};
use rsa::{RsaPublicKey, pkcs1::DecodeRsaPublicKey, traits::PublicKeyParts};
use std::num::NonZeroU32;

const INVITEE: &str = "456";

/// Actual public activation publishes its real bound route before activity and original-owner invitation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_activation_publishes_original_route_activity_and_invitation_then_retires() {
    let Some(world) = World::open() else {
        return;
    };
    tokio::time::timeout(ACQUISITION_TIMEOUT * 2, async {
        let namespace = tempfile::tempdir().unwrap();
        let visitor_cap = u32::try_from(crate::native_account::hosted_world::active::VISITOR_LIMIT)
            .unwrap()
            .checked_add(1)
            .unwrap();
        let cap = NonZeroU32::new(
            world.manager.status().max_players.unwrap().min(visitor_cap),
        )
        .unwrap();
        let config = minecraft::Config::desktop(
            protocol::GAME_VERSION,
            u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            metadata::service_device_id("123").unwrap(),
        );
        let HostedAuthority {
            account: services,
            identity,
            peer,
        } = HostedAuthority::acquire_activation(
            authentication_policy(),
            config,
            cap,
            bridge::contract::PRESENCE_TITLE_ID.to_string(),
            nethernet::identity::MINECRAFT_AUDIENCE.into(),
        )
        .await;
        let jwks = issuer_jwks(&peer);
        let account = install_activation_account(
            namespace.path(),
            services,
            &identity,
            &serde_json::to_vec(&jwks).unwrap(),
        )
        .await;
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let original = world
            .manager
            .native_hosted_world(Arc::clone(&account), deadline)
            .unwrap();
        let mut active = original
            .activate(deadline)
            .await
            .expect("public native host activation");
        let invitation = active.invitation();
        let invited = {
            let running = active.run();
            tokio::pin!(running);
            tokio::select! {
                result = &mut running => result.and(Err(io::Error::other("active host ended before invitation"))),
                result = invitation.send(INVITEE, deadline) => result,
            }
        };
        drop(invitation);
        let mut closing = active.begin_close(deadline);
        let closed = closing.finish_close().await;
        drop(closing);
        let observed = peer.finish_activation().await;
        let cleanup_empty = world.manager.native_host_cleanup().unwrap().is_none();
        let status = world.manager.status();
        invited.expect("original active-owner invitation");
        closed.expect("original listener/process/membership cleanup");
        assert!(
            cleanup_empty && status.state == bridge::WorldState::Running,
            "original publication cleanup failed or stopped the live world"
        );
        assert_eq!(observed.max_members, cap.get());
        assert_eq!(observed.created_properties["MaxMemberCount"], cap.get());
        let routes = observed.created_properties["SupportedConnections"]
            .as_array()
            .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(
            routes[0]["ConnectionType"],
            crate::native_account::world_metadata::JSON_RPC_SIGNALING
        );
        assert!(routes[0]["NetherNetId"].as_u64().is_some_and(|id| id != 0));
        assert!(
            Uuid::parse_str(routes[0]["PmsgId"].as_str().unwrap())
                .is_ok_and(|id| !id.is_nil())
        );
        VerifyingKey::from_public_key_der(&observed.minted_public_key)
            .expect("actual generated host key in original mint request");
        assert_eq!(observed.invited_xuid, INVITEE);
        assert_eq!(
            observed.rta,
            HostedSocketTerminal::Io(io::ErrorKind::UnexpectedEof)
        );
        assert!(
            matches!(
                observed.messaging,
                HostedSocketTerminal::Close | HostedSocketTerminal::Eof
            ),
            "actual messaging socket terminal: {:?}",
            observed.messaging
        );
    })
    .await
    .expect("finite public host activation fixture stalled");
}

/// Derives the original synthetic issuer's public JWKS without exposing its private signer.
pub(in crate::native_account::hosted_world) fn issuer_jwks(
    peer: &bedrock_services::test_support::HostedPeer,
) -> serde_json::Value {
    let public = RsaPublicKey::from_pkcs1_der(peer.issuer_public_key_der()).unwrap();
    json!({"keys":[{
        "kty":"RSA", "kid":HOSTED_ACTIVATION_KEY_ID, "alg":"RS256", "use":"sig",
        "n":URL_SAFE_NO_PAD.encode(public.n().to_be_bytes()),
        "e":URL_SAFE_NO_PAD.encode(public.e().to_be_bytes()),
    }]})
}

/// Reuses the original signed account install and provider cache for both actual activation controls.
pub(in crate::native_account::hosted_world) async fn install_activation_account(
    directory: &std::path::Path,
    services: minecraft::Minecraft,
    identity: &bedrock_services::xbox::Authorization,
    jwks: &[u8],
) -> Arc<NativeAccount> {
    let store = AccountStore::new(directory.join("synthetic-oauth.json"));
    let checkpoint = services.checkpoint().unwrap();
    let credentials = services.credentials();
    let expiry = oauth_expiry(credentials.expires_at()).unwrap();
    let pending = transaction("reserve public host fixture sign-in", || {
        store.try_begin_native_sign_in("public-host-generation", None)
    });
    transaction("publish public host fixture OAuth", || {
        pending.try_publish(
            credentials.access_token(),
            credentials.refresh_token(),
            &expiry,
        )
    });
    transaction("publish public host fixture checkpoint", || {
        pending.try_publish_checkpoint(checkpoint.as_bytes())
    });
    transaction("commit public host fixture sign-in", || {
        pending.try_commit(
            &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
        )
    });
    let account = Arc::new(NativeAccount::new(
        store,
        directory.join("synthetic-device.json"),
        Some(identity.xuid().into()),
    ));
    install(&account, services, identity);
    {
        let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
        operation.cached.as_mut().unwrap().provider = Some(
            protocol::test_support::hosted_provider(
                HOSTED_WORLD_ISSUER,
                minecraft::USER_AGENT,
                jwks,
                Instant::now(),
            )
            .unwrap(),
        );
        operation.persist().unwrap();
    }
    account
}

/// Reads the actual acknowledged nonce without manufacturing an approval or changing the directory.
pub(in crate::native_account::hosted_world) fn activation_nonce(
    host: &NativeHostedWorld,
    xuid: &str,
) -> String {
    let nonce = host
        .directory
        .as_ref()
        .unwrap()
        .approvals
        .as_ref()
        .unwrap()
        .document()
        .unwrap()[xuid]
        .as_str()
        .unwrap()
        .to_owned();
    host.check_approval(xuid, &nonce).unwrap();
    nonce
}

mod cutoff;
