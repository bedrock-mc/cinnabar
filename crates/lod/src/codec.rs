//! The stored section record: a checksummed header and a deflate-compressed body.
//!
//! Header, 26 bytes, little-endian except the key:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 0..4 | [`SECTION_MAGIC`] |
//! | 4..6 | [`SECTION_FORMAT_VERSION`] |
//! | 6..14 | [`SectionKey::to_be_bytes`] |
//! | 14..16 | voxel palette length, 1..=32768 |
//! | 16..18 | [`ChildMask::to_bits`] |
//! | 18..22 | compressed body length |
//! | 22..26 | CRC32 of bytes 0..22 and the compressed body |
//!
//! The body is raw deflate at [`DEFLATE_LEVEL`] of: the palette as `u32` voxel words, the
//! packed `u64` index words ([`crate::storage::packed_word_count`] of the minimal width),
//! the biome palette length as `u16`, the biome palette as `u32` ids and the 1024 biome
//! cells as `u8`. Decoding bounds every read by the header, so malformed records are
//! rejected without large allocations.

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

use crate::{
    biome_grid::{BiomeGrid, MAX_BIOME_PALETTE},
    geometry::{KeyError, SECTION_COLUMNS, SECTION_VOXELS, SectionKey},
    section::{ChildMask, Section},
    storage::{
        MAX_BITS_PER_INDEX, PackedVoxels, StorageError, bits_for_palette_len, packed_word_count,
    },
    voxel::Voxel,
};

/// Leading bytes of every section record.
pub const SECTION_MAGIC: [u8; 4] = *b"CLDS";
/// Version of the record layout.
pub const SECTION_FORMAT_VERSION: u16 = 1;
/// Deflate level of record bodies; level 6 measured the best ratio for its encode cost.
pub const DEFLATE_LEVEL: u32 = 6;
/// Bytes before the compressed body.
pub const HEADER_LEN: usize = 26;
const CHECKED_HEADER_LEN: usize = HEADER_LEN - 4;
/// Largest uncompressed body: a full palette, the widest indices and a full biome palette.
pub const MAX_BODY_LEN: usize = body_len(SECTION_VOXELS, MAX_BIOME_PALETTE);
/// Largest compressed body deflate can emit for [`MAX_BODY_LEN`] input (zlib's
/// `deflateBound` for raw streams).
pub const MAX_COMPRESSED_LEN: usize =
    MAX_BODY_LEN + (MAX_BODY_LEN >> 12) + (MAX_BODY_LEN >> 14) + (MAX_BODY_LEN >> 25) + 7;

const _: () = assert!(bits_for_palette_len(SECTION_VOXELS) == MAX_BITS_PER_INDEX);
const _: () = assert!(SECTION_VOXELS <= u16::MAX as usize);
const _: () = assert!(MAX_COMPRESSED_LEN <= u32::MAX as usize);

/// Uncompressed body length for the given palette sizes.
const fn body_len(palette_len: usize, biome_palette_len: usize) -> usize {
    palette_len * 4
        + packed_word_count(bits_for_palette_len(palette_len)) * 8
        + 2
        + biome_palette_len * 4
        + SECTION_COLUMNS
}

/// Why a record was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    #[error("record has {0} bytes; the header alone needs {HEADER_LEN}")]
    Truncated(usize),
    #[error("record starts with {0:02x?}, not the section magic")]
    Magic([u8; 4]),
    #[error("section format version {0} is not supported")]
    Version(u16),
    #[error("record key is invalid: {0}")]
    Key(#[from] KeyError),
    #[error("record holds {found:?}, not the requested {expected:?}")]
    KeyMismatch {
        expected: SectionKey,
        found: SectionKey,
    },
    #[error("voxel palette length {0} is outside 1..={SECTION_VOXELS}")]
    PaletteLength(u16),
    #[error("level-0 record has child mask {0:#06x}; level 0 has no children")]
    LevelZeroChildren(u16),
    #[error("header declares a {declared}-byte body but {actual} bytes follow")]
    BodyLength { declared: u32, actual: usize },
    #[error("record checksum {stored:#010x} does not match {computed:#010x}")]
    Checksum { stored: u32, computed: u32 },
    #[error("record body is not a single complete deflate stream")]
    Deflate,
    #[error("inflated body has {actual} bytes; the header implies {expected}")]
    BodySize { expected: usize, actual: usize },
    #[error("palette entry {index} holds non-canonical voxel word {bits:#010x}")]
    Voxel { index: usize, bits: u32 },
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// The validated header of a record, readable without inflating its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
    pub key: SectionKey,
    pub palette_len: u16,
    pub children: ChildMask,
    pub body_len: u32,
}

