//! Resource-pack content encryption: AES-CFB8 whose IV is the key's first
//! 16 bytes, a `contents.json` index behind a 256-byte header, and per-file keys.

use std::collections::HashMap;

use aes::cipher::{BlockCipherEncrypt, KeyInit};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

use pack_json::parse_jsonc;

use crate::{AdmissionError, MAX_ENTRIES_PER_PACK, MAX_PATH_BYTES};

const CONTENTS_HEADER_BYTES: usize = 256;

/// A content key retained only in memory and zeroed on drop.
pub(crate) struct ContentKey(Box<[u8]>);

impl ContentKey {
    /// Accepts only AES-128/192/256 key lengths after trimming ASCII whitespace.
    pub(crate) fn new(bytes: &[u8]) -> Option<Self> {
        let trimmed = bytes.trim_ascii();
        matches!(trimmed.len(), 16 | 24 | 32).then(|| Self(trimmed.into()))
    }

    /// Whether both keys decrypt identically; used only to compare admitted packs in memory.
    pub(crate) fn same_key(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    /// Identifies a decryption binding without exposing its key bytes to cache callers.
    pub(crate) fn digest(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(&self.0).into()
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

/// Decrypts CFB8 using the first 16 bytes of the supplied IV.
fn cfb8_decrypt<C: BlockCipherEncrypt<BlockSize = aes::cipher::consts::U16>>(
    cipher: &C,
    iv: &[u8],
    data: &mut [u8],
) {
    let mut register = [0u8; 16];
    register.copy_from_slice(&iv[..16]);
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
    #[serde(default, deserialize_with = "deserialize_key")]
    key: Option<Zeroizing<String>>,
}

/// Arms wiping when a parsed key string is returned, including partially built entries.
/// Serde's temporary buffers before string ownership transfers are outside this boundary.
fn deserialize_key<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Zeroizing<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(|key| key.map(Zeroizing::new))
}

/// Returns per-file keys from a `contents.json` that is either plaintext or
/// encrypted with the pack key after its fixed header. Entries with a malformed
/// path or key are ignored; the file is then read as stored.
pub(crate) fn contents_file_keys(
    raw: &[u8],
    pack_key: &ContentKey,
) -> Result<HashMap<Box<str>, ContentKey>, AdmissionError> {
    // Wipe the decrypted input and owned keys; parser temporaries are outside this boundary.
    let contents = match parse_jsonc::<Contents>(raw) {
        Ok(contents) => contents,
        Err(_) => {
            // A well-formed document with the wrong shape is not an encrypted index.
            if parse_jsonc::<serde::de::IgnoredAny>(raw).is_ok() {
                return Err(AdmissionError::MalformedContentsIndex);
            }
            let mut body = Zeroizing::new(
                raw.get(CONTENTS_HEADER_BYTES..)
                    .filter(|body| !body.is_empty())
                    .ok_or(AdmissionError::MalformedContentsIndex)?
                    .to_vec(),
            );
            pack_key.decrypt(&mut body);
            parse_jsonc::<Contents>(&body).map_err(|_| AdmissionError::MalformedContentsIndex)?
        }
    };
    if contents.content.len() > MAX_ENTRIES_PER_PACK {
        return Err(AdmissionError::TooManyEntries);
    }
    let mut keys = HashMap::with_capacity(contents.content.len());
    for entry in contents.content {
        let key = entry
            .key
            .as_deref()
            .and_then(|key| ContentKey::new(key.as_bytes()));
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

    /// Converts a public known-answer fixture into bytes without a parsing dependency.
    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn cfb8_matches_nist_decryption_vectors_for_every_key_size() {
        // NIST SP 800-38A, F.3.8, F.3.10 and F.3.12 (CFB8 decryption).
        let iv = hex("000102030405060708090a0b0c0d0e0f");
        let plain = hex("6bc1bee22e409f96e93d7e117393172aae2d");
        for (key, encrypted) in [
            (
                "2b7e151628aed2a6abf7158809cf4f3c",
                "3b79424c9c0dd436bace9e0ed4586a4f32b9",
            ),
            (
                "8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b",
                "cda2521ef0a905ca44cd057cbf0d47a0678a",
            ),
            (
                "603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4",
                "dc1f1a8520a64db55fcc8ac554844e889700",
            ),
        ] {
            let key = hex(key);
            let mut actual = hex(encrypted);
            match key.len() {
                16 => cfb8_decrypt(
                    &aes::Aes128::new_from_slice(&key).unwrap(),
                    &iv,
                    &mut actual,
                ),
                24 => cfb8_decrypt(
                    &aes::Aes192::new_from_slice(&key).unwrap(),
                    &iv,
                    &mut actual,
                ),
                _ => cfb8_decrypt(
                    &aes::Aes256::new_from_slice(&key).unwrap(),
                    &iv,
                    &mut actual,
                ),
            }
            assert_eq!(actual, plain);
        }
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

    #[test]
    fn contents_index_jsonc_matches_previous_normalization() {
        let pack_key = ContentKey::new(KEY).unwrap();
        for body in [
            br#"{"content":[{"path":"a.json","key":"abcdefghijklmnopqrstuvwxyz012345"}]}"#.as_slice(),
            b"\xef\xbb\xbf{/* index */\"content\":[{\"path\":\"caf\xe9.json\",\"key\":\"abcdefghijklmnopqrstuvwxyz012345\",},],}",
            br#"{"content":[],"ignored":1e309}"#,
            br#"{"content":[],"content":[]}"#,
            br#"{"content":[{"path":"a","path":"b"}]}"#,
            br#"{"content":[{"key":"abcdefghijklmnopqrstuvwxyz012345","key":"abcdefghijklmnopqrstuvwxyz012345"}]}"#,
            br#"{"content":false}"#,
            br#"{"content":false,"truncated":}"#,
            b"{} {}",
        ] {
            let before = pack_json::normalize_jsonc(body)
                .and_then(|json| serde_json::from_slice::<Contents>(&json).ok());
            let mut encrypted = vec![0; CONTENTS_HEADER_BYTES];
            let mut tail = body.to_vec();
            encrypt(KEY, &mut tail);
            encrypted.extend_from_slice(&tail);
            for raw in [body, encrypted.as_slice()] {
                let after = contents_file_keys(raw, &pack_key);
                if let Some(expected) = &before {
                    let after = after.expect("previously accepted index");
                    assert_eq!(after.len(), expected.content.len());
                    for entry in &expected.content {
                        let key = ContentKey::new(entry.key.as_ref().unwrap().as_bytes()).unwrap();
                        assert!(after[entry.path.as_str()].same_key(&key));
                    }
                } else {
                    assert_eq!(after.err(), Some(AdmissionError::MalformedContentsIndex));
                }
            }
        }
    }
}
