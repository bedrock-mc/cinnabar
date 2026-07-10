use crate::auth::types::{ChainClaims, ValidatedIdentity};
use crate::auth::util::key_from_base64;
use crate::error::{AuthError, JolyneError};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde_json::Value;
use tracing::debug;

const MAX_IDENTITY_PAYLOAD_BYTES: usize = 64 * 1024; // defensive bound against large/garbage payloads
const MAX_CHAIN_TOKENS: usize = 8;
const MAX_TOKEN_BYTES: usize = 8 * 1024;

// Standard Mojang public key for Bedrock Edition
// This key is used to verify the first JWT in the chain (the one signed by Mojang).
// It validates the identity of the XBL token.
pub const MOJANG_PUBLIC_KEY_BASE64: &str = "MHYwEAYHKoZIzj0CAQYFK4EEACIDYgAECRXueJeTDqNRRgJi/vlRufByu/2G0i2Ebt6YMar5QX/R0DIIyrJMcUpruK4QveTfJSTp3Shlq4Gk34cD/4GUWwkv0DVuzeuB+tXija7HBxii03NHDbPAD0AKnLr2wdAp";

pub(crate) fn chain_from_value(v: &Value) -> Option<Vec<String>> {
    // 0. Value IS the chain array.
    if let Some(arr) = v.as_array() {
        let mut out = Vec::with_capacity(arr.len());
        let mut all_strings = true;
        for item in arr {
            if let Some(s) = item.as_str() {
                out.push(s.to_string());
            } else {
                all_strings = false;
                break;
            }
        }
        if all_strings {
            return Some(out);
        }
    }

    // 0.5 Value is a JSON string (recurse).
    if let Some(s) = v.as_str()
        && let Ok(nested_val) = serde_json::from_str::<Value>(s)
        // Avoid infinite recursion if the string parses back to a string that is identical.
        // (serde_json::from_str of "\"foo\"" is Value::String("foo")).
        // But usually we expect an Object or Array inside.
        && !nested_val.is_string()
        && let Some(chain) = chain_from_value(&nested_val)
    {
        return Some(chain);
    }

    // Direct chain array.
    if let Some(arr) = v.get("chain").and_then(|c| c.as_array()) {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            if let Some(s) = item.as_str() {
                out.push(s.to_string());
            } else {
                return None;
            }
        }
        return Some(out);
    }

    // Certificate object/string with chain (handle both lower/upper keys).
    for key in ["certificate", "Certificate"] {
        if let Some(cert_obj) = v.get(key) {
            // certificate as object
            if let Some(arr) = cert_obj.get("chain").and_then(|c| c.as_array()) {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    if let Some(s) = item.as_str() {
                        out.push(s.to_string());
                    } else {
                        return None;
                    }
                }
                return Some(out);
            }
            // certificate as escaped JSON string
            if let Some(s) = cert_obj.as_str()
                && let Ok(cert_val) = serde_json::from_str::<Value>(s)
                && let Some(chain) = chain_from_value(&cert_val)
            {
                return Some(chain);
            }
        }
    }
    None
}

fn extract_chain_from_json(bytes: &[u8]) -> Result<Vec<String>, AuthError> {
    let val: Value = serde_json::from_slice(bytes).map_err(|_| AuthError::InvalidJson)?;
    if let Some(chain) = chain_from_value(&val) {
        if chain.len() > MAX_CHAIN_TOKENS {
            return Err(AuthError::ChainTooLong(MAX_CHAIN_TOKENS));
        }
        return Ok(chain);
    }
    Err(AuthError::MissingChain)
}

