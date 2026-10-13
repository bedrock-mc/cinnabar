//! When a store was last opened and when each of its region files was last read or
//! written, at minute resolution, from times the caller supplies. The last open has a tiny
//! file of its own, which whole-store expiry and quota ranking read without the access
//! table; far-region eviction reads the table.

use std::collections::HashMap;

use foldhash::fast::FixedState;

use crate::{
    epochs::{EpochId, EpochRef},
    identity::Dimension,
    minute::Minute,
    region_id::RegionId,
    table::{BodyReader, TableError, seal, unseal},
};

/// Leading bytes of the access file.
pub const ACCESS_MAGIC: [u8; 4] = *b"CLAT";
/// Version of the access layout.
pub const ACCESS_FORMAT_VERSION: u16 = 1;
/// Name of the access file inside a store directory.
pub const ACCESS_FILE: &str = "access.clat";
/// Largest access file read: far more region entries than a store cap allows.
pub const MAX_ACCESS_BYTES: usize = 16 * 1024 * 1024;
/// Leading bytes of the last-open file.
pub const OPENED_MAGIC: [u8; 4] = *b"CLOP";
/// Version of the last-open layout.
pub const OPENED_FORMAT_VERSION: u16 = 1;
/// Name of the last-open file inside a store directory.
pub const OPENED_FILE: &str = "opened.clop";
/// Largest last-open file read.
pub const MAX_OPENED_BYTES: usize = 64;

const ENTRY_LEN: usize = 21;

/// One region file of one epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionRef {
    pub epoch: EpochRef,
    pub region: RegionId,
}

/// Per-region access minutes of one store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessTable {
    regions: HashMap<RegionRef, Minute, FixedState>,
}

impl AccessTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// When `region` was last used; [`Minute::EPOCH`] when never recorded.
    #[must_use]
    pub fn region(&self, region: RegionRef) -> Minute {
        self.regions.get(&region).copied().unwrap_or(Minute::EPOCH)
    }

    /// Records a use of `region` at `now`; true when the stored minute changed.
    pub fn touch(&mut self, region: RegionRef, now: Minute) -> bool {
        let minute = self.regions.entry(region).or_insert(Minute::EPOCH);
        let changed = now > *minute;
        *minute = (*minute).max(now);
        changed
    }

    /// Forgets `region`.
    pub fn forget(&mut self, region: RegionRef) {
        self.regions.remove(&region);
    }

    /// Forgets every region of `epoch`.
    pub fn forget_epoch(&mut self, epoch: EpochRef) {
        self.regions.retain(|region, _| region.epoch != epoch);
    }

    /// Moves every region minute of `from` onto the same region of `into`, keeping the
    /// later minute where both exist.
    pub fn merge_epoch(&mut self, from: EpochRef, into: EpochRef) {
        let moved: Vec<_> = self
            .regions
            .iter()
            .filter(|(region, _)| region.epoch == from)
            .map(|(region, &minute)| (region.region, minute))
            .collect();
        self.forget_epoch(from);
        for (region, minute) in moved {
            self.touch(
                RegionRef {
                    epoch: into,
                    region,
                },
                minute,
            );
        }
    }

    /// The sealed table: entry count, then each entry's dimension, epoch, level, region x
    /// and z, and minute, little-endian.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut entries: Vec<_> = self.regions.iter().collect();
        entries.sort_unstable();
        let mut body = Vec::with_capacity(4 + entries.len() * ENTRY_LEN);
        body.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (region, minute) in entries {
            body.extend_from_slice(&region.epoch.dimension.0.to_le_bytes());
            body.extend_from_slice(&region.epoch.id.0.to_le_bytes());
            body.push(region.region.level());
            body.extend_from_slice(&region.region.x().to_le_bytes());
            body.extend_from_slice(&region.region.z().to_le_bytes());
            body.extend_from_slice(&minute.get().to_le_bytes());
        }
        seal(ACCESS_MAGIC, ACCESS_FORMAT_VERSION, &body)
    }

    /// Reads [`Self::encode`] output, rejecting bad framing and invalid levels.
    pub fn decode(bytes: &[u8]) -> Result<Self, TableError> {
        let mut reader = BodyReader::new(unseal(ACCESS_MAGIC, ACCESS_FORMAT_VERSION, bytes)?);
        let mut table = Self::new();
        let count = reader.count(ENTRY_LEN)?;
        table.regions.reserve(count);
        for _ in 0..count {
            let epoch = EpochRef {
                dimension: Dimension(reader.i32()?),
                id: EpochId(reader.u32()?),
            };
            let level = reader.u8()?;
            let (x, z) = (reader.i32()?, reader.i32()?);
            let region =
                RegionId::new(level, x, z).ok_or(TableError::Body("invalid region level"))?;
            table
                .regions
                .insert(RegionRef { epoch, region }, Minute::new(reader.u32()?));
        }
        reader.finish()?;
        Ok(table)
    }
}

