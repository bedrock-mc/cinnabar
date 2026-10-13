use std::{collections::BTreeMap, sync::Arc};

use crate::{
    BiomeIds, BlockIds, ChunkKey, DecodedBiomeColumn, DecodedBlockEntities, SubChunk,
    sub_chunk::Reader,
};

/// Vanilla indexes inline sub-chunk slots with the low byte of the read counter.
const SLOT_INDEX_MASK: usize = 0xff;

/// The sub-chunk slots a dimension holds, from its lowest sub-chunk upwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimensionSlots {
    pub base_sub_chunk_y: i32,
    pub count: usize,
}

impl DimensionSlots {
    fn slot_y(self, slot: usize) -> i32 {
        self.base_sub_chunk_y
            .saturating_add(i32::try_from(slot).unwrap_or(i32::MAX))
    }
}

/// Work performed while decoding an inline column; cache hits do not resolve block palettes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InlineDecodeWork {
    pub decoded_sections: usize,
    pub reused_sections: usize,
}

/// A full-column decode produced on a worker and ready for a cheap commit.
///
/// Packed sub-chunks are wrapped in `Arc`s so the value moves to the main
/// thread without copying chunk data.
#[derive(Debug, Clone)]
pub struct DecodedLevelChunk {
    pub(crate) sub_chunks: BTreeMap<i32, Arc<SubChunk>>,
    pub(crate) biomes: Option<DecodedBiomeColumn>,
    pub(crate) block_entities: Option<DecodedBlockEntities>,
    pub(crate) block_bytes_consumed: usize,
    pub(crate) bytes_consumed: usize,
    pub(crate) work: InlineDecodeWork,
}

impl PartialEq for DecodedLevelChunk {
    /// Decode work counters are diagnostics, not part of the decoded column value.
    fn eq(&self, other: &Self) -> bool {
        let Self {
            sub_chunks,
            biomes,
            block_entities,
            block_bytes_consumed,
            bytes_consumed,
            work: _,
        } = self;
        *sub_chunks == other.sub_chunks
            && *biomes == other.biomes
            && *block_entities == other.block_entities
            && *block_bytes_consumed == other.block_bytes_consumed
            && *bytes_consumed == other.bytes_consumed
    }
}
impl Eq for DecodedLevelChunk {}

impl DecodedLevelChunk {
    /// Decodes only the block sub-chunks of an inline payload into `count` slots.
    pub fn decode(
        first_sub_chunk_y: i32,
        sub_chunk_count: usize,
        payload: &[u8],
        ids: &dyn BlockIds,
    ) -> Self {
        let slots = DimensionSlots {
            base_sub_chunk_y: first_sub_chunk_y,
            count: sub_chunk_count,
        };
        let mut reader = Reader::new(payload);
        let (sub_chunks, work) = read_sub_chunks(&mut reader, slots, sub_chunk_count, ids, None);
        Self {
            sub_chunks,
            biomes: None,
            block_entities: None,
            block_bytes_consumed: reader.position(),
            bytes_consumed: reader.position(),
            work,
        }
    }

    /// Decodes an inline LevelChunk the way vanilla reads its one stream:
    /// sub-chunks, biomes, border blocks, then block entities to the end.
    pub fn decode_inline(
        chunk: ChunkKey,
        slots: DimensionSlots,
        sub_chunk_count: usize,
        payload: &[u8],
        blocks: &dyn BlockIds,
        biomes: &dyn BiomeIds,
    ) -> Self {
        Self::decode_inline_reusing(chunk, slots, sub_chunk_count, payload, blocks, biomes, None)
    }

    /// Reuses byte-identical resident sections with the same immutable block-id registry.
    /// Every changed section and the entire biome/actor tail still follow the ordinary decoder.
    pub fn decode_inline_reusing(
        chunk: ChunkKey,
        slots: DimensionSlots,
        sub_chunk_count: usize,
        payload: &[u8],
        blocks: &dyn BlockIds,
        biomes: &dyn BiomeIds,
        previous: Option<&BTreeMap<i32, Arc<SubChunk>>>,
    ) -> Self {
        let mut reader = Reader::new(payload);
        let (sub_chunks, work) =
            read_sub_chunks(&mut reader, slots, sub_chunk_count, blocks, previous);
        let block_bytes_consumed = reader.position();
        let (biome_column, block_entities) =
            decode_column_tail(chunk, slots, reader.remaining(), biomes);
        Self {
            sub_chunks,
            biomes: Some(biome_column),
            block_entities: Some(block_entities),
            block_bytes_consumed,
            bytes_consumed: payload.len(),
            work,
        }
    }

