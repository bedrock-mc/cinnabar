use std::{collections::BTreeMap, path::Path};

use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use aes::{Aes128, Aes192, Aes256};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

use super::resource_pack_items::strip_json_comments;
use super::{PackEntry, ServerResourcePackError, canonical_path};

#[derive(Deserialize)]
struct EncryptedContents {
    content: Vec<EncryptedContent>,
}

#[derive(Deserialize)]
struct EncryptedContent {
    path: String,
    #[serde(default)]
    key: String,
}

pub(super) fn decrypt_pack_entries(
    pack: &str,
    content_key: &str,
    entries: &mut [PackEntry],
) -> Result<(), ServerResourcePackError> {
    let pack_key = decode_resource_pack_key(content_key).map_err(|detail| {
        ServerResourcePackError::EncryptedPack {
            pack: pack.into(),
            detail,
        }
    })?;
    let contents_index = entries
        .iter()
        .position(|entry| {
            entry.path.as_ref() == "contents.json" || entry.path.ends_with("/contents.json")
        })
        .ok_or_else(|| ServerResourcePackError::EncryptedPack {
            pack: pack.into(),
            detail: "encrypted pack is missing contents.json".into(),
        })?;
    let contents_bytes = entries[contents_index].bytes.clone();
    let contents = parse_encrypted_contents(&contents_bytes, &pack_key).map_err(|detail| {
        ServerResourcePackError::EncryptedPack {
            pack: pack.into(),
            detail,
        }
    })?;
    let mut file_keys = BTreeMap::<Box<str>, Box<[u8]>>::new();
    for content in contents.content {
        if content.key.trim().is_empty() {
            continue;
        }
        let path = canonical_path(Some(Path::new(&content.path))).ok_or_else(|| {
            ServerResourcePackError::EncryptedPack {
                pack: pack.into(),
                detail: format!("encrypted content path {} is unsafe", content.path)
                    .into_boxed_str(),
            }
        })?;
        let key = decode_resource_pack_key(&content.key).map_err(|detail| {
            ServerResourcePackError::EncryptedPack {
                pack: pack.into(),
                detail: format!("invalid key for encrypted content {path}: {detail}")
                    .into_boxed_str(),
            }
        })?;
        file_keys.insert(path, key);
    }
    for entry in entries {
        let Some(key) = file_keys.get(entry.path.as_ref()) else {
            continue;
        };
        let decrypted = decrypt_cfb8(key, &entry.bytes).map_err(|detail| {
            ServerResourcePackError::EncryptedPack {
                pack: pack.into(),
                detail: format!("could not decrypt {}: {detail}", entry.path).into_boxed_str(),
            }
        })?;
        if entry.path.ends_with(".json")
            && json_document_is_valid(&entry.bytes)
            && !json_document_is_valid(&decrypted)
        {
            continue;
        }
        entry.bytes = decrypted.into_boxed_slice();
    }
    Ok(())
}

fn parse_encrypted_contents(raw: &[u8], pack_key: &[u8]) -> Result<EncryptedContents, Box<str>> {
    if let Some(contents) = parse_json_document(raw) {
        return deserialize_json(&contents);
    }
    const ENCRYPTED_CONTENTS_HEADER_BYTES: usize = 256;
    if raw.len() <= ENCRYPTED_CONTENTS_HEADER_BYTES {
        return Err("encrypted contents.json is too small".into());
    }
    let decrypted = decrypt_cfb8(pack_key, &raw[ENCRYPTED_CONTENTS_HEADER_BYTES..])?;
    let contents = parse_json_document(&decrypted)
        .ok_or_else(|| Box::<str>::from("decrypted contents.json is not valid JSON"))?;
    deserialize_json(&contents)
}

fn deserialize_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Box<str>> {
    serde_json::from_slice(bytes)
        .map_err(|source| format!("could not parse JSON: {source}").into_boxed_str())
}

fn parse_json_document(bytes: &[u8]) -> Option<Vec<u8>> {
    let normalized = strip_json_comments(bytes)?;
    serde_json::from_slice::<Value>(&normalized).ok()?;
    Some(normalized)
}

fn json_document_is_valid(bytes: &[u8]) -> bool {
    parse_json_document(bytes).is_some()
}

fn decode_resource_pack_key(value: &str) -> Result<Box<[u8]>, Box<str>> {
    let raw = value.trim().as_bytes();
    if matches!(raw.len(), 16 | 24 | 32) {
        return Ok(raw.to_vec().into_boxed_slice());
    }
    for decoded in [STANDARD.decode(raw), STANDARD_NO_PAD.decode(raw)] {
        if let Ok(decoded) = decoded
            && matches!(decoded.len(), 16 | 24 | 32)
        {
            return Ok(decoded.into_boxed_slice());
        }
    }
    Err("resource-pack key must be 16, 24, or 32 raw bytes or a base64 encoding".into())
}

fn decrypt_cfb8(key: &[u8], encrypted: &[u8]) -> Result<Vec<u8>, Box<str>> {
    match key.len() {
        16 => Ok(decrypt_cfb8_with(
            Aes128::new_from_slice(key)
                .map_err(|_| Box::<str>::from("could not initialize AES-128"))?,
            key,
            encrypted,
        )),
        24 => Ok(decrypt_cfb8_with(
            Aes192::new_from_slice(key)
                .map_err(|_| Box::<str>::from("could not initialize AES-192"))?,
            key,
            encrypted,
        )),
        32 => Ok(decrypt_cfb8_with(
            Aes256::new_from_slice(key)
                .map_err(|_| Box::<str>::from("could not initialize AES-256"))?,
            key,
            encrypted,
        )),
        length => Err(format!("invalid AES key length {length}").into_boxed_str()),
    }
}

fn decrypt_cfb8_with<C>(cipher: C, key: &[u8], encrypted: &[u8]) -> Vec<u8>
where
    C: BlockEncrypt,
{
    let mut state = [0u8; 16];
    state.copy_from_slice(&key[..16]);
    let mut plaintext = Vec::with_capacity(encrypted.len());
    for &byte in encrypted {
        let mut keystream = GenericArray::clone_from_slice(&state);
        cipher.encrypt_block(&mut keystream);
        let decoded = byte ^ keystream[0];
        plaintext.push(decoded);
        state.copy_within(1.., 0);
        state[15] = byte;
    }
    plaintext
}
