//! The store's persistent material registry: an append-only table from block-state keys to
//! store-local [`MaterialId`]s.
//!
//! Keys survive sessions and pack changes: registry states use their Bedrock block-state
//! network hash, server-custom states their name and canonical state text. Appearance is
//! never stored; callers resolve it from current assets when they load a material.

use std::collections::HashMap;

use crate::{codec::crc32, voxel::MaterialId};

/// Leading bytes of an encoded registry.
pub const REGISTRY_MAGIC: [u8; 4] = *b"CLMR";
/// Version of the encoded registry layout.
pub const REGISTRY_FORMAT_VERSION: u16 = 1;
/// Longest custom name or state text, in UTF-8 bytes.
pub const MAX_KEY_TEXT_BYTES: usize = u16::MAX as usize;
/// Most keys one registry holds: every block id between the reserved air and hidden ids.
pub const MAX_MATERIALS: usize =
    (MaterialId::LAST_BLOCK.raw() - MaterialId::FIRST_BLOCK.raw() + 1) as usize;

const NETWORK_TAG: u8 = 0;
const CUSTOM_TAG: u8 = 1;
/// Magic, version and entry count.
const PREAMBLE_LEN: usize = REGISTRY_MAGIC.len() + 2 + 4;
const CRC_LEN: usize = 4;

/// Why a key or an encoded registry was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("every material id between air and hidden is assigned")]
    Full,
    #[error("custom block name is empty")]
    EmptyName,
    #[error("custom key text has {0} bytes; at most {MAX_KEY_TEXT_BYTES} are allowed")]
    TextLength(usize),
    #[error("encoded registry ends after {0} bytes")]
    Truncated(usize),
    #[error("encoded registry starts with {0:02x?}, not the registry magic")]
    Magic([u8; 4]),
    #[error("registry format version {0} is not supported")]
    Version(u16),
    #[error("registry declares {0} entries; at most {MAX_MATERIALS} fit")]
    Count(u32),
    #[error("entry {entry} has unknown tag {tag}")]
    Tag { entry: usize, tag: u8 },
    #[error("entry {0} holds text that is not UTF-8")]
    Utf8(usize),
    #[error("entry {0} repeats an earlier key")]
    Duplicate(usize),
    #[error("{0} bytes follow the last entry")]
    TrailingBytes(usize),
    #[error("registry checksum {stored:#010x} does not match {computed:#010x}")]
    Checksum { stored: u32, computed: u32 },
}

/// A server-custom block state: its namespaced name and canonical state text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CustomStateKey {
    name: Box<str>,
    states: Box<str>,
}

impl CustomStateKey {
    /// Validates a key. `states` must be canonical for the state, so the adapter formats it
    /// deterministically (for example sorted `name=value` pairs with typed values spelled
    /// distinctly); this crate compares it byte for byte.
    pub fn new(name: &str, states: &str) -> Result<Self, RegistryError> {
        if name.is_empty() {
            return Err(RegistryError::EmptyName);
        }
        for text in [name, states] {
            if text.len() > MAX_KEY_TEXT_BYTES {
                return Err(RegistryError::TextLength(text.len()));
            }
        }
        Ok(Self {
            name: name.into(),
            states: states.into(),
        })
    }

    /// The block name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The canonical block-state text.
    #[must_use]
    pub fn states(&self) -> &str {
        &self.states
    }
}

/// The persistent identity of one block state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MaterialKey {
    /// A state of the pinned block registry, by its Bedrock block-state network hash.
    Network(u32),
    /// A server-custom state, whose hash space may collide with registry states.
    Custom(CustomStateKey),
}

/// Append-only key → material table. Ids are assigned densely from
/// [`MaterialId::FIRST_BLOCK`] in insertion order and never change or get reused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialRegistry {
    keys: Vec<MaterialKey>,
    ids: HashMap<MaterialKey, MaterialId>,
}

