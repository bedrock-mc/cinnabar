//! Which store and dimension directory a session's terrain belongs to.
//!
//! A [`StoreKey`] names one destination world: the client's stable destination identity,
//! plus StartGame's level id when the server sets one and its seed when non-zero
//! (Gophertunnel-based servers leave both unset). Its directory name is the hex SHA-256 of
//! a versioned, length-prefixed encoding, so no two keys share a directory. Each
//! [`Dimension`] gets its own directory inside the store; worlds that share one key, as
//! behind a proxy, are told apart by epochs.

use sha2::{Digest, Sha256};

/// Domain separation for the key digest.
const DIGEST_DOMAIN: &[u8] = b"cinnabar-lod-store";
/// Version of the key encoding; changing the encoding changes every directory name.
pub const IDENTITY_ENCODING_VERSION: u8 = 1;
/// Length of a store directory name: a lowercase hex SHA-256.
pub const STORE_DIR_NAME_LEN: usize = 64;

const DESTINATION_TAG: u8 = 1;
const LEVEL_ID_TAG: u8 = 2;
const SEED_TAG: u8 = 3;
const DIMENSION_PREFIX: &str = "dim";

/// The identity of one destination world's store.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoreKey {
    destination: Box<str>,
    level_id: Option<Box<str>>,
    seed: Option<u64>,
}

impl StoreKey {
    /// The key of `destination` with StartGame's `level_id` and `seed`; an empty level id
    /// and a zero seed are treated as unset.
    #[must_use]
    pub fn new(destination: &str, level_id: &str, seed: u64) -> Self {
        Self {
            destination: destination.into(),
            level_id: (!level_id.is_empty()).then(|| level_id.into()),
            seed: (seed != 0).then_some(seed),
        }
    }

    /// The destination identity.
    #[must_use]
    pub fn destination(&self) -> &str {
        &self.destination
    }

    /// StartGame's level id, when set.
    #[must_use]
    pub fn level_id(&self) -> Option<&str> {
        self.level_id.as_deref()
    }

    /// StartGame's seed, when non-zero.
    #[must_use]
    pub fn seed(&self) -> Option<u64> {
        self.seed
    }

    /// The versioned encoding the digest covers: each present field as a tag byte, then
    /// a little-endian `u32` length and UTF-8 text, or the seed as a little-endian `u64`.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DIGEST_DOMAIN.len() + 32 + self.destination.len());
        out.extend_from_slice(DIGEST_DOMAIN);
        out.push(IDENTITY_ENCODING_VERSION);
        push_text(&mut out, DESTINATION_TAG, &self.destination);
        if let Some(level_id) = &self.level_id {
            push_text(&mut out, LEVEL_ID_TAG, level_id);
        }
        if let Some(seed) = self.seed {
            out.push(SEED_TAG);
            out.extend_from_slice(&seed.to_le_bytes());
        }
        out
    }

    /// SHA-256 of [`Self::encode`].
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.encode()).into()
    }

    /// The store directory name: [`Self::digest`] as lowercase hex.
    #[must_use]
    pub fn dir_name(&self) -> String {
        use std::fmt::Write;
        self.digest().iter().fold(
            String::with_capacity(STORE_DIR_NAME_LEN),
            |mut out, byte| {
                write!(out, "{byte:02x}").expect("writing to a String cannot fail");
                out
            },
        )
    }
}

/// True for names [`StoreKey::dir_name`] can produce.
#[must_use]
pub fn is_store_dir_name(name: &str) -> bool {
    name.len() == STORE_DIR_NAME_LEN
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Appends a tagged, length-prefixed text field.
fn push_text(out: &mut Vec<u8>, tag: u8, text: &str) {
    out.push(tag);
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

/// A Bedrock dimension id, as StartGame and ChangeDimension carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dimension(pub i32);

impl Dimension {
    /// The dimension's directory name inside a store, which [`Self::from_dir_name`] reads
    /// back.
    #[must_use]
    pub fn dir_name(self) -> String {
        format!("{DIMENSION_PREFIX}{}", self.0)
    }

    /// Parses a name written by [`Self::dir_name`]; `None` for any other name.
    #[must_use]
    pub fn from_dir_name(name: &str) -> Option<Self> {
        let id: i32 = name.strip_prefix(DIMENSION_PREFIX)?.parse().ok()?;
        let dimension = Self(id);
        (dimension.dir_name() == name).then_some(dimension)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_fields_are_normalised_and_set_ones_change_the_directory() {
        let base = StoreKey::new("remote:play.example:19132", "", 0);
        assert_eq!(base.level_id(), None);
        assert_eq!(base.seed(), None);
        let with_level = StoreKey::new("remote:play.example:19132", "world", 0);
        let with_seed = StoreKey::new("remote:play.example:19132", "", 7);
        let names = [&base, &with_level, &with_seed].map(StoreKey::dir_name);
        assert!(names.iter().all(|name| is_store_dir_name(name)));
        assert_ne!(names[0], names[1]);
        assert_ne!(names[0], names[2]);
        assert_ne!(names[1], names[2]);
        assert_eq!(
            base.dir_name(),
            StoreKey::new(base.destination(), "", 0).dir_name()
        );
    }

    #[test]
    fn field_boundaries_cannot_be_confused() {
        // Without length prefixes these two would encode the same bytes.
        let a = StoreKey::new("remote:a", "bc", 0);
        let b = StoreKey::new("remote:ab", "c", 0);
        assert_ne!(a.encode(), b.encode());
        assert_ne!(a.dir_name(), b.dir_name());
    }

    #[test]
    fn the_digest_is_pinned() {
        // Pinned: directory names are persisted, so the encoding must not change silently.
        let key = StoreKey::new("local:world", "level", 42);
        assert_eq!(&key.dir_name()[..16], PINNED_PREFIX);
    }

    /// The first 16 hex digits of the pinned key's directory name.
    const PINNED_PREFIX: &str = "5245aa9137f9297e";

    #[test]
    fn dimension_directories_round_trip_canonically() {
        for id in [0, 1, 2, -1, i32::MIN, i32::MAX] {
            let dimension = Dimension(id);
            assert_eq!(
                Dimension::from_dir_name(&dimension.dir_name()),
                Some(dimension)
            );
        }
        for name in ["dim", "dim+1", "dim01", "dimx", "e1", "dim-0"] {
            assert_eq!(Dimension::from_dir_name(name), None, "{name}");
        }
        assert!(!is_store_dir_name(&"A".repeat(STORE_DIR_NAME_LEN)));
        assert!(!is_store_dir_name("abc"));
    }
}
