use crate::{
    BlockUpdate, PalettedStorage,
    palette::{BLOCKS_PER_SUB_CHUNK, PACKED_BITS, word_count},
};

/// Client limit for block layers addressed by block updates.
pub const MAX_STORAGE_COUNT: usize = 16;

/// At most one distinct value can be used by each block position.
pub const MAX_PALETTE_ENTRIES: usize = BLOCKS_PER_SUB_CHUNK;

/// Block storage layers vanilla reads from a v8+ sub-chunk; extra layers stay unread.
const MAX_WIRE_STORAGES: usize = 2;

/// Versions 0 and 2–7 carry 4096 legacy id bytes plus 2048 data nibble bytes.
const LEGACY_PAYLOAD_BYTES: usize = BLOCKS_PER_SUB_CHUNK + BLOCKS_PER_SUB_CHUNK / 2;

/// Resolves raw network block ids the way the vanilla palette does.
pub trait BlockIds {
    /// Network id of air in the session's id mode.
    fn air(&self) -> u32;

    /// Returns the id itself when the registry knows it, otherwise air.
    fn resolve(&self, network_id: u32) -> u32;

    /// Resolves a persistent name/state entry; unavailable registries use air.
    fn resolve_persistent(&self, _entry: &crate::NbtCompound) -> u32 {
        self.air()
    }
}

/// Keeps every network id; for decoders whose callers own id resolution.
#[derive(Debug, Clone, Copy)]
pub struct RawBlockIds {
    pub air: u32,
}

impl BlockIds for RawBlockIds {
    fn air(&self) -> u32 {
        self.air
    }

    fn resolve(&self, network_id: u32) -> u32 {
        network_id
    }
}

/// A decoded 16×16×16 Bedrock sub-chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubChunk {
    version: u8,
    y_index: Option<i8>,
    storages: Box<[PalettedStorage]>,
}

impl SubChunk {
    /// Decodes one network sub-chunk, ignoring any bytes vanilla would leave unread.
    pub fn decode(bytes: &[u8], ids: &dyn BlockIds) -> Self {
        Self::decode_prefix(bytes, ids).0
    }

    /// Decodes one network sub-chunk and reports the bytes vanilla consumes.
    ///
    /// Like vanilla, malformed input never fails: reads past the end yield zero.
    pub fn decode_prefix(bytes: &[u8], ids: &dyn BlockIds) -> (Self, usize) {
        let mut reader = Reader::new(bytes);
        let sub_chunk = Self::read(&mut reader, ids);
        (sub_chunk, reader.position())
    }

    pub(crate) fn read(reader: &mut Reader<'_>, ids: &dyn BlockIds) -> Self {
        let version = reader.read_u8();
        if version == 0 || (2..=7).contains(&version) {
            // Provisional: vanilla converts legacy ids through its own table.
            reader.read_exact(LEGACY_PAYLOAD_BYTES);
            return Self {
                version,
                y_index: None,
                storages: Box::new([]),
            };
        }
        let storage_count = if version > 7 {
            usize::from(reader.read_u8()).min(MAX_WIRE_STORAGES)
        } else {
            1
        };
        let y_index = (version > 8).then(|| reader.read_u8() as i8);
        let storages = (0..storage_count)
            .map(|_| {
                if version > 9 {
                    PalettedStorage::uniform(ids.air())
                } else {
                    read_block_storage(reader, ids)
                }
            })
            .collect();
        Self {
            version,
            y_index,
            storages,
        }
    }

    /// Wire-format version byte.
    #[must_use]
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Absolute sub-chunk Y embedded by version 9+, or `None` for earlier versions.
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
            match layer_updates.as_slice() {
                [] => {}
                &[(linear, runtime_id)] => {
                    storages[layer].apply_runtime_update(linear, runtime_id);
                }
                updates => {
                    storages[layer].apply_runtime_updates(updates);
                }
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

fn read_block_storage(reader: &mut Reader<'_>, ids: &dyn BlockIds) -> PalettedStorage {
    let header = reader.read_u8();
    let persistent = header & 1 == 0;
    let entry = |reader: &mut Reader<'_>| {
        if persistent {
            let input = reader.remaining();
            let consumed = crate::block_entity::lenient_nbt_len(input);
            reader.read_exact(consumed);
            crate::BlockEntityNbt::decode_prefix(&input[..consumed])
                .ok()
                .and_then(|(nbt, _)| nbt.parse())
                .map_or_else(|| ids.air(), |entry| ids.resolve_persistent(&entry))
        } else {
            ids.resolve(reader.read_var_i32() as u32)
        }
    };
    match header >> 1 {
        0 | 0x7f => PalettedStorage::uniform(entry(reader)),
        bits if PACKED_BITS.contains(&bits) => read_packed_storage(reader, bits, entry),
        // Vanilla leaves a null layer, which reads back as air.
        _ => PalettedStorage::uniform(ids.air()),
    }
}

/// Reads packed words and a palette clamped to the storage's capacity, then
/// rewrites indices past the palette to entry zero, as vanilla does.
pub(crate) fn read_packed_storage(
    reader: &mut Reader<'_>,
    bits_per_index: u8,
    mut entry: impl FnMut(&mut Reader<'_>) -> u32,
) -> PalettedStorage {
    let word_count = word_count(bits_per_index);
    let words = reader.read_exact(word_count * 4).map_or_else(
        || vec![0; word_count],
        |bytes| {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect()
        },
    );
    let max_palette_len = (1_usize << bits_per_index).min(MAX_PALETTE_ENTRIES);
    let palette_len = usize::try_from(reader.read_var_i32())
        .unwrap_or(0)
        .clamp(1, max_palette_len);
    let palette = (0..palette_len).map(|_| entry(reader)).collect();
    let mut storage = PalettedStorage::new(bits_per_index, words, palette);
    storage.zero_indices_at_or_above(palette_len);
    storage
}

/// Byte reader with vanilla's stream semantics: a read past the end yields
/// zero and parks the cursor at the end.
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

    pub(crate) fn is_at_end(&self) -> bool {
        self.position >= self.bytes.len()
    }

    pub(crate) fn skip_to_end(&mut self) {
        self.position = self.bytes.len();
    }

    pub(crate) fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.position..]
    }

    pub(crate) fn read_u8(&mut self) -> u8 {
        let Some(&byte) = self.bytes.get(self.position) else {
            return 0;
        };
        self.position += 1;
        byte
    }

    /// Returns `None` and parks at the end when fewer than `count` bytes remain.
    pub(crate) fn read_exact(&mut self, count: usize) -> Option<&'a [u8]> {
        let Some(bytes) = self.remaining().get(..count) else {
            self.skip_to_end();
            return None;
        };
        self.position += count;
        Some(bytes)
    }

    /// Zigzag VarInt; bits past 32 are dropped and an unterminated fifth byte yields zero.
    pub(crate) fn read_var_i32(&mut self) -> i32 {
        let mut encoded = 0_u32;
        for index in 0..5 {
            if self.is_at_end() {
                return 0;
            }
            let byte = self.read_u8();
            encoded |= u32::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                return ((encoded >> 1) as i32) ^ -((encoded & 1) as i32);
            }
        }
        0
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