impl RecordHeader {
    /// Validates the header and the checksum over the header and body of a whole record.
    pub fn read(record: &[u8]) -> Result<Self, CodecError> {
        if record.len() < HEADER_LEN {
            return Err(CodecError::Truncated(record.len()));
        }
        let field = |range: std::ops::Range<usize>| &record[range];
        let magic: [u8; 4] = field(0..4).try_into().expect("four bytes");
        if magic != SECTION_MAGIC {
            return Err(CodecError::Magic(magic));
        }
        let version = u16::from_le_bytes(field(4..6).try_into().expect("two bytes"));
        if version != SECTION_FORMAT_VERSION {
            return Err(CodecError::Version(version));
        }
        let body_len = u32::from_le_bytes(field(18..22).try_into().expect("four bytes"));
        let body = &record[HEADER_LEN..];
        if body_len as usize != body.len() || body.len() > MAX_COMPRESSED_LEN {
            return Err(CodecError::BodyLength {
                declared: body_len,
                actual: body.len(),
            });
        }
        let stored = u32::from_le_bytes(field(22..26).try_into().expect("four bytes"));
        let computed = record_crc(&record[..CHECKED_HEADER_LEN], body);
        if stored != computed {
            return Err(CodecError::Checksum { stored, computed });
        }
        let key = SectionKey::from_be_bytes(field(6..14).try_into().expect("eight bytes"))?;
        let palette_len = u16::from_le_bytes(field(14..16).try_into().expect("two bytes"));
        if palette_len == 0 || usize::from(palette_len) > SECTION_VOXELS {
            return Err(CodecError::PaletteLength(palette_len));
        }
        let mask_bits = u16::from_le_bytes(field(16..18).try_into().expect("two bytes"));
        let children = ChildMask::from_bits(mask_bits)?;
        if key.pos().level() == 0 && children != ChildMask::UNKNOWN {
            return Err(CodecError::LevelZeroChildren(mask_bits));
        }
        Ok(Self {
            key,
            palette_len,
            children,
            body_len,
        })
    }
}

/// Encodes and decodes section records, reusing its deflate state and scratch buffers.
pub struct SectionCodec {
    compress: Compress,
    decompress: Decompress,
    body: Vec<u8>,
}

impl Default for SectionCodec {
    fn default() -> Self {
        Self {
            compress: Compress::new(Compression::new(DEFLATE_LEVEL), false),
            decompress: Decompress::new(false),
            body: Vec::new(),
        }
    }
}

impl std::fmt::Debug for SectionCodec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SectionCodec")
            .finish_non_exhaustive()
    }
}

impl SectionCodec {
    /// A codec with fresh deflate state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces `out` with the record of `section`. [`Section`] keeps level-0 masks
    /// unknown, so every encoded record decodes.
    pub fn encode_into(&mut self, section: &Section, out: &mut Vec<u8>) {
        self.body.clear();
        let voxels = section.voxels();
        for voxel in voxels.palette() {
            self.body.extend_from_slice(&voxel.bits().to_le_bytes());
        }
        for word in voxels.words() {
            self.body.extend_from_slice(&word.to_le_bytes());
        }
        let biomes = section.biomes();
        self.body
            .extend_from_slice(&(biomes.palette().len() as u16).to_le_bytes());
        for biome in biomes.palette() {
            self.body.extend_from_slice(&biome.to_le_bytes());
        }
        self.body.extend_from_slice(biomes.cells());

        out.clear();
        out.extend_from_slice(&SECTION_MAGIC);
        out.extend_from_slice(&SECTION_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&section.pos().key().to_be_bytes());
        out.extend_from_slice(&(voxels.palette().len() as u16).to_le_bytes());
        out.extend_from_slice(&section.children().to_bits().to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        self.deflate_body(out);
        let body_len = out.len() - HEADER_LEN;
        out[18..22].copy_from_slice(&(body_len as u32).to_le_bytes());
        let crc = record_crc(&out[..CHECKED_HEADER_LEN], &out[HEADER_LEN..]);
        out[22..26].copy_from_slice(&crc.to_le_bytes());
    }

    /// The record of `section` in a new buffer.
    #[must_use]
    pub fn encode(&mut self, section: &Section) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(section, &mut out);
        out
    }

