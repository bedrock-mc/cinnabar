//! Cache namespaces: a hashed server identity, a dimension and a world identity.

use std::fmt::{self, Write};

use sha2::{Digest, Sha256};

use crate::{COLUMN_CELLS, ColumnSummary};

/// One-way identity of a server; the address never reaches disk or logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerKey([u8; 16]);

impl ServerKey {
    /// Hashes a destination such as `remote:host:port` or `local:<world id>`.
    #[must_use]
    pub fn from_destination(destination: &str) -> Self {
        let digest = Sha256::new()
            .chain_update(b"cinnabar far-terrain server\0")
            .chain_update(destination.trim().to_lowercase().as_bytes())
            .finalize();
        Self(std::array::from_fn(|index| digest[index]))
    }

    /// Parses the lowercase hex that [`fmt::Display`] writes.
    #[must_use]
    pub fn from_hex(text: &str) -> Option<Self> {
        parse_hex(text).map(Self)
    }
}

impl fmt::Display for ServerKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0
            .iter()
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}

/// A dimension of one StartGame level identity on one server, before its epoch is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldKey {
    pub server: ServerKey,
    pub dimension: i32,
    pub level: u64,
    /// Surface resources select a namespace while sharing the destination's disk quota.
    pub resources: [u8; 32],
}

impl WorldKey {
    #[must_use]
    pub const fn namespace(self, epoch: u32) -> NamespaceKey {
        NamespaceKey {
            server: self.server,
            dimension: self.dimension,
            level: self.level,
            epoch,
            resources: self.resources,
        }
    }
}

/// A world on one server: a dimension, the StartGame level identity, and an epoch that
/// separates different maps served under the same level identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NamespaceKey {
    pub server: ServerKey,
    pub dimension: i32,
    pub level: u64,
    pub epoch: u32,
    pub resources: [u8; 32],
}

impl NamespaceKey {
    #[must_use]
    pub const fn world(self) -> WorldKey {
        WorldKey {
            server: self.server,
            dimension: self.dimension,
            level: self.level,
            resources: self.resources,
        }
    }

    /// Directory name below the server's directory.
    #[must_use]
    pub fn directory(self) -> String {
        let mut directory = format!("{}.{:016x}.{}", self.dimension, self.level, self.epoch);
        if self.resources != [0; 32] {
            directory.push('.');
            for byte in self.resources {
                write!(directory, "{byte:02x}").expect("writing to a string");
            }
        }
        directory
    }

    /// Parses [`NamespaceKey::directory`] output for `server`.
    #[must_use]
    pub fn parse(server: ServerKey, directory: &str) -> Option<Self> {
        let mut parts = directory.split('.');
        let key = Self {
            server,
            dimension: parts.next()?.parse().ok()?,
            level: u64::from_str_radix(parts.next()?, 16).ok()?,
            epoch: parts.next()?.parse().ok()?,
            resources: match parts.next() {
                Some(resources) => parse_hex(resources)?,
                None => [0; 32],
            },
        };
        parts.next().is_none().then_some(key)
    }
}

/// Decodes an exact lowercase hexadecimal cache key, rejecting malformed directory names.
fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    let mut key = [0; N];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(key)
}

/// Share of cells whose surfaces agree within a block, empty cells agreeing only with empty.
#[must_use]
pub fn surface_similarity(a: &ColumnSummary, b: &ColumnSummary) -> f32 {
    let agreeing = a
        .cells()
        .iter()
        .zip(b.cells())
        .filter(|(a, b)| match (a.is_empty(), b.is_empty()) {
            (true, true) => true,
            (false, false) => (i32::from(a.height) - i32::from(b.height)).abs() <= 1,
            _ => false,
        })
        .count();
    agreeing as f32 / COLUMN_CELLS as f32
}
