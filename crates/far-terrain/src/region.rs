//! Regions of 32×32 columns, the unit of disk reads, writes and eviction.

use crate::{ColumnPos, EncodedSummary, Versioned};

/// Columns along one side of a region.
pub const REGION_SIDE: i32 = 32;
const REGION_COLUMNS: usize = (REGION_SIDE * REGION_SIDE) as usize;
const MAGIC: &[u8; 8] = b"CNBRLOD3";
/// Slot, payload length, content hash and version.
const ENTRY_BYTES: usize = 2 + 4 + 8 + 8;
const CHECKSUM_BYTES: usize = 8;
/// Maximum canonical region size, including its table and integrity trailer.
pub(crate) const MAX_REGION_BYTES: usize = MAGIC.len()
    + 2
    + REGION_COLUMNS * (ENTRY_BYTES + crate::summary::MAX_ENCODED_BYTES)
    + CHECKSUM_BYTES;
/// Persisted clocks reserve half their range for recovery before arithmetic can overflow.
pub(crate) const MAX_VERSION: u64 = u64::MAX / 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionPos {
    pub x: i32,
    pub z: i32,
}

impl RegionPos {
    #[must_use]
    pub const fn of(column: ColumnPos) -> Self {
        Self {
            x: column.x.div_euclid(REGION_SIDE),
            z: column.z.div_euclid(REGION_SIDE),
        }
    }

    /// The column at `index` within this region.
    #[must_use]
    pub const fn column(self, index: u16) -> ColumnPos {
        ColumnPos::new(
            self.x * REGION_SIDE + index as i32 % REGION_SIDE,
            self.z * REGION_SIDE + index as i32 / REGION_SIDE,
        )
    }

    /// File name inside a namespace directory.
    #[must_use]
    pub fn file_name(self) -> String {
        format!("r.{}.{}.lod", self.x, self.z)
    }

    /// Parses [`RegionPos::file_name`] output.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let rest = name.strip_prefix("r.")?.strip_suffix(".lod")?;
        let (x, z) = rest.split_once('.')?;
        Some(Self {
            x: x.parse().ok()?,
            z: z.parse().ok()?,
        })
    }
}

/// `column`'s slot within its region.
#[must_use]
pub const fn region_index(column: ColumnPos) -> u16 {
    (column.z.rem_euclid(REGION_SIDE) * REGION_SIDE + column.x.rem_euclid(REGION_SIDE)) as u16
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("far-terrain region file is malformed")]
pub struct RegionDecodeError;

/// Serialises summaries keyed by region slot, keeping the highest version of each slot.
#[must_use]
pub fn encode_region(columns: &[(u16, Versioned)]) -> Vec<u8> {
    let mut latest = std::collections::BTreeMap::<u16, &Versioned>::new();
    for (index, column) in columns {
        let newest = latest.entry(*index).or_insert(column);
        if column.version >= newest.version {
            *newest = column;
        }
    }
    let payload: usize = latest
        .values()
        .map(|column| column.summary.bytes.len())
        .sum();
    let mut bytes = Vec::with_capacity(MAGIC.len() + 2 + latest.len() * ENTRY_BYTES + payload);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(latest.len() as u16).to_le_bytes());
    for (index, column) in &latest {
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&(column.summary.bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&column.summary.hash.to_le_bytes());
        bytes.extend_from_slice(&column.version.to_le_bytes());
    }
    for column in latest.values() {
        bytes.extend_from_slice(&column.summary.bytes);
    }
    let checksum = xxhash_rust::xxh64::xxh64(&bytes, 0);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes
}

/// Reads [`encode_region`] output, rejecting any inconsistency rather than reading part.
pub fn decode_region(bytes: &[u8]) -> Result<Vec<(u16, Versioned)>, RegionDecodeError> {
    if bytes.len() > MAX_REGION_BYTES || bytes.len() < MAGIC.len() + 2 + CHECKSUM_BYTES {
        return Err(RegionDecodeError);
    }
    let (bytes, trailer) = bytes.split_at(bytes.len() - CHECKSUM_BYTES);
    if xxhash_rust::xxh64::xxh64(bytes, 0).to_le_bytes() != trailer {
        return Err(RegionDecodeError);
    }
    let rest = bytes.strip_prefix(MAGIC).ok_or(RegionDecodeError)?;
    let (count, mut rest) = rest.split_first_chunk::<2>().ok_or(RegionDecodeError)?;
    let count = usize::from(u16::from_le_bytes(*count));
    if count > REGION_COLUMNS || rest.len() < count * ENTRY_BYTES {
        return Err(RegionDecodeError);
    }
    let (table, mut payload) = rest.split_at(count * ENTRY_BYTES);
    rest = table;
    let mut columns = Vec::with_capacity(count);
    for entry in rest.as_chunks::<ENTRY_BYTES>().0 {
        let index = u16::from_le_bytes([entry[0], entry[1]]);
        let len = u32::from_le_bytes(entry[2..6].try_into().expect("four bytes")) as usize;
        let hash = u64::from_le_bytes(entry[6..14].try_into().expect("eight bytes"));
        let version = u64::from_le_bytes(entry[14..22].try_into().expect("eight bytes"));
        if usize::from(index) >= REGION_COLUMNS
            || len > payload.len()
            || len > crate::summary::MAX_ENCODED_BYTES
            || version > MAX_VERSION
        {
            return Err(RegionDecodeError);
        }
        let (summary, next) = payload.split_at(len);
        payload = next;
        columns.push((
            index,
            Versioned {
                version,
                summary: EncodedSummary {
                    hash,
                    bytes: summary.into(),
                },
            },
        ));
    }
    if payload.is_empty() {
        Ok(columns)
    } else {
        Err(RegionDecodeError)
    }
}
