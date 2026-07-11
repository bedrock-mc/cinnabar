use crate::{
    BlockUpdate, PalettedStorage,
    error::DecodeError,
    palette::{BLOCKS_PER_SUB_CHUNK, SUPPORTED_BITS, word_count},
};

/// Deliberate client safety limit for block storage layers in one sub-chunk.
///
/// The wire count is an unsigned byte, but vanilla uses only a handful of
/// layers. Sixteen retains generous custom-server headroom while preventing a
/// malicious packet from multiplying packed-storage allocations 255 times.
pub const MAX_STORAGE_COUNT: usize = 16;

/// At most one distinct value can be used by each block position.
pub const MAX_PALETTE_ENTRIES: usize = BLOCKS_PER_SUB_CHUNK;

/// A decoded 16×16×16 Bedrock sub-chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubChunk {
    version: u8,
    y_index: Option<i8>,
    storages: Box<[PalettedStorage]>,
}

impl SubChunk {
    /// Decodes one standalone network sub-chunk and requires exact EOF.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (sub_chunk, consumed) = Self::decode_prefix(bytes)?;
        let remaining = bytes.len() - consumed;
        if remaining != 0 {
            return Err(DecodeError::TrailingBytes { remaining });
        }
        Ok(sub_chunk)
    }

    /// Prefix-decodes one network sub-chunk and reports the number of bytes
    /// consumed. Block-entity NBT and other packet data may follow the prefix.
    ///
    /// This function is pure and suitable for a decode worker. Use
    /// [`crate::ChunkStore::commit_sub_chunk`] to apply its result later.
    pub fn decode_prefix(bytes: &[u8]) -> Result<(Self, usize), DecodeError> {
        let mut reader = Reader::new(bytes);
        let version = reader.read_u8("sub-chunk version")?;

        let (storage_count, y_index) = match version {
            1 => (1, None),
            8 => (usize::from(reader.read_u8("storage count")?), None),
            9 => {
                let count = usize::from(reader.read_u8("storage count")?);
                if count > MAX_STORAGE_COUNT {
                    return Err(DecodeError::TooManyStorages {
                        count,
                        max: MAX_STORAGE_COUNT,
                    });
                }
                let index = reader.read_u8("sub-chunk Y index")? as i8;
                (count, Some(index))
            }
            other => return Err(DecodeError::UnsupportedVersion(other)),
        };

        if storage_count > MAX_STORAGE_COUNT {
            return Err(DecodeError::TooManyStorages {
                count: storage_count,
                max: MAX_STORAGE_COUNT,
            });
        }

        let mut storages = Vec::with_capacity(storage_count);
        for _ in 0..storage_count {
            storages.push(decode_storage(&mut reader)?);
        }

        Ok((
            Self {
                version,
                y_index,
                storages: storages.into_boxed_slice(),
            },
            reader.position(),
        ))
    }

    /// Wire-format version (1, 8, or 9).
    #[must_use]
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Absolute sub-chunk Y embedded by version 9, or `None` for v1/v8.
    #[must_use]
    pub fn y_index(&self) -> Option<i8> {
        self.y_index
    }

    /// Packed block layers in this sub-chunk.
    #[must_use]
    pub fn storages(&self) -> &[PalettedStorage] {
        &self.storages
    }

    /// Looks up a raw network runtime value without flattening any storage.
    #[must_use]
    pub fn runtime_id(&self, layer: usize, x: u8, y: u8, z: u8) -> Option<u32> {
        self.storages.get(layer)?.runtime_id(x, y, z)
    }

    /// True for a zero-storage response or a single uniform air-like value.
    ///
    /// The registry decides which value is actually air, so only the
    /// zero-storage case is unconditionally considered all-air here.
    #[must_use]
    pub fn has_no_storages(&self) -> bool {
        self.storages.is_empty()
    }

    pub(crate) fn for_block_updates(y: i32) -> Self {
        let y_index = i8::try_from(y).ok();
        Self {
            // Modern in-range columns retain the v9 Y metadata so a later
            // identical network snapshot can reuse this Arc. The sparse store
            // still supports synthetic i32 Y values through the v8 shape.
            version: if y_index.is_some() { 9 } else { 8 },
            y_index,
            storages: Box::new([]),
        }
    }

    pub(crate) fn apply_block_updates(&mut self, updates: &[BlockUpdate], air_runtime_id: u32) {
        let mut storages = std::mem::take(&mut self.storages).into_vec();
        let mut updates_by_layer: [Vec<(usize, u32)>; MAX_STORAGE_COUNT] =
            std::array::from_fn(|_| Vec::new());
        for update in updates {
            let layer = update.layer as usize;
            while storages.len() <= layer {
                storages.push(PalettedStorage::uniform(air_runtime_id));
            }
            let linear =
                (usize::from(update.x) << 8) | (usize::from(update.z) << 4) | usize::from(update.y);
            updates_by_layer[layer].push((linear, update.runtime_id));
        }
        for (layer, layer_updates) in updates_by_layer.iter().enumerate() {
            if !layer_updates.is_empty() {
                storages[layer].apply_runtime_updates(layer_updates);
            }
        }
        while storages
            .last()
            .is_some_and(|storage| storage.contains_only(air_runtime_id))
        {
            storages.pop();
        }
        self.storages = storages.into_boxed_slice();
    }
}

