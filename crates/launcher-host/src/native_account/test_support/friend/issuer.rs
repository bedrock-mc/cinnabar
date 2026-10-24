//! Independent controlled host signing key and the original SDK issuer's public metadata.

use super::*;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use bedrock_services::test_support::HOSTED_ACTIVATION_KEY_ID;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate, pkcs8::EncodePublicKey};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1::DecodeRsaPublicKey,
    pkcs1v15,
    sha2::Sha256,
    signature::{SignatureEncoding, Signer},
    traits::PublicKeyParts,
};
use serde_json::json;

const KEY_ID: &str = "controlled-friend-host";

pub(super) struct Issuer {
    signer: pkcs1v15::SigningKey<Sha256>,
    pub(super) jwks: Vec<u8>,
}

impl Issuer {
    /// Combines public keys only; the SDK's original client mint signer remains private to its peer.
    pub(super) fn new(peer: &HostedPeer) -> Self {
        let signer = pkcs1v15::SigningKey::new(RsaPrivateKey::new(&mut rand::rng(), 1024).unwrap());
        let sdk = RsaPublicKey::from_pkcs1_der(peer.issuer_public_key_der()).unwrap();
        let key = |public: &RsaPublicKey, kid: &str| {
            json!({
                "kty":"RSA", "kid":kid, "alg":"RS256", "use":"sig",
                "n":URL_SAFE_NO_PAD.encode(public.n().to_be_bytes()),
                "e":URL_SAFE_NO_PAD.encode(public.e().to_be_bytes()),
            })
        };
        let host = RsaPublicKey::from(signer.as_ref());
        let jwks = serde_json::to_vec(
            &json!({"keys":[key(&sdk, HOSTED_ACTIVATION_KEY_ID), key(&host, KEY_ID)]}),
        )
        .unwrap();
        Self { signer, jwks }
    }

    /// Uses the original strict issuer verifier for both peer directions.
    pub(super) fn verifier(&self) -> io::Result<bedrock_net::login::provider::ProviderVerifier> {
        bedrock_net::login::provider::ProviderVerifier::new(
            HOSTED_WORLD_ISSUER.into(),
            &["RS256".into()],
            &self.jwks,
        )
    }

    /// Signs only this controlled host's actual connection key and decoded remote identity.
    pub(super) fn host_identity(
        &self,
        xuid: &str,
        time: std::time::SystemTime,
        observed: Instant,
    ) -> io::Result<protocol::NativeHostingIdentity> {
        let key = Arc::new(SigningKey::generate());
        let expires = time + Duration::from_secs(300);
        let seconds =
            |value: std::time::SystemTime| value.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let header = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&json!({"alg":"RS256","kid":KEY_ID})).unwrap());
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "iss":HOSTED_WORLD_ISSUER, "aud":nethernet::identity::MINECRAFT_AUDIENCE,
                "sub":"controlled-friend-host", "xid":xuid, "xname":"Controlled host",
                "iat":seconds(time), "exp":seconds(expires),
                "cpk":STANDARD.encode(key.verifying_key().to_public_key_der().unwrap().as_bytes()),
            }))
            .unwrap(),
        );
        let input = format!("{header}.{payload}");
        let signature: pkcs1v15::Signature = self.signer.sign(input.as_bytes());
        let token = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_vec()));
        protocol::NativeHostingIdentity::new(
            key,
            protocol::NativeCredential::new(token, time, expires, observed)?,
            &self.verifier()?,
            xuid,
            Instant::now(),
        )
    }
}
