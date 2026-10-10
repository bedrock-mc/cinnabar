//! Domain-separated Ed25519 signatures over exact canonical JSON bytes.

use anyhow::{Result, bail, ensure};
use ring::signature::Ed25519KeyPair;
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

pub const OFFER_DOMAIN: &[u8] = b"Cinnabar/experience/offer/v1\0";
pub const ACCEPT_DOMAIN: &[u8] = b"Cinnabar/experience/accept/v1\0";
pub const MANIFEST_DOMAIN: &[u8] = b"Cinnabar/experience/manifest/v1\0";

/// Hex avoids JSON number and Unicode ambiguities around signed bytes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedDocument {
    pub payload: String,
    pub signature: String,
}

impl SignedDocument {
    /// Checks bounds, canonical bytes and the pinned key before returning data.
    pub fn verify<T: DeserializeOwned + Serialize>(
        &self,
        key: &str,
        domain: &[u8],
        limit: usize,
    ) -> Result<(T, String)> {
        ensure!(self.payload.len() <= limit * 2, "signed document too large");
        let payload = unhex(&self.payload)?;
        let key = fixed_hex::<32>(key)?;
        let signature = fixed_hex::<64>(&self.signature)?;
        let mut message = Vec::with_capacity(domain.len() + payload.len());
        message.extend_from_slice(domain);
        message.extend_from_slice(&payload);
        signature::UnparsedPublicKey::new(&signature::ED25519, key)
            .verify(&message, &signature)
            .map_err(|_| anyhow::anyhow!("invalid signature"))?;
        let value: T = serde_json::from_slice(&payload)?;
        ensure!(
            serde_json::to_vec(&value)? == payload,
            "noncanonical document"
        );
        Ok((value, digest(&payload)))
    }
}

/// Signs the canonical serde_json bytes of `value` under `domain`; the exact inverse of `verify`.
pub fn sign<T: Serialize>(
    value: &T,
    domain: &[u8],
    key: &Ed25519KeyPair,
) -> Result<SignedDocument> {
    let payload = serde_json::to_vec(value)?;
    let mut message = Vec::with_capacity(domain.len() + payload.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(&payload);
    Ok(SignedDocument {
        payload: hex(&payload),
        signature: hex(key.sign(&message).as_ref()),
    })
}

/// Computes the immutable content name, independent of URL or destination.
pub fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Encodes lowercase hex, also used for fresh session challenges.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 15)] as char);
    }
    out
}

/// Rejects uppercase, odd lengths and alternate encodings.
pub fn unhex(text: &str) -> Result<Vec<u8>> {
    ensure!(text.len().is_multiple_of(2), "odd hex length");
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok((nibble(pair[0])? << 4) | nibble(pair[1])?))
        .collect()
}

/// Parses fixed-width keys, digests and nonces without truncation.
pub fn fixed_hex<const N: usize>(text: &str) -> Result<[u8; N]> {
    ensure!(text.len() == N * 2, "incorrect hex length");
    unhex(text)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("incorrect hex length"))
}

/// Generates an unpredictable connection challenge from the operating system.
pub fn challenge() -> Result<String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("random source failed"))?;
    Ok(hex(&bytes))
}

/// Converts one canonical hex digit.
fn nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => bail!("invalid hex digit"),
    }
}
