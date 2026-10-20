//! Fresh synthetic issuer; the production verifier still checks RS256, identity and the ES384 proof.

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use bedrock_services::test_support::HOSTED_WORLD_ISSUER;
use p384::{ecdsa::SigningKey, pkcs8::EncodePublicKey};
use rsa::{
    RsaPrivateKey, pkcs1v15,
    sha2::Sha256,
    signature::{SignatureEncoding, Signer},
    traits::PublicKeyParts,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(in crate::native_account::hosted_world) const VISITOR_XUID: &str = "456";
pub(in crate::native_account::hosted_world) const VISITOR_NAME: &str = "Visitor";
const KEY_ID: &str = "paired-world-issuer";

pub(in crate::native_account::hosted_world) struct Provider {
    signing: pkcs1v15::SigningKey<Sha256>,
    pub(in crate::native_account::hosted_world) jwks: Vec<u8>,
}

impl Provider {
    /// Keeps the fixture issuer independent of both connection signing keys.
    pub(in crate::native_account::hosted_world) fn new() -> Self {
        let signing =
            pkcs1v15::SigningKey::new(RsaPrivateKey::new(&mut rand::rng(), 1024).unwrap());
        let public = signing.as_ref();
        let jwks = serde_json::to_vec(&json!({"keys":[{
            "kty":"RSA", "kid":KEY_ID, "alg":"RS256", "use":"sig",
            "n":URL_SAFE_NO_PAD.encode(public.n().to_be_bytes()),
            "e":URL_SAFE_NO_PAD.encode(public.e().to_be_bytes())
        }]}))
        .unwrap();
        Self { signing, jwks }
    }

    /// Qualifies the synthetic response through the normal NativeAuthentication verifier and clock owner.
    pub(in crate::native_account::hosted_world) fn authentication(
        &self,
        key: SigningKey,
        service_time: SystemTime,
        observed: Instant,
        claims: Value,
    ) -> protocol::NativeAuthentication {
        self.authentication_for(
            key,
            service_time,
            observed,
            claims,
            (VISITOR_XUID, VISITOR_NAME),
        )
    }

    /// Verifies each actual visitor's distinct identity against its own connection proof.
    pub(in crate::native_account::hosted_world) fn authentication_for(
        &self,
        key: SigningKey,
        service_time: SystemTime,
        observed: Instant,
        claims: Value,
        visitor: (&str, &str),
    ) -> protocol::NativeAuthentication {
        let (xuid, name) = visitor;
        let now = Instant::now();
        let seconds = service_time.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let expires = service_time + Duration::from_secs(300);
        let expires_seconds = expires.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let claims = serde_json::value::to_raw_value(&claims).unwrap();
        let payload = json!({"iss":HOSTED_WORLD_ISSUER,
            "aud":nethernet::identity::MINECRAFT_AUDIENCE, "sub":"paired-world-visitor",
            "xid":xuid, "xname":name, "iat":seconds, "exp":expires_seconds,
            "cpk":STANDARD.encode(key.verifying_key().to_public_key_der().unwrap().as_bytes())});
        let header = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&json!({"alg":"RS256","kid":KEY_ID})).unwrap());
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let input = format!("{header}.{payload}");
        let signature: pkcs1v15::Signature = self.signing.sign(input.as_bytes());
        let token = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_vec()));
        let credential =
            protocol::NativeCredential::new(token, service_time, expires, observed).unwrap();
        let verifier = protocol::NativeTokenVerifier::new(
            HOSTED_WORLD_ISSUER.into(),
            &["RS256".into()],
            &self.jwks,
        )
        .unwrap();
        protocol::NativeAuthentication::new(key, credential, &claims, &verifier, xuid, now).unwrap()
    }
}