pub fn parse_login_chain(identity_payload: &[u8]) -> Result<Vec<String>, JolyneError> {
    if identity_payload.len() > MAX_IDENTITY_PAYLOAD_BYTES {
        return Err(AuthError::PayloadTooLarge(MAX_IDENTITY_PAYLOAD_BYTES).into());
    }
    let mut first_error: Option<AuthError> = None;
    // Fast path: payload is plain JSON with chain field.
    if let Ok(s) = std::str::from_utf8(identity_payload) {
        match extract_chain_from_json(s.as_bytes()) {
            Ok(chain) => return Ok(chain),
            Err(e) => {
                if !matches!(e, AuthError::MissingChain) && first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
    }

    // Fallback: Bedrock/Go-style little-endian length-prefixed blob: [u32_le len][json][u32_le raw_token_len][raw_token]
    if identity_payload.len() >= 4 {
        let len = u32::from_le_bytes(identity_payload[0..4].try_into().unwrap()) as usize;
        if identity_payload.len() >= 4 + len {
            let json_slice = &identity_payload[4..4 + len];
            match extract_chain_from_json(json_slice) {
                Ok(chain) => return Ok(chain),
                Err(e) => {
                    if !matches!(e, AuthError::MissingChain) && first_error.is_none() {
                        first_error = Some(e);
                    }
                }
            }
        }
    }

    // Last-resort heuristic: find the first '{' and try shrinking from the end until JSON parses.
    if let Some(start) = identity_payload.iter().position(|b| *b == b'{') {
        let mut end = identity_payload.len();
        while end > start {
            match extract_chain_from_json(&identity_payload[start..end]) {
                Ok(chain) => return Ok(chain),
                Err(_) => { /* keep searching */ }
            }
            // Back off a bit; raw token bytes come after the JSON, so trim from the end.
            end = end.saturating_sub(1);
        }
    }

    let preview_len = identity_payload.len().min(64);
    let preview = &identity_payload[..preview_len];
    match std::str::from_utf8(preview) {
        Ok(as_str) => debug!(
            payload_len = identity_payload.len(),
            preview_str = as_str,
            preview_hex = ?preview,
            "parse_login_chain failed to locate chain"
        ),
        Err(_) => debug!(
            payload_len = identity_payload.len(),
            preview_hex = ?preview,
            "parse_login_chain failed to locate chain"
        ),
    }

    if let Some(err) = first_error {
        return Err(err.into());
    }

    if std::str::from_utf8(identity_payload).is_err() {
        Err(AuthError::InvalidUtf8.into())
    } else {
        Err(AuthError::MissingChain.into())
    }
}

fn decode_chain_token(
    token: &str,
    key: &DecodingKey,
) -> Result<jsonwebtoken::TokenData<ChainClaims>, JolyneError> {
    let header = decode_header(token).map_err(|e| AuthError::InvalidHeader(e.to_string()))?;
    if header.alg != Algorithm::ES384 {
        return Err(AuthError::UnsupportedAlg(format!("{:?}", header.alg)).into());
    }

    let mut validation = Validation::new(Algorithm::ES384);
    // Accept default exp/nbf checking; audience/issuer are currently not enforced until we define expected values
    validation.validate_exp = true;
    validation.validate_nbf = true;

    decode::<ChainClaims>(token, key, &validation).map_err(|e| {
        let is_time = matches!(
            e.kind(),
            jsonwebtoken::errors::ErrorKind::ExpiredSignature
                | jsonwebtoken::errors::ErrorKind::ImmatureSignature
        );
        if is_time {
            AuthError::TemporalValidation
        } else {
            AuthError::BadSignature(e.to_string())
        }
        .into()
    })
}

fn verify_chain_with_key(
    chain: &[String],
    mut current_key: DecodingKey,
) -> Result<ValidatedIdentity, JolyneError> {
    let mut identity: Option<ValidatedIdentity> = None;

    for (idx, token_str) in chain.iter().enumerate() {
        if token_str.len() > MAX_TOKEN_BYTES {
            return Err(AuthError::TokenTooLarge(MAX_TOKEN_BYTES).into());
        }
        let token = decode_chain_token(token_str, &current_key)?;
        let header_key_b64 = token
            .header
            .x5u
            .clone()
            .or_else(|| token.header.x5c.as_ref().and_then(|v| v.first().cloned()));
        let claims = token.claims;

        let pub_key_b64 = claims
            .identity_public_key
            .clone()
            .or(header_key_b64.clone())
            .ok_or(AuthError::MissingIdentityKey)?;

        let is_last = idx + 1 == chain.len();
        if let Some(extra) = claims.extra_data.as_ref() {
            // Some clients may include extraData earlier; only required on last.
            identity = Some(ValidatedIdentity {
                xuid: extra.xuid.clone(),
                display_name: extra.display_name.clone(),
                uuid: extra.uuid.clone(),
                identity_public_key: pub_key_b64.clone(),
            });
        } else if is_last {
            // Only enforce extraData on the final token (identity) to match vanilla flow.
            return Err(AuthError::MissingExtraData.into());
        }

        current_key = key_from_base64(&pub_key_b64)?;
    }

    identity.ok_or_else(|| AuthError::MissingExtraData.into())
}

pub fn validate_chain(
    chain: Vec<String>,
    online_mode: bool,
) -> Result<ValidatedIdentity, JolyneError> {
    if chain.is_empty() {
        return Err(AuthError::EmptyChain.into());
    }
    if chain.len() > MAX_CHAIN_TOKENS {
        return Err(AuthError::ChainTooLong(MAX_CHAIN_TOKENS).into());
    }

    // The chain is expected root->leaf. Verify signatures when online_mode is true.
    if online_mode {
        let root_key: DecodingKey = key_from_base64(MOJANG_PUBLIC_KEY_BASE64)
            .expect("Shouldn't occur mojang pub key invalid, hard coded const.");
        debug!(chain_len = chain.len(), "validate_chain starting");

        let mut reversed = chain.clone();
        reversed.reverse();

        // Try provided order with each Mojang root key.
        if let Ok(id) = verify_chain_with_key(&chain, root_key.clone()) {
            debug!("validated login chain using mojang root");
            return Ok(id);
        }
        // Try reversed with each root key.
        if reversed != chain
            && let Ok(id) = verify_chain_with_key(&reversed, root_key)
        {
            debug!("validated login chain after reversing order (mojang root)");
            return Ok(id);
        }

        debug!(
            chain_len = chain.len(),
            "validate_chain failed all signature attempts"
        );
        Err(AuthError::BadSignature("InvalidSignature".to_string()).into())
    } else {
        // Offline mode: parse without signature validation but still require structured data.
        let last = chain.last().expect("non-empty checked");
        let header_key_b64 = decode_header(last)
            .ok()
            .and_then(|h| h.x5u.or_else(|| h.x5c.and_then(|v| v.first().cloned())));
        let parts: Vec<&str> = last.split('.').collect();
        if parts.len() < 2 {
            return Err(AuthError::BadSignature("Invalid JWT format".to_string()).into());
        }

        let payload_bytes = URL_SAFE_NO_PAD
            .decode(parts[1])
            .or_else(|_| STANDARD.decode(parts[1]))
            .map_err(|e| AuthError::BadSignature(format!("Invalid offline JWT b64: {e}")))?;

        let claims: ChainClaims =
            serde_json::from_slice(&payload_bytes).map_err(|_| AuthError::InvalidJson)?;
        let pub_key_b64 = claims
            .identity_public_key
            .or(header_key_b64)
            .ok_or(AuthError::MissingIdentityKey)?;
        if let Some(extra) = claims.extra_data {
            Ok(ValidatedIdentity {
                xuid: extra.xuid,
                display_name: extra.display_name,
                uuid: extra.uuid,
                identity_public_key: pub_key_b64,
            })
        } else {
            Err(AuthError::MissingExtraData.into())
        }
    }
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;

    #[test]
    fn chain_from_value_handles_direct_array() {
        let json = serde_json::json!(["jwt1", "jwt2"]);
        let chain = chain_from_value(&json).expect("should extract chain from array");
        assert_eq!(chain, vec!["jwt1", "jwt2"]);
    }

    #[test]
    fn chain_from_value_handles_json_string() {
        // Simulates `certificate: "{\"chain\":[\"jwt1\"]}"`
        let inner_json = r#"{"chain":["jwt1"]}"#;
        let val = Value::String(inner_json.to_string());
        let chain = chain_from_value(&val).expect("should recurse into string");
        assert_eq!(chain, vec!["jwt1"]);
    }

    #[test]
    fn validate_chain_handles_padded_offline_jwt() {
        // Construct a dummy payload.
        let payload = serde_json::json!({
            "identityPublicKey": "key",
            "extraData": {
                "XUID": "123",
                "displayName": "User",
                "identity": "uuid"
            }
        });
        // Encode with standard padding.
        let payload_b64 = STANDARD.encode(serde_json::to_vec(&payload).unwrap());
        let jwt = format!("header.{}.sig", payload_b64);

        let identity = validate_chain(vec![jwt], false).expect("should accept padded offline jwt");
        assert_eq!(identity.display_name.as_deref(), Some("User"));
    }
}
