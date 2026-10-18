use std::{collections::HashSet, str};

use crate::AssetError;

mod identity;
pub use identity::pinned_block_identity;

mod schema;
use crate::facts::{self, family_routing};

#[cfg(test)]
mod wire_tests;

pub use schema::{
    BeaconTint, BlockFlags, BlockTraits, CollisionBox, CollisionConfidence, CollisionSeed,
    ContributorRole, LEGACY_PHYSICS_REGISTRY_MAGIC, ModelFamily, ModelState, ModelStateField,
    MovementSemantics, PHYSICS_MOVEMENT_SEMANTICS_OFFSET, PHYSICS_RECORD_HEADER_BYTES,
    PHYSICS_REGISTRY_HEADER_BYTES, PHYSICS_REGISTRY_MAGIC, RegistryMetadata, RegistryProvenance,
    RegistryRecord, SurvivalRule,
};
use schema::{
    LEGACY_REGISTRY_PROTOCOL, MAX_COLLISION_BOXES, MAX_REGISTRY_RECORDS, MAX_REGISTRY_STATE_BYTES,
    PREVIOUS_RECORD_HEADER_BYTES, PREVIOUS_REGISTRY_MAGIC, RECORD_HEADER_BYTES, REGISTRY_MAGIC,
    RecordHeader, read_collision_box, read_record_header, read_registry_metadata,
};

impl BlockFlags {
    #[must_use]
    pub const fn has_valid_semantics(self) -> bool {
        let air = self.contains(Self::AIR);
        let cube = self.contains(Self::CUBE_GEOMETRY);
        let leaf = self.contains(Self::LEAF_MODEL);
        (!air || self.bits() == Self::AIR.bits())
            && (!leaf || (cube && !self.contains(Self::OCCLUDES_FULL_FACE)))
    }
}

impl ModelState {
    /// Records a typed selector and marks it present in the carrier header.
    pub fn set(&mut self, field: ModelStateField, value: u32) {
        let index = usize::from(field as u8 - 1);
        self.mask |= 1 << index;
        self.values[index] = value;
    }

    #[must_use]
    pub fn get(self, field: ModelStateField) -> Option<u32> {
        let index = usize::from(field as u8 - 1);
        (self.mask & (1 << index) != 0).then_some(self.values[index])
    }

    #[must_use]
    pub const fn mask(self) -> u8 {
        self.mask
    }
}

/// Reads the bounded protocol-1001 registry, deriving facts omitted by its historical format.
pub fn read_registry(bytes: &[u8]) -> Result<Box<[RegistryRecord]>, AssetError> {
    read_registry_for_protocol(bytes, LEGACY_REGISTRY_PROTOCOL)
}

/// Reads the wire protocol stamped in a supported block header without decoding
/// any record payload.
///
/// Startup cross-carrier coherence checks use this cheap reader ahead of the
/// full decode; malformed headers surface the same error classes the full
/// reader produces.
pub fn registry_header_protocol(bytes: &[u8]) -> Result<u32, AssetError> {
    let mut reader = Reader::new(bytes);
    let magic = reader.read_exact(REGISTRY_MAGIC.len(), "registry magic")?;
    if magic != REGISTRY_MAGIC && magic != PREVIOUS_REGISTRY_MAGIC {
        return Err(AssetError::InvalidRegistryMagic);
    }
    reader.read_u32("registry protocol")
}

