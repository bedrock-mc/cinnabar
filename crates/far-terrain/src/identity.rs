//! Cache namespaces: a hashed server identity, a dimension and a world identity.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::{COLUMN_CELLS, ColumnSummary};

/// One-way identity of a server; the address never reaches disk or logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerKey([u8; 16]);

impl ServerKey {
    /// Separates cached surface colors produced by different resource-pack inputs.
    #[must_use]
    pub fn with_resources(self, resources: [u8; 32]) -> Self {
        if resources == [0; 32] {
            return self;
        }
        let digest = Sha256::new()
            .chain_update(b"cinnabar far-terrain resources\0")
            .chain_update(self.0)
            .chain_update(resources)
            .finalize();
        Self(std::array::from_fn(|index| digest[index]))
    }

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
        if text.len() != 32
            || !text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return None;
        }
        let mut key = [0; 16];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Self(key))
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
}

impl WorldKey {
    #[must_use]
    pub const fn namespace(self, epoch: u32) -> NamespaceKey {
        NamespaceKey {
            server: self.server,
            dimension: self.dimension,
            level: self.level,
            epoch,
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
}

impl NamespaceKey {
    #[must_use]
    pub const fn world(self) -> WorldKey {
        WorldKey {
            server: self.server,
            dimension: self.dimension,
            level: self.level,
        }
    }

    /// Directory name below the server's directory.
    #[must_use]
    pub fn directory(self) -> String {
        format!("{}.{:016x}.{}", self.dimension, self.level, self.epoch)
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
        };
        parts.next().is_none().then_some(key)
    }
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
