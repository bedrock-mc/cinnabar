//! MCBESND1: sound-event routing JSON plus an offset index over encoded sound files.
//!
//! Layout: header, three JSON blobs, index, SHA-256 of all of those, then file data. Only the
//! prefix is read at startup; sound files are read by offset on demand.

use std::collections::HashMap;

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const SOUND_BANK_MAGIC: &[u8; 8] = b"MCBESND1";
const SCHEMA: u32 = 1;
const HEADER_BYTES: usize = 40;
const HASH_BYTES: usize = 32;
pub const MAX_SOUND_BANK_PREFIX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SOUND_BANK_FILES: usize = 16_384;
pub const MAX_SOUND_BANK_PATH_BYTES: usize = 256;

#[derive(Debug, Error)]
#[error("invalid sound bank: {0}")]
pub struct SoundBankError(pub &'static str);

/// Location of one sound file inside the bank file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundBankEntry {
    pub offset: u64,
    pub len: u32,
}

#[derive(Clone, Debug)]
pub struct SoundBankIndex {
    sounds_json: Box<[u8]>,
    materials_json: Box<[u8]>,
    music_json: Box<[u8]>,
    entries: HashMap<Box<str>, SoundBankEntry>,
    prefix_sha256: [u8; 32],
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, SoundBankError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(SoundBankError("truncated field"))
}

/// Bytes the header declares for everything before the file data.
pub fn sound_bank_prefix_len(header: &[u8]) -> Result<usize, SoundBankError> {
    if header.get(..8) != Some(SOUND_BANK_MAGIC) || word(header, 8)? != SCHEMA {
        return Err(SoundBankError("magic or schema"));
    }
    let total = [12, 16, 20, 28]
        .into_iter()
        .try_fold(HEADER_BYTES + HASH_BYTES, |sum, at| {
            sum.checked_add(word(header, at).ok()? as usize)
        })
        .ok_or(SoundBankError("prefix length"))?;
    if total > MAX_SOUND_BANK_PREFIX_BYTES {
        return Err(SoundBankError("prefix exceeds its bound"));
    }
    Ok(total)
}

impl SoundBankIndex {
    /// Decodes the exact prefix (`sound_bank_prefix_len` bytes) of a bank file.
    pub fn decode_prefix(prefix: &[u8]) -> Result<Self, SoundBankError> {
        if sound_bank_prefix_len(prefix)? != prefix.len() {
            return Err(SoundBankError("prefix length mismatch"));
        }
        let hash_at = prefix.len() - HASH_BYTES;
        let expected: [u8; 32] = Sha256::digest(&prefix[..hash_at]).into();
        if prefix[hash_at..] != expected {
            return Err(SoundBankError("prefix hash mismatch"));
        }
        let lengths = [12, 16, 20].map(|at| word(prefix, at).unwrap_or(0) as usize);
        let file_count = word(prefix, 24)? as usize;
        let data_len = u64::from_le_bytes(prefix[32..40].try_into().expect("header width"));
        if (prefix.len() as u64).checked_add(data_len).is_none() {
            return Err(SoundBankError("absolute data extent overflow"));
        }
        if file_count > MAX_SOUND_BANK_FILES {
            return Err(SoundBankError("file count"));
        }
        let mut cursor = HEADER_BYTES;
        let blobs = lengths.map(|length| {
            let blob = &prefix[cursor..cursor + length];
            cursor += length;
            blob.to_vec().into_boxed_slice()
        });
        let index = &prefix[cursor..hash_at];
        let mut entries = HashMap::with_capacity(file_count);
        let mut at = 0_usize;
        for _ in 0..file_count {
            let path_len = usize::from(u16::from_le_bytes(
                index
                    .get(at..at + 2)
                    .ok_or(SoundBankError("truncated index"))?
                    .try_into()
                    .expect("two bytes"),
            ));
            let record = index
                .get(at + 2..at + 2 + path_len + 12)
                .ok_or(SoundBankError("truncated index"))?;
            let (path, tail) = record.split_at(path_len);
            let path = std::str::from_utf8(path).map_err(|_| SoundBankError("path encoding"))?;
            let offset = u64::from_le_bytes(tail[..8].try_into().expect("offset width"));
            let len = u32::from_le_bytes(tail[8..12].try_into().expect("length width"));
            if offset
                .checked_add(u64::from(len))
                .is_none_or(|end| end > data_len)
            {
                return Err(SoundBankError("entry outside data"));
            }
            entries.insert(
                Box::from(path),
                SoundBankEntry {
                    offset: prefix.len() as u64 + offset,
                    len,
                },
            );
            at += 2 + path_len + 12;
        }
        if at != index.len() {
            return Err(SoundBankError("trailing index bytes"));
        }
        let [sounds_json, materials_json, music_json] = blobs;
        Ok(Self {
            sounds_json,
            materials_json,
            music_json,
            entries,
            prefix_sha256: expected,
        })
    }

    pub fn sounds_json(&self) -> &[u8] {
        &self.sounds_json
    }

    pub fn materials_json(&self) -> &[u8] {
        &self.materials_json
    }

    pub fn music_json(&self) -> &[u8] {
        &self.music_json
    }