impl MaterialRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of `key`, if assigned.
    #[must_use]
    pub fn get(&self, key: &MaterialKey) -> Option<MaterialId> {
        self.ids.get(key).copied()
    }

    /// The id of `key`, assigning the next free id when it is new.
    pub fn get_or_insert(&mut self, key: &MaterialKey) -> Result<MaterialId, RegistryError> {
        if let Some(id) = self.get(key) {
            return Ok(id);
        }
        self.push(key.clone())
    }

    /// The key a block id was assigned to; `None` for air, hidden and unassigned ids.
    #[must_use]
    pub fn key(&self, id: MaterialId) -> Option<&MaterialKey> {
        let index = id.raw().checked_sub(MaterialId::FIRST_BLOCK.raw())?;
        self.keys.get(index as usize)
    }

    /// Assigned keys in id order, starting at [`MaterialId::FIRST_BLOCK`].
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (MaterialId, &MaterialKey)> {
        self.keys
            .iter()
            .enumerate()
            .map(|(index, key)| (id_at(index), key))
    }

    /// Materials registered so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// True when no material is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Encodes the table: magic, version and entry count, the entries in id order, then a
    /// CRC32 of everything before it. Integers are little-endian.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PREAMBLE_LEN + self.keys.len() * 5 + CRC_LEN);
        out.extend_from_slice(&REGISTRY_MAGIC);
        out.extend_from_slice(&REGISTRY_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.keys.len() as u32).to_le_bytes());
        for key in &self.keys {
            match key {
                MaterialKey::Network(hash) => {
                    out.push(NETWORK_TAG);
                    out.extend_from_slice(&hash.to_le_bytes());
                }
                MaterialKey::Custom(custom) => {
                    out.push(CUSTOM_TAG);
                    for text in [custom.name(), custom.states()] {
                        out.extend_from_slice(&(text.len() as u16).to_le_bytes());
                        out.extend_from_slice(text.as_bytes());
                    }
                }
            }
        }
        let crc = crc32(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }

    /// Decodes [`MaterialRegistry::encode`] output, rejecting bad framing, checksums,
    /// unknown tags, invalid keys, duplicates and trailing bytes. Reads are bounded by the
    /// input length, so a corrupt count cannot force a large allocation.
    pub fn decode(bytes: &[u8]) -> Result<Self, RegistryError> {
        let Some((body, stored)) = bytes
            .len()
            .checked_sub(CRC_LEN)
            .filter(|&split| split >= PREAMBLE_LEN)
            .map(|split| bytes.split_at(split))
        else {
            return Err(RegistryError::Truncated(bytes.len()));
        };
        let mut reader = Reader { bytes: body, at: 0 };
        let magic = reader.array::<4>()?;
        if magic != REGISTRY_MAGIC {
            return Err(RegistryError::Magic(magic));
        }
        let version = u16::from_le_bytes(reader.array()?);
        if version != REGISTRY_FORMAT_VERSION {
            return Err(RegistryError::Version(version));
        }
        let stored = u32::from_le_bytes(stored.try_into().expect("four checksum bytes"));
        let computed = crc32(body);
        if stored != computed {
            return Err(RegistryError::Checksum { stored, computed });
        }
        let count = u32::from_le_bytes(reader.array()?);
        if count as usize > MAX_MATERIALS {
            return Err(RegistryError::Count(count));
        }
        let mut registry = Self::new();
        for entry in 0..count as usize {
            let key = match reader.array::<1>()?[0] {
                NETWORK_TAG => MaterialKey::Network(u32::from_le_bytes(reader.array()?)),
                CUSTOM_TAG => {
                    let name = reader.text(entry)?;
                    let states = reader.text(entry)?;
                    MaterialKey::Custom(CustomStateKey::new(name, states)?)
                }
                tag => return Err(RegistryError::Tag { entry, tag }),
            };
            if registry.ids.contains_key(&key) {
                return Err(RegistryError::Duplicate(entry));
            }
            registry.push(key)?;
        }
        match reader.remaining() {
            0 => Ok(registry),
            extra => Err(RegistryError::TrailingBytes(extra)),
        }
    }

    /// Appends `key` with the next material id; fails when the registry is full.
    fn push(&mut self, key: MaterialKey) -> Result<MaterialId, RegistryError> {
        if self.keys.len() >= MAX_MATERIALS {
            return Err(RegistryError::Full);
        }
        let id = id_at(self.keys.len());
        self.ids.insert(key.clone(), id);
        self.keys.push(key);
        Ok(id)
    }
}

/// The id of the entry at `index`; callers keep `index < MAX_MATERIALS`.
fn id_at(index: usize) -> MaterialId {
    MaterialId::new(MaterialId::FIRST_BLOCK.raw() + index as u32)
        .expect("registry indices stay below the hidden id")
}

/// Bounds-checked little-endian reads over an encoded registry.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    /// The next `len` bytes; fails when fewer remain.
    fn take(&mut self, len: usize) -> Result<&'a [u8], RegistryError> {
        let end = self
            .at
            .checked_add(len)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(RegistryError::Truncated(self.bytes.len() + CRC_LEN))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    /// The next `N` bytes as an array; fails when fewer remain.
    fn array<const N: usize>(&mut self) -> Result<[u8; N], RegistryError> {
        Ok(self.take(N)?.try_into().expect("took exactly N bytes"))
    }

    /// A `u16`-length-prefixed UTF-8 string; `entry` names the table entry in errors.
    fn text(&mut self, entry: usize) -> Result<&'a str, RegistryError> {
        let len = u16::from_le_bytes(self.array()?);
        std::str::from_utf8(self.take(usize::from(len))?).map_err(|_| RegistryError::Utf8(entry))
    }

    /// Bytes not yet read.
    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
}

#[cfg(test)]
mod tests;