    /// Reports full decoder calls and payload hash hits for this column.
    #[must_use]
    pub const fn decode_work(&self) -> InlineDecodeWork {
        self.work
    }

    #[must_use]
    pub fn bytes_consumed(&self) -> usize {
        self.bytes_consumed
    }

    /// Bytes occupied only by serialized block sub-chunks.
    #[must_use]
    pub fn block_bytes_consumed(&self) -> usize {
        self.block_bytes_consumed
    }

    /// Returns an immutable worker-produced snapshot for one Y index.
    #[must_use]
    pub fn sub_chunk(&self, y: i32) -> Option<Arc<SubChunk>> {
        self.sub_chunks.get(&y).cloned()
    }

    pub fn sub_chunks(&self) -> impl ExactSizeIterator<Item = (i32, Arc<SubChunk>)> + '_ {
        self.sub_chunks
            .iter()
            .map(|(&y, sub_chunk)| (y, Arc::clone(sub_chunk)))
    }
}

/// Decodes the biome, border-block and block-entity data that follows the
/// sub-chunks, which is the whole payload of a request-mode LevelChunk.
pub fn decode_column_tail(
    chunk: ChunkKey,
    slots: DimensionSlots,
    payload: &[u8],
    biomes: &dyn BiomeIds,
) -> (DecodedBiomeColumn, DecodedBlockEntities) {
    let biome_column =
        DecodedBiomeColumn::decode(slots.base_sub_chunk_y, slots.count, payload, biomes);
    let y_range =
        slots.base_sub_chunk_y.saturating_mul(16)..slots.slot_y(slots.count).saturating_mul(16);
    let block_entities = DecodedBlockEntities::decode_level_chunk_tail(
        chunk,
        y_range,
        &payload[biome_column.bytes_consumed()..],
    );
    (biome_column, block_entities)
}

/// Runs vanilla's `sub_chunk_count` reads: each fills slot `index & 0xff` when
/// the dimension has it, and a slot whose Y byte disagrees reads back as empty.
fn read_sub_chunks(
    reader: &mut Reader<'_>,
    slots: DimensionSlots,
    sub_chunk_count: usize,
    ids: &dyn BlockIds,
    previous: Option<&BTreeMap<i32, Arc<SubChunk>>>,
) -> (BTreeMap<i32, Arc<SubChunk>>, InlineDecodeWork) {
    let mut sub_chunks = BTreeMap::new();
    let mut work = InlineDecodeWork::default();
    let context = std::cell::OnceCell::new();
    if slots.count == 0 {
        return (sub_chunks, work);
    }
    for index in 0..sub_chunk_count {
        if reader.is_at_end() {
            // Every remaining read decodes an empty sub-chunk into its slot.
            for offset in 0..(sub_chunk_count - index).min(SLOT_INDEX_MASK + 1) {
                let slot = (index + offset) & SLOT_INDEX_MASK;
                if slot < slots.count {
                    sub_chunks.remove(&slots.slot_y(slot));
                }
            }
            break;
        }
        let slot = index & SLOT_INDEX_MASK;
        if slot >= slots.count {
            continue;
        }
        let y = slots.slot_y(slot);
        if let Some(resident) = previous.and_then(|previous| previous.get(&y))
            && resident
                .y_index()
                .is_none_or(|actual| i32::from(actual) == y)
            && resident.reuse_inline(reader, ids)
        {
            sub_chunks.insert(y, Arc::clone(resident));
            work.reused_sections += 1;
            continue;
        }
        let context = context.get_or_init(|| ids.inline_cache_context());
        let sub_chunk = SubChunk::read_inline(reader, ids, context.as_ref());
        work.decoded_sections += 1;
        let misplaced = sub_chunk
            .y_index()
            .is_some_and(|actual| i32::from(actual) != y);
        if misplaced || sub_chunk.has_no_storages() {
            sub_chunks.remove(&y);
        } else {
            sub_chunks.insert(y, Arc::new(sub_chunk));
        }
    }
    (sub_chunks, work)
}

#[cfg(test)]
mod tests;
