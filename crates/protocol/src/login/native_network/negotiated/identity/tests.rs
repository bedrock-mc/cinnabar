use super::*;
use crate::login::native_network::transport;
use bytes::Bytes;
use nethernet::identity::{
    TokenTrust, TokenVerifier,
    error::IdentityError,
    jwt::{Claims, Jws},
};
use nethernet_tokio::{
    ConnectionConfig, NetherServer, ServerIceConfig,
    signaling::http::{HttpServerConfig, HttpSignalingServer},
};

struct OriginalProof(p384::ecdsa::VerifyingKey);

impl TokenVerifier for OriginalProof {
    /// Checks the real self-signed offer against the same original key that later owns Login.
    fn verify(&self, token: &str, now: SystemTime) -> nethernet::identity::error::Result<Claims> {
        let jws = Jws::parse(token)?;
        let claims = jws.claims()?;
        claims.check_expiry(now)?;
        let key = claims.client_public_key()?;
        if key != self.0 {
            return Err(IdentityError::Untrusted("wrong original proof".into()));
        }
        jws.verify_es384(&key)?;
        Ok(claims)
    }
}

#[tokio::test]
async fn local_http_ready_connection_preserves_original_proof_and_bidirectional_input() {
    tokio::time::timeout(crate::NATIVE_JOIN_TIMEOUT, async {
        let credentials = Credentials::new(None).unwrap();
        let proof = TokenTrust::Provider(Arc::new(OriginalProof(*credentials.key.verifying_key())));
        let signaling = Arc::new(
            HttpSignalingServer::bind(
                "127.0.0.1:0".parse().unwrap(),
                HttpServerConfig {
                    signaler: nethernet::prelude::HttpSignalerConfig {
                        token_trust: Some(proof.clone()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await
            .unwrap(),
        );
        let address = signaling.local_addr();
        let mut listener = NetherServer::bind_with(
            Arc::clone(&signaling),
            ConnectionConfig {
                identity: Some(Arc::new(
                    nethernet::identity::ServerIdentity::generate("self", SystemTime::now())
                        .unwrap(),
                )),
                token_trust: Some(proof),
                server_ice: Some(ServerIceConfig {
                    turn_streams: Vec::new(),
                    turn_tls: None,
                    ice: Default::default(),
                    bind_addr: "127.0.0.1:0".parse().unwrap(),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut client = transport::http::connect(
            address,
            credentials.transport_identity().unwrap(),
            tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
        )
        .await
        .unwrap();
        let mut accepted = listener.accept().await.unwrap();
        assert_eq!(
            client.nonce(),
            "",
            "HTTP must not invent a LAN/friend nonce"
        );
        let expired = client
            .send_until(Bytes::from_static(b"expired Login"), Some(Instant::now()))
            .await
            .unwrap_err();
        assert!(authentication::is_expired(&expired));
        client
            .send_until(Bytes::from_static(b"original local player"), None)
            .await
            .unwrap();
        assert_eq!(
            accepted.reliable.recv().await.unwrap().unwrap(),
            b"original local player"[..]
        );
        accepted
            .session
            .send(Bytes::from_static(b"original world reply"))
            .await
            .unwrap();
        assert_eq!(client.recv().await.unwrap(), b"original world reply"[..]);
        client.close().await.unwrap();
        drop(accepted.close());
        listener.close().await.unwrap();
        drop(listener);
        Arc::try_unwrap(signaling)
            .ok()
            .expect("HTTP fixture ownership leaked")
            .shutdown()
            .await;
    })
    .await
    .expect("finite actual local HTTP fixture stalled");
}

#[test]
fn local_http_requires_numeric_loopback_and_a_signed_answer() {
    for address in [
        "example.invalid:19132",
        "192.0.2.1:19132",
        "http://127.0.0.1:19132",
        "127.0.0.1:0",
    ] {
        assert!(
            crate::NativeConnectionTarget::new(
                address.into(),
                crate::NativeTransport::LocalNetherNetHttp
            )
            .local_http_address()
            .is_err()
        );
    }
    for address in ["127.0.0.1:19132", "[::1]:19132"] {
        let selected = crate::NativeConnectionTarget::new(
            address.into(),
            crate::NativeTransport::LocalNetherNetHttp,
        );
        assert_eq!(selected.local_http_address().unwrap().to_string(), address);
    }
    let credentials = Credentials::new(None).unwrap();
    let policy = credentials.transport_identity().unwrap();
    let sdp = "v=0\r\na=fingerprint:sha-256 AB:CD\r\nm=application 9\r\n";
    let signed = policy.identity.augment(sdp, SystemTime::now()).unwrap();
    nethernet::identity::validate_sdp(
        &signed,
        &TokenTrust::Provider(Arc::new(OriginalProof(*credentials.key.verifying_key()))),
        SystemTime::now(),
    )
    .unwrap();
    assert!(
        nethernet::identity::verify_answer(sdp, Some(&policy.trust), SystemTime::now()).is_err(),
        "an identityless answer must remain rejected"
    );
}

#[tokio::test]
async fn local_http_expired_credential_rejects_before_any_association() {
    let credentials = Credentials::new(None).unwrap();
    let mut identity = credentials.transport_identity().unwrap();
    identity.expires = Some(Instant::now());
    let result = transport::http::connect(
        "127.0.0.1:1".parse().unwrap(),
        identity,
        tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
    )
    .await;
    assert!(authentication::is_expired(
        &result.err().expect("expired credential admitted")
    ));
}