/// The sealed last-open file: the minute, little-endian.
#[must_use]
pub fn encode_opened(opened: Minute) -> Vec<u8> {
    seal(
        OPENED_MAGIC,
        OPENED_FORMAT_VERSION,
        &opened.get().to_le_bytes(),
    )
}

/// Reads [`encode_opened`] output.
pub fn decode_opened(bytes: &[u8]) -> Result<Minute, TableError> {
    let mut reader = BodyReader::new(unseal(OPENED_MAGIC, OPENED_FORMAT_VERSION, bytes)?);
    let opened = Minute::new(reader.u32()?);
    reader.finish()?;
    Ok(opened)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(epoch: u32, x: i32) -> RegionRef {
        RegionRef {
            epoch: EpochRef {
                dimension: Dimension(1),
                id: EpochId(epoch),
            },
            region: RegionId::new(2, x, -x).expect("region"),
        }
    }

    #[test]
    fn tables_round_trip_and_keep_the_latest_minutes() {
        let mut table = AccessTable::new();
        assert!(table.touch(region(0, 1), Minute::new(12)));
        assert!(
            !table.touch(region(0, 1), Minute::new(11)),
            "never moves back"
        );
        table.touch(region(1, 2), Minute::new(30));
        let encoded = table.encode();
        assert_eq!(AccessTable::decode(&encoded), Ok(table.clone()));
        assert_eq!(
            decode_opened(&encode_opened(Minute::new(40))),
            Ok(Minute::new(40))
        );
        assert!(decode_opened(&encoded).is_err(), "another table's magic");
        assert_eq!(table.region(region(0, 1)), Minute::new(12));
        assert_eq!(table.region(region(0, 9)), Minute::EPOCH);
    }

    #[test]
    fn merging_an_epoch_moves_its_regions() {
        let mut table = AccessTable::new();
        table.touch(region(0, 1), Minute::new(50));
        table.touch(region(1, 1), Minute::new(20));
        table.touch(region(1, 2), Minute::new(70));
        table.merge_epoch(region(1, 0).epoch, region(0, 0).epoch);
        assert_eq!(table.region(region(0, 1)), Minute::new(50));
        assert_eq!(table.region(region(0, 2)), Minute::new(70));
        assert_eq!(table.region(region(1, 2)), Minute::EPOCH);
    }

    #[test]
    fn malformed_tables_are_rejected() {
        let mut table = AccessTable::new();
        table.touch(region(0, 1), Minute::new(5));
        let mut bytes = table.encode();
        // Level byte of the only entry: 6 preamble, 4 count, then dimension and epoch.
        bytes[6 + 4 + 8] = 9;
        let crc_at = bytes.len() - 4;
        let crc = crate::frame::crc32(&bytes[..crc_at]);
        bytes[crc_at..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(
            AccessTable::decode(&bytes),
            Err(TableError::Body("invalid region level"))
        );
    }
}