    /// Decodes a whole record, rejecting any corrupt or non-canonical part.
    pub fn decode(&mut self, record: &[u8]) -> Result<Section, CodecError> {
        let header = RecordHeader::read(record)?;
        self.decode_body(&header, &record[HEADER_LEN..])
    }

    /// Decodes a record that must hold `expected`, as read from a store slot for that key.
    pub fn decode_expecting(
        &mut self,
        record: &[u8],
        expected: SectionKey,
    ) -> Result<Section, CodecError> {
        let header = RecordHeader::read(record)?;
        if header.key != expected {
            return Err(CodecError::KeyMismatch {
                expected,
                found: header.key,
            });
        }
        self.decode_body(&header, &record[HEADER_LEN..])
    }

    /// Appends the raw deflate stream of the body scratch to `out`.
    fn deflate_body(&mut self, out: &mut Vec<u8>) {
        self.compress.reset();
        out.reserve(self.body.len() / 2 + 64);
        loop {
            let consumed = self.compress.total_in() as usize;
            let status = self
                .compress
                .compress_vec(&self.body[consumed..], out, FlushCompress::Finish)
                .expect("deflating an in-memory buffer cannot fail");
            if status == Status::StreamEnd {
                return;
            }
            out.reserve(out.capacity().max(1024));
        }
    }

    /// Inflates and validates a body against its already validated header.
    fn decode_body(
        &mut self,
        header: &RecordHeader,
        compressed: &[u8],
    ) -> Result<Section, CodecError> {
        let palette_len = usize::from(header.palette_len);
        let min_len = body_len(palette_len, 1);
        let max_len = body_len(palette_len, MAX_BIOME_PALETTE);
        self.inflate(compressed, max_len)?;
        let body = &self.body;
        if body.len() < min_len || body.len() > max_len {
            return Err(CodecError::BodySize {
                expected: min_len,
                actual: body.len(),
            });
        }
        let mut at = 0;
        let mut take = |len: usize| {
            let bytes = &body[at..at + len];
            at += len;
            bytes
        };
        let palette = take(palette_len * 4)
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .map(|(index, &bytes)| {
                let bits = u32::from_le_bytes(bytes);
                Voxel::from_bits(bits).ok_or(CodecError::Voxel { index, bits })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bits = bits_for_palette_len(palette_len);
        let words = take(packed_word_count(bits) * 8)
            .as_chunks::<8>()
            .0
            .iter()
            .map(|&bytes| u64::from_le_bytes(bytes))
            .collect();
        let biome_len = usize::from(u16::from_le_bytes(take(2).try_into().expect("two bytes")));
        let expected = body_len(palette_len, biome_len);
        if biome_len == 0 || biome_len > MAX_BIOME_PALETTE || body.len() != expected {
            return Err(CodecError::BodySize {
                expected,
                actual: body.len(),
            });
        }
        let biome_palette = take(biome_len * 4)
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&bytes| u32::from_le_bytes(bytes))
            .collect();
        let cells = take(SECTION_COLUMNS)
            .try_into()
            .expect("one cell per column");
        let voxels = PackedVoxels::from_parts(palette, bits, words)?;
        let biomes = BiomeGrid::from_parts(biome_palette, cells)?;
        Ok(Section::new(
            header.key.pos(),
            voxels,
            biomes,
            header.children,
        )?)
    }

    /// Inflates `compressed` into the body scratch, accepting only one complete stream
    /// that uses every input byte and inflates to at most `limit` bytes.
    fn inflate(&mut self, compressed: &[u8], limit: usize) -> Result<(), CodecError> {
        self.decompress.reset(false);
        self.body.clear();
        // One spare byte tells an over-long body from one that exactly fills the limit.
        self.body.reserve_exact(limit + 1);
        let status = self
            .decompress
            .decompress_vec(compressed, &mut self.body, FlushDecompress::Finish)
            .map_err(|_| CodecError::Deflate)?;
        if self.body.len() > limit {
            return Err(CodecError::BodySize {
                expected: limit,
                actual: self.body.len(),
            });
        }
        if status != Status::StreamEnd || self.decompress.total_in() as usize != compressed.len() {
            return Err(CodecError::Deflate);
        }
        Ok(())
    }
}

/// CRC32 of a record's checked header bytes followed by its body.
fn record_crc(header: &[u8], body: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(header);
    crc.update(body);
    crc.sum()
}

/// CRC-32 (IEEE) of `bytes`, the checksum of every encoded table in this crate.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    record_crc(bytes, &[])
}

#[cfg(test)]
mod tests;
