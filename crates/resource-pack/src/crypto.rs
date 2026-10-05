//! Marketplace-style content encryption: AES-CFB8 whose IV is the key's first
//! 16 bytes, a `contents.json` index behind a 256-byte header, and per-file keys.

use std::collections::HashMap;

use aes::cipher::{BlockEncrypt, KeyInit};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::{AdmissionError, MAX_ENTRIES_PER_PACK, MAX_PATH_BYTES, normalize_jsonc};

const CONTENTS_HEADER_BYTES: usize = 256;

/// A content key retained only in memory and zeroed on drop.
pub(crate) struct ContentKey(Box<[u8]>);

impl ContentKey {
    pub(crate) fn identity(&self) -> [u8; 32] {
        Sha256::digest(&self.0).into()
    }

    /// Accepts only AES-128/192/256 key lengths after trimming ASCII whitespace.
    pub(crate) fn new(bytes: &[u8]) -> Option<Self> {
        let trimmed = bytes.trim_ascii();
        matches!(trimmed.len(), 16 | 24 | 32).then(|| Self(trimmed.into()))
    }

    /// Decrypts `data` in place.
    pub(crate) fn decrypt(&self, data: &mut [u8]) {
        match self.0.len() {
            16 => cfb8_decrypt(
                &aes::Aes128::new_from_slice(&self.0).expect("len"),
                &self.0,
                data,
            ),
            24 => cfb8_decrypt(
                &aes::Aes192::new_from_slice(&self.0).expect("len"),
                &self.0,
                data,
            ),
            _ => cfb8_decrypt(
                &aes::Aes256::new_from_slice(&self.0).expect("len"),
                &self.0,
                data,
            ),
        }
    }
}

impl Drop for ContentKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn cfb8_decrypt<C: BlockEncrypt<BlockSize = aes::cipher::consts::U16>>(
    cipher: &C,
    key: &[u8],
    data: &mut [u8],
) {
    let mut register = [0u8; 16];
    register.copy_from_slice(&key[..16]);
    let mut block = aes::Block::default();
    for byte in data {
        block.copy_from_slice(&register);
        cipher.encrypt_block(&mut block);
        let ciphertext = *byte;
        *byte ^= block[0];
        register.copy_within(1.., 0);
        register[15] = ciphertext;
    }
}

#[derive(Deserialize)]
struct Contents {
    #[serde(default)]
    content: Vec<ContentsEntry>,
}

#[derive(Deserialize)]
struct ContentsEntry {
    #[serde(default)]
    path: String,
    #[serde(default)]
    key: Option<String>,
}

/// Returns per-file keys from a `contents.json` that is either plaintext or
/// encrypted with the pack key after its fixed header. Entries with a malformed
/// path or key are ignored; the file is then read as stored.
pub(crate) fn contents_file_keys(
    raw: &[u8],
    pack_key: &ContentKey,
) -> Result<HashMap<Box<str>, ContentKey>, AdmissionError> {
    // The decrypted index and its per-file keys are secrets: wipe them on drop.
    let json = match normalize_jsonc(raw) {
        Some(json) => Zeroizing::new(json),
        None => {
            let mut body = Zeroizing::new(
                raw.get(CONTENTS_HEADER_BYTES..)
                    .filter(|body| !body.is_empty())
                    .ok_or(AdmissionError::MalformedContentsIndex)?
                    .to_vec(),
            );
            pack_key.decrypt(&mut body);
            Zeroizing::new(normalize_jsonc(&body).ok_or(AdmissionError::MalformedContentsIndex)?)
        }
    };
    let mut contents: Contents =
        serde_json::from_slice(&json).map_err(|_| AdmissionError::MalformedContentsIndex)?;
    if contents.content.len() > MAX_ENTRIES_PER_PACK {
        return Err(AdmissionError::TooManyEntries);
    }
    let mut keys = HashMap::with_capacity(contents.content.len());
    for entry in &mut contents.content {
        let key = entry
            .key
            .as_deref()
            .and_then(|key| ContentKey::new(key.as_bytes()));
        entry.key.zeroize();
        let path = entry.path.replace('\\', "/");
        let path = path.trim_start_matches("./");
        let Some(key) = key else {
            continue;
        };
        if !path.is_empty() && path.len() <= MAX_PATH_BYTES {
            keys.insert(path.into(), key);
        }
    }
    Ok(keys)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// CFB8 encryption, the inverse of `ContentKey::decrypt`.
    pub(crate) fn encrypt(key: &[u8], data: &mut [u8]) {
        let cipher = aes::Aes256::new_from_slice(key).expect("test key is 32 bytes");
        let mut register = [0u8; 16];
        register.copy_from_slice(&key[..16]);
        let mut block = aes::Block::default();
        for byte in data {
            block.copy_from_slice(&register);
            cipher.encrypt_block(&mut block);
            *byte ^= block[0];
            register.copy_within(1.., 0);
            register[15] = *byte;
        }
    }

    const KEY: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";

    #[test]
    fn cfb8_round_trips_with_key_prefix_iv() {
        let plain = b"{\"format_version\": 2}".to_vec();
        let mut data = plain.clone();
        encrypt(KEY, &mut data);
        assert_ne!(data, plain);
        ContentKey::new(KEY).unwrap().decrypt(&mut data);
        assert_eq!(data, plain);
    }

    #[test]
    fn contents_index_accepts_encrypted_or_plaintext_bodies() {
        let body = br#"{"content":[{"path":"a.json","key":"abcdefghijklmnopqrstuvwxyz012345"},{"path":"b.png"},{"path":"c","key":"short"}]}"#;
        let mut encrypted = vec![0u8; CONTENTS_HEADER_BYTES];
        let mut tail = body.to_vec();
        encrypt(KEY, &mut tail);
        encrypted.extend_from_slice(&tail);
        let pack_key = ContentKey::new(KEY).unwrap();
        for raw in [encrypted.as_slice(), body.as_slice()] {
            let keys = contents_file_keys(raw, &pack_key).expect("index");
            assert_eq!(keys.len(), 1);
            assert!(keys.contains_key("a.json"));
        }
        assert_eq!(
            contents_file_keys(&[1; 64], &pack_key).err(),
            Some(AdmissionError::MalformedContentsIndex)
        );
        assert!(ContentKey::new(b"not-a-key").is_none());
    }
}
