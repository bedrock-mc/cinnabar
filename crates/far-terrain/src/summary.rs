//! Per-column surface summaries and their compact, versioned encoding.

use std::{
    io::{Read, Write},
    sync::Arc,
};

use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};

/// Cells along one side of a column.
pub const COLUMN_SIDE: usize = 16;
/// Cells in one column summary, indexed `z * 16 + x`.
pub const COLUMN_CELLS: usize = COLUMN_SIDE * COLUMN_SIDE;

const FORMAT_VERSION: u8 = 1;
const PLANES: usize = 6;
const RAW_BYTES: usize = COLUMN_CELLS * PLANES;

/// The visible top of one block column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SurfaceCell {
    /// Y of the top face of the highest visible block, or [`SurfaceCell::EMPTY_HEIGHT`].
    pub height: i16,
    /// sRGB colour as seen from above, with any water already blended over its floor.
    pub rgb: [u8; 3],
    /// Blocks of water above the floor; zero for dry land.
    pub water_depth: u8,
}

impl SurfaceCell {
    /// Height of a column with no visible block.
    pub const EMPTY_HEIGHT: i16 = i16::MIN;
    pub const EMPTY: Self = Self {
        height: Self::EMPTY_HEIGHT,
        rgb: [0; 3],
        water_depth: 0,
    };

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.height == Self::EMPTY_HEIGHT
    }
}

/// Surface cells of one 16×16 column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSummary {
    cells: Box<[SurfaceCell; COLUMN_CELLS]>,
}

impl Default for ColumnSummary {
    fn default() -> Self {
        Self {
            cells: Box::new([SurfaceCell::EMPTY; COLUMN_CELLS]),
        }
    }
}

/// A summary's compressed bytes plus the hash of its uncompressed content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedSummary {
    pub hash: u64,
    pub bytes: Arc<[u8]>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SummaryDecodeError {
    #[error("unknown far-terrain summary format {0}")]
    UnknownFormat(u8),
    #[error("far-terrain summary is truncated or oversized")]
    Length,
}

impl ColumnSummary {
    #[must_use]
    pub fn cells(&self) -> &[SurfaceCell; COLUMN_CELLS] {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut [SurfaceCell; COLUMN_CELLS] {
        &mut self.cells
    }

    /// The cell at local `x`, `z`, each below 16.
    #[must_use]
    pub fn cell(&self, x: usize, z: usize) -> SurfaceCell {
        self.cells[z * COLUMN_SIDE + x]
    }

    /// True when no cell has a visible block.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|cell| cell.is_empty())
    }

    /// Stable content hash, equal across sessions and builds for equal cells.
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        hash_planes(&self.planes())
    }

    /// Compresses the summary; decoding the bytes returns an equal summary.
    #[must_use]
    pub fn encode(&self) -> EncodedSummary {
        let planes = self.planes();
        let mut encoder = DeflateEncoder::new(vec![FORMAT_VERSION], Compression::fast());
        encoder
            .write_all(&planes)
            .expect("writing to a Vec cannot fail");
        let bytes = encoder.finish().expect("writing to a Vec cannot fail");
        EncodedSummary {
            hash: hash_planes(&planes),
            bytes: bytes.into(),
        }
    }

    /// Decodes bytes from [`ColumnSummary::encode`], rejecting other formats and lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, SummaryDecodeError> {
        let (&version, payload) = bytes.split_first().ok_or(SummaryDecodeError::Length)?;
        if version != FORMAT_VERSION {
            return Err(SummaryDecodeError::UnknownFormat(version));
        }
        let mut planes = Vec::with_capacity(RAW_BYTES);
        DeflateDecoder::new(payload)
            .take(RAW_BYTES as u64 + 1)
            .read_to_end(&mut planes)
            .map_err(|_| SummaryDecodeError::Length)?;
        let planes: &[u8; RAW_BYTES] = planes
            .as_slice()
            .try_into()
            .map_err(|_| SummaryDecodeError::Length)?;
        Ok(Self::from_planes(planes))
    }

    /// Byte planes with zigzag height deltas, which deflate far better than interleaved cells.
    fn planes(&self) -> [u8; RAW_BYTES] {
        let mut planes = [0; RAW_BYTES];
        let mut previous = 0_i16;
        for (index, cell) in self.cells.iter().enumerate() {
            let delta = cell.height.wrapping_sub(previous);
            previous = cell.height;
            let zigzag = ((delta << 1) ^ (delta >> 15)) as u16;
            planes[index] = zigzag as u8;
            planes[COLUMN_CELLS + index] = (zigzag >> 8) as u8;
            for channel in 0..3 {
                planes[(2 + channel) * COLUMN_CELLS + index] = cell.rgb[channel];
            }
            planes[5 * COLUMN_CELLS + index] = cell.water_depth;
        }
        planes
    }

    fn from_planes(planes: &[u8; RAW_BYTES]) -> Self {
        let mut summary = Self::default();
        let mut previous = 0_i16;
        for (index, cell) in summary.cells.iter_mut().enumerate() {
            let zigzag = u16::from(planes[index]) | (u16::from(planes[COLUMN_CELLS + index]) << 8);
            let delta = ((zigzag >> 1) as i16) ^ -((zigzag & 1) as i16);
            previous = previous.wrapping_add(delta);
            *cell = SurfaceCell {
                height: previous,
                rgb: std::array::from_fn(|channel| planes[(2 + channel) * COLUMN_CELLS + index]),
                water_depth: planes[5 * COLUMN_CELLS + index],
            };
        }
        summary
    }
}

fn hash_planes(planes: &[u8; RAW_BYTES]) -> u64 {
    xxhash_rust::xxh64::xxh64(planes, u64::from(FORMAT_VERSION))
}