/// Reads a bounded supported block registry for one explicit wire protocol.
pub fn read_registry_for_protocol(
    bytes: &[u8],
    expected_protocol: u32,
) -> Result<Box<[RegistryRecord]>, AssetError> {
    if expected_protocol != LEGACY_REGISTRY_PROTOCOL
        && expected_protocol != crate::active_content_registry_protocol()
    {
        return Err(AssetError::InvalidRegistryMagic);
    }
    let mut reader = Reader::new(bytes);
    let magic = reader.read_exact(REGISTRY_MAGIC.len(), "registry magic")?;
    let previous =
        magic == PREVIOUS_REGISTRY_MAGIC && expected_protocol == LEGACY_REGISTRY_PROTOCOL;
    if magic != REGISTRY_MAGIC && !previous {
        return Err(AssetError::InvalidRegistryMagic);
    }
    let metadata = read_registry_metadata(&mut reader)?;
    if metadata.protocol != expected_protocol {
        return Err(AssetError::InvalidRegistryMagic);
    }
    let name_count = metadata.canonical_names as usize;
    let count = metadata.canonical_states as usize;
    let valentine_names = metadata.valentine_names as usize;
    let valentine_states = metadata.valentine_states as usize;
    let gap_names = metadata.valentine_gap_names as usize;
    let gap_states = metadata.valentine_gap_states as usize;
    if count > MAX_REGISTRY_RECORDS {
        return Err(AssetError::TooManyRegistryRecords {
            count,
            max: MAX_REGISTRY_RECORDS,
        });
    }
    if name_count > count
        || valentine_names.checked_add(gap_names) != Some(name_count)
        || valentine_states.checked_add(gap_states) != Some(count)
    {
        return Err(AssetError::InvalidRegistryFlags(0xff));
    }
    let minimum_bytes = count
        .checked_mul(if previous {
            PREVIOUS_RECORD_HEADER_BYTES
        } else {
            RECORD_HEADER_BYTES
        })
        .ok_or(AssetError::TooManyRegistryRecords {
            count,
            max: MAX_REGISTRY_RECORDS,
        })?;
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
    let mut names = HashSet::with_capacity(name_count);
    let mut valentine_name_set = HashSet::with_capacity(valentine_names);
    let mut valentine_overlap = 0usize;
    for _ in 0..count {
        let RecordHeader {
            sequential_id,
            network_hash,
            raw_flags,
            model_family,
            contributor_role,
            model_mask,
            face_coverage,
            confidence,
            raw_provenance,
            box_count,
            shape_id,
            name_len,
            state_len,
            values,
            beacon_tint,
            raw_traits,
            survival,
        } = read_record_header(&mut reader, previous)?;
        let traits =
            BlockTraits::from_bits(raw_traits).ok_or(AssetError::InvalidRegistryFlags(0xff))?;
        let survival = SurvivalRule::read(survival)?;
        let beacon_tint = BeaconTint::read(beacon_tint)?;
        let model_family = ModelFamily::read(model_family)?;
        let contributor_role = ContributorRole::read(contributor_role)?;
        let confidence = CollisionConfidence::read(confidence)?;
        let box_count = usize::from(box_count);
        let name_len = usize::from(name_len);
        let state_len = state_len as usize;

        if !sequential_ids.insert(sequential_id) {
            return Err(AssetError::DuplicateSequentialId(sequential_id));
        }
        if !network_hashes.insert(network_hash) {
            return Err(AssetError::DuplicateNetworkHash(network_hash));
        }
        let flags =
            BlockFlags::from_bits(raw_flags).ok_or(AssetError::InvalidRegistryFlags(raw_flags))?;
        if !flags.has_valid_semantics() {
            return Err(AssetError::InvalidRegistryFlags(raw_flags));
        }
        if values
            .iter()
            .enumerate()
            .any(|(index, value)| model_mask & (1 << index) == 0 && *value != 0)
            || face_coverage & !0x3f != 0
            || box_count > MAX_COLLISION_BOXES
        {
            return Err(AssetError::InvalidRegistryFlags(model_mask));
        }
        let provenance = RegistryProvenance::from_bits(raw_provenance)
            .filter(|source| !source.is_empty())
            .ok_or(AssetError::InvalidRegistryFlags(raw_provenance))?;
        if provenance.contains(RegistryProvenance::VALENTINE) {
            valentine_overlap += 1;
        }
        if confidence == CollisionConfidence::None && (shape_id != 0 || box_count != 0) {
            return Err(AssetError::InvalidRegistryFlags(confidence as u8));
        }
        if state_len > MAX_REGISTRY_STATE_BYTES {
            return Err(AssetError::RegistryStateTooLarge {
                size: state_len,
                max: MAX_REGISTRY_STATE_BYTES,
            });
        }
        let mut boxes = Vec::with_capacity(box_count);
        for _ in 0..box_count {
            let collision_box = read_collision_box(&mut reader)?;
            if collision_box.min_x > collision_box.max_x
                || collision_box.min_y > collision_box.max_y
                || collision_box.min_z > collision_box.max_z
            {
                return Err(AssetError::InvalidRegistryFlags(confidence as u8));
            }
            boxes.push(collision_box);
        }
        let name: Box<str> = str::from_utf8(reader.read_exact(name_len, "record name")?)
            .map_err(|source| AssetError::InvalidRegistryUtf8 {
                field: "name",
                source,
            })?
            .into();
        if provenance.contains(RegistryProvenance::VALENTINE) {
            valentine_name_set.insert(name.clone());
        }
        names.insert(name.clone());
        let canonical_state: Box<str> =
            str::from_utf8(reader.read_exact(state_len, "record state")?)
                .map_err(|source| AssetError::InvalidRegistryUtf8 {
                    field: "canonical state",
                    source,
                })?
                .into();
        let properties = facts::decode(&canonical_state)?;
        let model_family = family_routing::resolve(
            metadata.protocol == LEGACY_REGISTRY_PROTOCOL,
            &name,
            model_family,
        )?;
        let mut record = RegistryRecord {
            sequential_id,
            network_hash,
            name,
            canonical_state,
            flags,
            model_family,
            contributor_role,
            model_state: ModelState {
                mask: model_mask,
                values,
            },
            face_coverage,
            collision_seed: CollisionSeed {
                shape_id,
                confidence,
                boxes: boxes.into_boxed_slice(),
            },
            provenance,
            beacon_tint,
            traits,
            survival,
            properties,
        };
        if previous {
            facts::rules::restore_historical(&mut record).map_err(|error| {
                AssetError::InvalidCompiledAssets {
                    detail: format!("invalid historical block facts: {error}").into(),
                }
            })?;
        }
        records.push(record);
    }
    if names.len() != name_count
        || valentine_overlap != valentine_states
        || valentine_name_set.len() != valentine_names
    {
        return Err(AssetError::InvalidRegistryFlags(0xff));
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
            self.read_exact(2, context)?.try_into().expect("two bytes"),
        ))
    }

    fn read_u32(&mut self, context: &'static str) -> Result<u32, AssetError> {
        Ok(u32::from_le_bytes(
            self.read_exact(4, context)?.try_into().expect("four bytes"),
        ))
    }

    fn read_i32(&mut self, context: &'static str) -> Result<i32, AssetError> {
        Ok(i32::from_le_bytes(
            self.read_exact(4, context)?.try_into().expect("four bytes"),
        ))
    }

    /// Reads a bounded little-endian trait mask.
    fn read_u64(&mut self, context: &'static str) -> Result<u64, AssetError> {
        Ok(u64::from_le_bytes(
            self.read_exact(8, context)?
                .try_into()
                .expect("eight bytes"),
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

#[cfg(test)]
mod model_family_tests {
    use super::ModelFamily;

    #[test]
    fn reads_dedicated_chiseled_bookshelf_family_value() {
        assert_eq!(
            ModelFamily::read(35).expect("family value 35"),
            ModelFamily::ChiseledBookshelf
        );
    }

    #[test]
    fn reads_dedicated_resin_clump_family_value() {
        assert_eq!(
            ModelFamily::read(36).expect("family value 36"),
            ModelFamily::ResinClump
        );
    }
}