    /// Entry for a sound path without extension, e.g. `sounds/random/click`.
    pub fn entry(&self, path: &str) -> Option<SoundBankEntry> {
        self.entries.get(path).copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn prefix_sha256(&self) -> [u8; 32] {
        self.prefix_sha256
    }
}

/// Builds a bank; `files` are `(extensionless path, encoded bytes)` and order is canonicalized.
pub fn encode_sound_bank(
    sounds_json: &[u8],
    materials_json: &[u8],
    music_json: &[u8],
    files: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, SoundBankError> {
    let mut order: Vec<&(String, Vec<u8>)> = files.iter().collect();
    order.sort_by(|left, right| left.0.cmp(&right.0));
    if order.len() > MAX_SOUND_BANK_FILES
        || order.windows(2).any(|pair| pair[0].0 == pair[1].0)
        || order.iter().any(|(path, bytes)| {
            path.is_empty()
                || path.len() > MAX_SOUND_BANK_PATH_BYTES
                || u32::try_from(bytes.len()).is_err()
        })
    {
        return Err(SoundBankError("file set outside its bounds"));
    }
    let mut index = Vec::new();
    let mut offset = 0_u64;
    for (path, bytes) in &order {
        index.extend((path.len() as u16).to_le_bytes());
        index.extend(path.as_bytes());
        index.extend(offset.to_le_bytes());
        index.extend((bytes.len() as u32).to_le_bytes());
        offset = offset
            .checked_add(bytes.len() as u64)
            .ok_or(SoundBankError("data length overflow"))?;
    }
    let prefix_size = [sounds_json, materials_json, music_json]
        .into_iter()
        .try_fold(HEADER_BYTES + HASH_BYTES + index.len(), |total, blob| {
            u32::try_from(blob.len()).ok()?;
            total.checked_add(blob.len())
        })
        .filter(|size| *size <= MAX_SOUND_BANK_PREFIX_BYTES)
        .ok_or(SoundBankError("prefix length exceeds bound"))?;
    let capacity = usize::try_from(offset)
        .ok()
        .and_then(|data| prefix_size.checked_add(data))
        .ok_or(SoundBankError("carrier length overflow"))?;
    let mut out = Vec::with_capacity(capacity);
    out.extend(SOUND_BANK_MAGIC);
    out.extend(SCHEMA.to_le_bytes());
    for blob in [sounds_json, materials_json, music_json] {
        out.extend((blob.len() as u32).to_le_bytes());
    }
    out.extend((order.len() as u32).to_le_bytes());
    out.extend((index.len() as u32).to_le_bytes());
    out.extend(offset.to_le_bytes());
    for blob in [sounds_json, materials_json, music_json] {
        out.extend(blob);
    }
    out.extend(&index);
    let hash = Sha256::digest(&out);
    out.extend(hash);
    for (_, bytes) in order {
        out.extend(bytes);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        encode_sound_bank(
            b"{\"a\":1}",
            b"{}",
            b"{}",
            &[
                ("sounds/b".into(), vec![9, 9, 9]),
                ("sounds/a".into(), vec![1, 2]),
            ],
        )
        .expect("encode")
    }

    #[test]
    fn round_trips_and_locates_files() {
        let bytes = sample();
        let prefix_len = sound_bank_prefix_len(&bytes).expect("prefix length");
        let index = SoundBankIndex::decode_prefix(&bytes[..prefix_len]).expect("decode");
        assert_eq!(index.sounds_json(), b"{\"a\":1}");
        let entry = index.entry("sounds/b").expect("entry");
        let start = entry.offset as usize;
        assert_eq!(&bytes[start..start + entry.len as usize], [9, 9, 9]);
        assert!(index.entry("sounds/missing").is_none());
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn review_absolute_sound_offsets_cannot_overflow() {
        let mut bytes = sample();
        let prefix_len = sound_bank_prefix_len(&bytes).unwrap();
        bytes[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        let lengths = [12, 16, 20].map(|offset| word(&bytes, offset).unwrap() as usize);
        let first = HEADER_BYTES + lengths.into_iter().sum::<usize>();
        let name_len = u16::from_le_bytes(bytes[first..first + 2].try_into().unwrap()) as usize;
        let offset = first + 2 + name_len;
        bytes[offset..offset + 8].copy_from_slice(&(u64::MAX - 2).to_le_bytes());
        let hash = Sha256::digest(&bytes[..prefix_len - HASH_BYTES]);
        bytes[prefix_len - HASH_BYTES..prefix_len].copy_from_slice(&hash);
        assert!(SoundBankIndex::decode_prefix(&bytes[..prefix_len]).is_err());
    }

    #[test]
    fn review_sound_encoder_respects_the_prefix_bound() {
        assert!(
            encode_sound_bank(&vec![0; MAX_SOUND_BANK_PREFIX_BYTES], b"{}", b"{}", &[]).is_err()
        );
    }

    #[test]
    fn corrupt_prefix_is_rejected() {
        let mut bytes = sample();
        let prefix_len = sound_bank_prefix_len(&bytes).expect("prefix length");
        bytes[HEADER_BYTES] ^= 0xff;
        assert!(SoundBankIndex::decode_prefix(&bytes[..prefix_len]).is_err());
        assert!(sound_bank_prefix_len(b"short").is_err());
    }
}
