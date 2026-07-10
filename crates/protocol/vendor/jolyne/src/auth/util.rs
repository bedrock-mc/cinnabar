use crate::error::{AuthError, JolyneError};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey};
use serde::de::DeserializeOwned;

pub fn decode_unverified_claims<T: DeserializeOwned>(token: &str) -> Option<T> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let payload = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    serde_json::from_slice(&payload).ok()
}

pub fn key_from_base64(b64: &str) -> Result<DecodingKey, JolyneError> {
    let der = STANDARD
        .decode(b64)
        .map_err(|e| AuthError::BadSignature(format!("Invalid base64 key: {e}")))?;
    Ok(DecodingKey::from_ec_der(&der))
}

pub fn key_from_base64_for_alg(b64: &str, alg: Algorithm) -> Result<DecodingKey, JolyneError> {
    let der = STANDARD
        .decode(b64)
        .map_err(|e| AuthError::BadSignature(format!("Invalid base64 key: {e}")))?;
    match alg {
        Algorithm::ES256 | Algorithm::ES384 => Ok(DecodingKey::from_ec_der(&der)),
        Algorithm::RS256 => Ok(DecodingKey::from_rsa_der(&der)),
        _ => Err(AuthError::UnsupportedAlg(format!("{alg:?}")).into()),
    }
}
