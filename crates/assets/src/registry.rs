use std::{collections::HashSet, str};

use bitflags::bitflags;

use crate::AssetError;

const REGISTRY_MAGIC: &[u8; 8] = b"BREG1001";
const RECORD_HEADER_BYTES: usize = 4 + 4 + 1 + 2 + 4;
const MAX_REGISTRY_RECORDS: usize = 65_536;
const MAX_REGISTRY_STATE_BYTES: usize = 1024 * 1024;

bitflags! {
    /// Facts exported from Dragonfly for one canonical block state.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct BlockFlags: u8 {
        const AIR = 1 << 0;
        const FULL_CUBE = 1 << 1;
    }
}

/// One record from the deterministic Dragonfly block-registry export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRecord {
    pub sequential_id: u32,
    pub network_hash: u32,
    pub name: Box<str>,
    pub canonical_state: Box<str>,
    pub flags: BlockFlags,
}

/// Reads the version-1001 Dragonfly registry export with allocation bounds.
pub fn read_registry(bytes: &[u8]) -> Result<Box<[RegistryRecord]>, AssetError> {
    let mut reader = Reader::new(bytes);
    if reader.read_exact(REGISTRY_MAGIC.len(), "registry magic")? != REGISTRY_MAGIC {
        return Err(AssetError::InvalidRegistryMagic);
    }

    let count = reader.read_u32("registry record count")? as usize;
    if count > MAX_REGISTRY_RECORDS {
        return Err(AssetError::TooManyRegistryRecords {
            count,
            max: MAX_REGISTRY_RECORDS,
        });
    }

    let minimum_bytes = count * RECORD_HEADER_BYTES;
    if reader.remaining() < minimum_bytes {
        return Err(AssetError::UnexpectedEof {
            context: "registry record headers",
            needed: minimum_bytes,
            remaining: reader.remaining(),
        });
    }

    let mut records = Vec::with_capacity(count);
    let mut sequential_ids = HashSet::with_capacity(count);
    let mut network_hashes = HashSet::with_capacity(count);
    for _ in 0..count {
        let sequential_id = reader.read_u32("record sequential ID")?;
        let network_hash = reader.read_u32("record network hash")?;
        let raw_flags = reader.read_u8("record flags")?;
        let name_len = reader.read_u16("record name length")? as usize;
        let state_len = reader.read_u32("record state length")? as usize;

        if !sequential_ids.insert(sequential_id) {
            return Err(AssetError::DuplicateSequentialId(sequential_id));
        }
        if !network_hashes.insert(network_hash) {
            return Err(AssetError::DuplicateNetworkHash(network_hash));
        }
        let flags =
            BlockFlags::from_bits(raw_flags).ok_or(AssetError::InvalidRegistryFlags(raw_flags))?;
        if state_len > MAX_REGISTRY_STATE_BYTES {
            return Err(AssetError::RegistryStateTooLarge {
                size: state_len,
                max: MAX_REGISTRY_STATE_BYTES,
            });
        }

        let name = str::from_utf8(reader.read_exact(name_len, "record name")?)
            .map_err(|source| AssetError::InvalidRegistryUtf8 {
                field: "name",
                source,
            })?
            .into();
        let canonical_state = str::from_utf8(reader.read_exact(state_len, "record state")?)
            .map_err(|source| AssetError::InvalidRegistryUtf8 {
                field: "canonical state",
                source,
            })?
            .into();

        records.push(RegistryRecord {
            sequential_id,
            network_hash,
            name,
            canonical_state,
            flags,
        });
    }

    if reader.remaining() != 0 {
        return Err(AssetError::TrailingRegistryBytes {
            remaining: reader.remaining(),
        });
    }
    Ok(records.into_boxed_slice())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn read_u8(&mut self, context: &'static str) -> Result<u8, AssetError> {
        Ok(self.read_exact(1, context)?[0])
    }

    fn read_u16(&mut self, context: &'static str) -> Result<u16, AssetError> {
        Ok(u16::from_le_bytes(
            self.read_exact(2, context)?
                .try_into()
                .expect("two-byte slice"),
        ))
    }

    fn read_u32(&mut self, context: &'static str) -> Result<u32, AssetError> {
        Ok(u32::from_le_bytes(
            self.read_exact(4, context)?
                .try_into()
                .expect("four-byte slice"),
        ))
    }

    fn read_exact(&mut self, count: usize, context: &'static str) -> Result<&'a [u8], AssetError> {
        let remaining = self.remaining();
        if remaining < count {
            return Err(AssetError::UnexpectedEof {
                context,
                needed: count,
                remaining,
            });
        }
        let start = self.position;
        self.position += count;
        Ok(&self.bytes[start..self.position])
    }
}