fn decode_storage(reader: &mut Reader<'_>) -> Result<PalettedStorage, DecodeError> {
    let header = reader.read_u8("palette header")?;
    decode_storage_with_header(reader, header)
}

pub(crate) fn decode_storage_with_header(
    reader: &mut Reader<'_>,
    header: u8,
) -> Result<PalettedStorage, DecodeError> {
    if header & 1 == 0 {
        return Err(DecodeError::DiskPaletteInNetworkData { header });
    }

    let bits_per_index = header >> 1;
    if !SUPPORTED_BITS.contains(&bits_per_index) {
        return Err(DecodeError::UnsupportedBitsPerIndex(bits_per_index));
    }

    let word_count = word_count(bits_per_index);
    let word_bytes = word_count * std::mem::size_of::<u32>();
    let packed = reader.read_exact(word_bytes, "packed index words")?;
    let mut words = Vec::with_capacity(word_count);
    for bytes in packed.chunks_exact(4) {
        words.push(u32::from_le_bytes(
            bytes.try_into().expect("four-byte chunk"),
        ));
    }

    let max_palette_len = if bits_per_index == 0 {
        1
    } else {
        (1_usize << bits_per_index).min(MAX_PALETTE_ENTRIES)
    };
    let palette_len = if bits_per_index == 0 {
        1
    } else {
        let count = reader.read_var_i32("palette length")?;
        if count <= 0 || usize::try_from(count).map_or(true, |count| count > max_palette_len) {
            return Err(DecodeError::InvalidPaletteLength {
                count,
                max: max_palette_len,
            });
        }
        count as usize
    };

    let mut palette = Vec::with_capacity(palette_len);
    for _ in 0..palette_len {
        palette.push(reader.read_var_i32("palette entry")? as u32);
    }

    let storage = PalettedStorage::new(bits_per_index, words, palette);
    for block_index in 0..BLOCKS_PER_SUB_CHUNK {
        let palette_index = storage
            .palette_index(block_index)
            .expect("validated packed storage has every block index");
        if palette_index >= storage.palette().len() {
            return Err(DecodeError::PaletteIndexOutOfBounds {
                block_index,
                palette_index,
                palette_len: storage.palette().len(),
            });
        }
    }
    Ok(storage)
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(crate) fn position(&self) -> usize {
        self.position
    }

    pub(crate) fn read_u8(&mut self, context: &'static str) -> Result<u8, DecodeError> {
        Ok(self.read_exact(1, context)?[0])
    }

    fn read_exact(&mut self, count: usize, context: &'static str) -> Result<&'a [u8], DecodeError> {
        let remaining = self.bytes.len().saturating_sub(self.position);
        if remaining < count {
            return Err(DecodeError::UnexpectedEof {
                context,
                needed: count,
                remaining,
            });
        }
        let start = self.position;
        self.position += count;
        Ok(&self.bytes[start..self.position])
    }

    fn read_var_i32(&mut self, context: &'static str) -> Result<i32, DecodeError> {
        let mut encoded = 0_u32;
        for index in 0..5 {
            let byte = self.read_u8(context)?;
            if index == 4 {
                if byte & 0x80 != 0 {
                    return Err(DecodeError::VarIntTooLong { context });
                }
                if byte & 0xf0 != 0 {
                    return Err(DecodeError::VarIntOverflow { context });
                }
            }
            encoded |= u32::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                let magnitude = (encoded >> 1) as i32;
                return Ok(if encoded & 1 == 0 {
                    magnitude
                } else {
                    !magnitude
                });
            }
        }
        Err(DecodeError::VarIntTooLong { context })
    }
}

#[cfg(test)]
mod tests {
    use super::word_count;

    #[test]
    fn bedrock_word_counts_include_per_word_padding() {
        let actual = [0, 1, 2, 3, 4, 5, 6, 8, 16].map(word_count);
        assert_eq!(actual, [0, 128, 256, 410, 512, 683, 820, 1024, 2048]);
    }
}
