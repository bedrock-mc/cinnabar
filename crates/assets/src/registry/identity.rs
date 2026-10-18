//! Borrowed diagnostic identities from the embedded, startup-validated block registry.

use std::{str, sync::OnceLock};

use super::{
    Reader,
    schema::{
        MAX_COLLISION_BOXES, MAX_REGISTRY_RECORDS, MAX_REGISTRY_STATE_BYTES, RECORD_HEADER_BYTES,
        REGISTRY_MAGIC, read_collision_box, read_record_header, read_registry_metadata,
    },
};
use crate::AssetError;

/// The text fields needed by observations, without decoded collision or state facts.
pub(super) struct RegistryIdentity<'a> {
    pub(super) sequential_id: u32,
    pub(super) name: &'a str,
    pub(super) canonical_state: &'a str,
}

/// Returns exact pinned name/state text without decoding the registry's semantic facts.
/// The immutable index borrows the embedded bytes; startup remains responsible for full validation.
pub fn pinned_block_identity(sequential_id: u32) -> Option<(&'static str, &'static str)> {
    static IDENTITIES: OnceLock<Box<[RegistryIdentity<'static>]>> = OnceLock::new();
    let identities = IDENTITIES.get_or_init(|| {
        read_identities(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .unwrap_or_default()
    });
    let identity = identities.get(sequential_id as usize)?;
    (identity.sequential_id == sequential_id).then_some((identity.name, identity.canonical_state))
}

/// Projects bounded current-format framing into borrowed text, leaving semantic validation to startup.
pub(super) fn read_identities(
    bytes: &[u8],
    expected_protocol: u32,
) -> Result<Box<[RegistryIdentity<'_>]>, AssetError> {
    let mut reader = Reader::new(bytes);
    if reader.read_exact(REGISTRY_MAGIC.len(), "registry magic")? != REGISTRY_MAGIC {
        return Err(AssetError::InvalidRegistryMagic);
    }
    let metadata = read_registry_metadata(&mut reader)?;
    if metadata.protocol != expected_protocol {
        return Err(AssetError::InvalidRegistryMagic);
    }
    let count = metadata.canonical_states as usize;
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
    let mut identities = Vec::with_capacity(count);
    for _ in 0..count {
        let header = read_record_header(&mut reader, false)?;
        if usize::from(header.box_count) > MAX_COLLISION_BOXES {
            return Err(AssetError::InvalidRegistryFlags(header.model_mask));
        }
        let state_len = header.state_len as usize;
        if state_len > MAX_REGISTRY_STATE_BYTES {
            return Err(AssetError::RegistryStateTooLarge {
                size: state_len,
                max: MAX_REGISTRY_STATE_BYTES,
            });
        }
        for _ in 0..header.box_count {
            let _ = read_collision_box(&mut reader)?;
        }
        let name = str::from_utf8(reader.read_exact(usize::from(header.name_len), "record name")?)
            .map_err(|source| AssetError::InvalidRegistryUtf8 {
                field: "name",
                source,
            })?;
        let canonical_state = str::from_utf8(reader.read_exact(state_len, "record state")?)
            .map_err(|source| AssetError::InvalidRegistryUtf8 {
                field: "canonical state",
                source,
            })?;
        identities.push(RegistryIdentity {
            sequential_id: header.sequential_id,
            name,
            canonical_state,
        });
    }
    if reader.remaining() != 0 {
        return Err(AssetError::TrailingRegistryBytes {
            remaining: reader.remaining(),
        });
    }
    Ok(identities.into_boxed_slice())
}
