use std::{collections::BTreeMap, ops::Range, sync::Arc};

use thiserror::Error;

use crate::{BlockIds, ChunkKey, SubChunk, SubChunkKey};

/// Maximum encoded size retained for one block entity.
pub const MAX_BLOCK_ENTITY_NBT_BYTES: usize = 1024 * 1024;
/// Maximum nested compound/list depth accepted from untrusted network NBT.
pub const MAX_NBT_DEPTH: usize = 64;
/// Maximum elements accepted in one NBT list or primitive array.
pub const MAX_NBT_COLLECTION_LENGTH: usize = 16_384;
/// Maximum UTF-8 byte length accepted for an NBT name or string value.
pub const MAX_NBT_STRING_BYTES: usize = 64 * 1024;
/// Maximum aggregate tag payload visits in one block entity.
pub const MAX_NBT_TAGS: usize = 16_384;
/// Aggregate block-entity byte budget; see `MAX_BLOCK_ENTITY_BYTES_PER_CHUNK`.
pub const MAX_BLOCK_ENTITY_TAIL_BYTES: usize = 8 * 1024 * 1024;
/// Maximum aggregate exact NBT bytes retained in one sparse chunk column.
pub const MAX_BLOCK_ENTITY_BYTES_PER_CHUNK: usize = MAX_BLOCK_ENTITY_TAIL_BYTES;
/// Maximum sparse records accepted for one complete chunk column.
pub const MAX_BLOCK_ENTITIES_PER_CHUNK: usize = 16_384;
/// Maximum sparse records accepted in one 16³ subchunk.
pub const MAX_BLOCK_ENTITIES_PER_SUB_CHUNK: usize = 4_096;

/// Absolute position of one block entity, including dimension identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockEntityKey {
    pub dimension: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockEntityKey {
    #[must_use]
    pub const fn new(dimension: i32, x: i32, y: i32, z: i32) -> Self {
        Self { dimension, x, y, z }
    }

    #[must_use]
    pub const fn position(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }

    #[must_use]
    pub const fn chunk(self) -> ChunkKey {
        ChunkKey::new(self.dimension, self.x.div_euclid(16), self.z.div_euclid(16))
    }

    #[must_use]
    pub const fn sub_chunk(self) -> SubChunkKey {
        SubChunkKey::new(
            self.dimension,
            self.x.div_euclid(16),
            self.y.div_euclid(16),
            self.z.div_euclid(16),
        )
    }
}

/// Exact validated NetworkLittleEndian NBT retained for one block entity.
#[derive(Debug, Clone)]
pub struct BlockEntityNbt {
    bytes: Arc<[u8]>,
    pub(crate) parsed: Arc<std::sync::OnceLock<Option<Arc<crate::NbtCompound>>>>,
    id: Option<Arc<str>>,
    embedded_position: Option<[i32; 3]>,
    note_candidate: RootByteCandidate,
    powered_candidate: RootByteCandidate,
}

impl PartialEq for BlockEntityNbt {
    /// Compares wire identity; warming a derived cache does not change the source.
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for BlockEntityNbt {}

/// Bounded root-byte metadata retained for id-less Note discrimination.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RootByteCandidate {
    #[default]
    Absent,
    Value(u8),
    Invalid,
}

impl BlockEntityNbt {
    /// Prefix-decodes exactly one named root compound and preserves its exact
    /// encoded bytes. Trailing input belongs to subsequent block entities or
    /// the containing packet and is not consumed.
    pub fn decode_prefix(input: &[u8]) -> Result<(Self, usize), BlockEntityNbtError> {
        let (nbt, consumed, semantic_error) = Self::scan_prefix(input)?;
        if let Some(error) = semantic_error {
            return Err(error);
        }
        Ok((nbt, consumed))
    }

    /// Structurally scans one complete named root value and returns any deferred semantic error.
    fn scan_prefix(
        input: &[u8],
    ) -> Result<(Self, usize, Option<BlockEntityNbtError>), BlockEntityNbtError> {
        let mut reader = Reader::new(input);
        let root = reader.read_u8("root tag")?;
        let _root_name = reader.read_string("root name")?;
        let mut state = ScanState::default();
        state.visit_tag()?;
        if root != 10 {
            let mut semantic_error = Some(BlockEntityNbtError::RootNotCompound { tag: root });
            prefer_wire_or_first_semantic(
                scan_payload(root, &mut reader, &mut state, 0),
                &mut semantic_error,
            )?;
            let consumed = reader.position();
            return Ok((
                Self {
                    bytes: Arc::from(&input[..consumed]),
                    parsed: Arc::default(),
                    id: None,
                    embedded_position: None,
                    note_candidate: RootByteCandidate::Absent,
                    powered_candidate: RootByteCandidate::Absent,
                },
                consumed,
                semantic_error,
            ));
        }
        state.enter_container(0)?;

        let mut id = None;
        let mut position = [None; 3];
        let mut note_candidate = RootByteCandidate::Absent;
        let mut powered_candidate = RootByteCandidate::Absent;
        let mut semantic_error: Option<BlockEntityNbtError> = None;
        loop {
            let tag =
                prefer_wire_or_first_semantic(reader.read_u8("compound tag"), &mut semantic_error)?;
            if tag == 0 {
                break;
            }
            prefer_wire_or_first_semantic(state.visit_tag(), &mut semantic_error)?;
            let name =
                prefer_wire_or_first_semantic(reader.read_string("tag name"), &mut semantic_error)?;
            match name {
                "id" => {
                    if tag != 8 {
                        semantic_error.get_or_insert(BlockEntityNbtError::InvalidRootFieldType {
                            field: "id",
                            expected: 8,
                            actual: tag,
                        });
                        prefer_wire_or_first_semantic(
                            scan_payload(tag, &mut reader, &mut state, 1),
                            &mut semantic_error,
                        )?;
                        continue;
                    }
                    let value = Arc::<str>::from(prefer_wire_or_first_semantic(
                        reader.read_string("id value"),
                        &mut semantic_error,
                    )?);
                    if id.is_some() {
                        semantic_error
                            .get_or_insert(BlockEntityNbtError::DuplicateRootField { field: "id" });
                    } else {
                        id = Some(value);
                    }
                }
                "x" | "y" | "z" => {
                    let (slot, field) = match name {
                        "x" => (0, "x"),
                        "y" => (1, "y"),
                        "z" => (2, "z"),
                        _ => unreachable!(),
                    };
                    if tag != 3 {
                        semantic_error.get_or_insert(BlockEntityNbtError::InvalidRootFieldType {
                            field,
                            expected: 3,
                            actual: tag,
                        });
                        prefer_wire_or_first_semantic(
                            scan_payload(tag, &mut reader, &mut state, 1),
                            &mut semantic_error,
                        )?;
                        continue;
                    }
                    let value = prefer_wire_or_first_semantic(
                        reader.read_zigzag_i32("position"),
                        &mut semantic_error,
                    )?;
                    if position[slot].is_some() {
                        semantic_error
                            .get_or_insert(BlockEntityNbtError::DuplicateRootField { field });
                    } else {
                        position[slot] = Some(value);
                    }
                }
                "note" => prefer_wire_or_first_semantic(
                    scan_root_byte_candidate(&mut note_candidate, tag, &mut reader, &mut state),
                    &mut semantic_error,
                )?,
                "powered" => prefer_wire_or_first_semantic(
                    scan_root_byte_candidate(&mut powered_candidate, tag, &mut reader, &mut state),
                    &mut semantic_error,
                )?,
                _ => prefer_wire_or_first_semantic(
                    scan_payload(tag, &mut reader, &mut state, 1),
                    &mut semantic_error,
                )?,
            }
        }

        let embedded_position = match position {
            [None, None, None] => None,
            [Some(x), Some(y), Some(z)] => Some([x, y, z]),
            _ => {
                semantic_error.get_or_insert(BlockEntityNbtError::PartialPosition);
                None
            }
        };
        let consumed = reader.position();
        Ok((
            Self {
                bytes: Arc::from(&input[..consumed]),
                parsed: Arc::default(),
                id,
                embedded_position,
                note_candidate,
                powered_candidate,
            },
            consumed,
            semantic_error,
        ))
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    #[must_use]
    pub const fn embedded_position(&self) -> Option<[i32; 3]> {
        self.embedded_position
    }

    #[must_use]
    pub const fn note_candidate(&self) -> RootByteCandidate {
        self.note_candidate
    }

    #[must_use]
    pub const fn powered_candidate(&self) -> RootByteCandidate {
        self.powered_candidate
    }
}

/// Sparse block-entity replacement decoded for one packet scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBlockEntities {
    entities: BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>,
    bytes_consumed: usize,
}

impl DecodedBlockEntities {
    /// Decodes the border-block list and every block entity after it, keeping
    /// entities vanilla would place and skipping the rest.
    pub fn decode_level_chunk_tail(chunk: ChunkKey, y_range: Range<i32>, payload: &[u8]) -> Self {
        let mut reader = crate::sub_chunk::Reader::new(payload);
        let border_blocks = usize::from(reader.read_u8());
        reader.read_exact(border_blocks);
        let mut decoded = decode_scoped_entities(
            BlockEntityScope::Chunk { chunk, y_range },
            reader.remaining(),
            MAX_BLOCK_ENTITIES_PER_CHUNK,
        );
        decoded.bytes_consumed = payload.len();
        decoded
    }

    /// Decodes every block entity after one serialized SubChunkPacket entry.
    pub fn decode_sub_chunk_tail(sub_chunk: SubChunkKey, payload: &[u8]) -> Self {
        decode_scoped_entities(
            BlockEntityScope::SubChunk(sub_chunk),
            payload,
            MAX_BLOCK_ENTITIES_PER_SUB_CHUNK,
        )
    }

    /// Validates an exact packet-56 payload against its outer packet position.
    pub fn decode_live(
        key: BlockEntityKey,
        payload: &[u8],
    ) -> Result<BlockEntityNbt, BlockEntityError> {
        let (nbt, consumed, semantic_error) = BlockEntityNbt::scan_prefix(payload)?;
        if consumed != payload.len() {
            return Err(BlockEntityError::TrailingBytes {
                remaining: payload.len() - consumed,
            });
        }
        if let Some(error) = semantic_error {
            return Err(error.into());
        }
        if let Some(actual) = nbt.embedded_position()
            && actual != key.position()
        {
            return Err(BlockEntityError::PositionMismatch {
                expected: key.position(),
                actual,
            });
        }
        Ok(nbt)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    #[must_use]
    pub const fn bytes_consumed(&self) -> usize {
        self.bytes_consumed
    }

    #[must_use]
    pub fn get(&self, key: BlockEntityKey) -> Option<Arc<BlockEntityNbt>> {
        self.entities.get(&key).cloned()
    }

    pub(crate) fn into_entities(self) -> BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>> {
        self.entities
    }

    pub(crate) fn entities(&self) -> impl Iterator<Item = (BlockEntityKey, &BlockEntityNbt)> {
        self.entities.iter().map(|(&key, nbt)| (key, nbt.as_ref()))
    }
}

/// One successful SubChunk block prefix and its complete sparse entity tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSubChunk {
    sub_chunk: SubChunk,
    block_entities: DecodedBlockEntities,
}

impl DecodedSubChunk {
    /// Vanilla stores the entry at the requested Y whatever the payload's Y byte says.
    pub fn decode(key: SubChunkKey, payload: &[u8], ids: &dyn BlockIds) -> Self {
        let (sub_chunk, consumed) = SubChunk::decode_prefix(payload, ids);
        let block_entities = DecodedBlockEntities::decode_sub_chunk_tail(key, &payload[consumed..]);
        Self {
            sub_chunk,
            block_entities,
        }
    }

    #[must_use]
    pub fn sub_chunk(&self) -> &SubChunk {
        &self.sub_chunk
    }

    pub(crate) fn into_parts(self) -> (SubChunk, DecodedBlockEntities) {
        (self.sub_chunk, self.block_entities)
    }
}

/// Bytes vanilla's lenient NBT read consumes for one root at the start of
/// `input`: one byte for a non-compound root, the rest of the input when a
/// compound is malformed.
pub(crate) fn lenient_nbt_len(input: &[u8]) -> usize {
    match input.first() {
        Some(&COMPOUND_TAG) => {
            BlockEntityNbt::scan_prefix(input).map_or(input.len(), |(_, consumed, _)| consumed)
        }
        Some(_) => 1,
        None => 0,
    }
}

const COMPOUND_TAG: u8 = 10;

#[derive(Debug, Clone)]
enum BlockEntityScope {
    Chunk {
        chunk: ChunkKey,
        y_range: Range<i32>,
    },
    SubChunk(SubChunkKey),
}

/// Reads roots until the payload ends, as vanilla does, skipping malformed
/// NBT and entities outside the scope or at an already taken position.
fn decode_scoped_entities(
    scope: BlockEntityScope,
    payload: &[u8],
    max_entities: usize,
) -> DecodedBlockEntities {
    let mut entities = BTreeMap::new();
    let mut consumed = 0;
    let mut retained_bytes = 0;
    while consumed < payload.len() && entities.len() < max_entities {
        let input = &payload[consumed..];
        if input[0] != COMPOUND_TAG {
            consumed += 1;
            continue;
        }
        let Ok((nbt, used, semantic_error)) = BlockEntityNbt::scan_prefix(input) else {
            break;
        };
        consumed += used;
        if semantic_error.is_some() {
            continue;
        }
        let Some([x, y, z]) = nbt.embedded_position() else {
            continue;
        };
        let (dimension, in_scope) = match &scope {
            BlockEntityScope::Chunk { chunk, y_range } => (
                chunk.dimension,
                ChunkKey::new(chunk.dimension, x >> 4, z >> 4) == *chunk && y_range.contains(&y),
            ),
            BlockEntityScope::SubChunk(key) => (
                key.dimension,
                BlockEntityKey::new(key.dimension, x, y, z).sub_chunk() == *key,
            ),
        };
        let key = BlockEntityKey::new(dimension, x, y, z);
        if in_scope && !entities.contains_key(&key) {
            let total = retained_bytes + nbt.bytes().len();
            if total <= MAX_BLOCK_ENTITY_BYTES_PER_CHUNK {
                entities.insert(key, Arc::new(nbt));
                retained_bytes = total;
            }
        }
    }
    DecodedBlockEntities {
        entities,
        bytes_consumed: payload.len(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockEntityError {
    #[error(transparent)]
    Nbt(#[from] BlockEntityNbtError),
    #[error("block-entity tail exceeds {max} sparse records")]
    TooManyEntities { max: usize },
    #[error("chunk block entities retain {len} NBT bytes, exceeding {max}")]
    ChunkEntityBytesTooLarge { len: usize, max: usize },
    #[error("live block-entity position mismatch: expected {expected:?}, got {actual:?}")]
    PositionMismatch {
        expected: [i32; 3],
        actual: [i32; 3],
    },
    #[error("live block-entity NBT has {remaining} trailing bytes")]
    TrailingBytes { remaining: usize },
}

/// Lets later malformed wire override a deferred semantic error while preserving first policy.
fn prefer_wire_or_first_semantic<T>(
    result: Result<T, BlockEntityNbtError>,
    semantic_error: &mut Option<BlockEntityNbtError>,
) -> Result<T, BlockEntityNbtError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) if error.wire_error_reason().is_some() => Err(error),
        Err(error) => Err(semantic_error.take().unwrap_or(error)),
    }
}

fn scan_root_byte_candidate(
    candidate: &mut RootByteCandidate,
    tag: u8,
    reader: &mut Reader<'_>,
    state: &mut ScanState,
) -> Result<(), BlockEntityNbtError> {
    let value = if tag == 1 {
        Some(reader.read_u8("root byte candidate")?)
    } else {
        scan_payload(tag, reader, state, 1)?;
        None
    };
    *candidate = match (*candidate, value) {
        (RootByteCandidate::Absent, Some(value)) => RootByteCandidate::Value(value),
        _ => RootByteCandidate::Invalid,
    };
    Ok(())
}

#[derive(Debug, Default)]
struct ScanState {
    tags: usize,
}

impl ScanState {
    fn visit_tag(&mut self) -> Result<(), BlockEntityNbtError> {
        self.tags += 1;
        if self.tags > MAX_NBT_TAGS {
            Err(BlockEntityNbtError::TooManyTags { max: MAX_NBT_TAGS })
        } else {
            Ok(())
        }
    }

    fn enter_container(&self, depth: usize) -> Result<usize, BlockEntityNbtError> {
        if depth >= MAX_NBT_DEPTH {
            Err(BlockEntityNbtError::DepthExceeded { max: MAX_NBT_DEPTH })
        } else {
            Ok(depth + 1)
        }
    }
}

fn scan_payload(
    tag: u8,
    reader: &mut Reader<'_>,
    state: &mut ScanState,
    depth: usize,
) -> Result<(), BlockEntityNbtError> {
    match tag {
        1 => reader.skip(1, "byte"),
        2 => reader.skip(2, "short"),
        3 => reader.skip_zigzag_i32("int"),
        4 => reader.skip_zigzag_i64("long"),
        5 => reader.skip(4, "float"),
        6 => reader.skip(8, "double"),
        7 => {
            let len = reader.read_collection_length("byte array")?;
            reader.check_collection_limit(len, 1, "byte array")?;
            reader.skip(len, "byte array")
        }
        8 => {
            let _ = reader.read_string("string")?;
            Ok(())
        }
        9 => {
            let nested_depth = state.enter_container(depth)?;
            let element_tag = reader.read_u8("list element tag")?;
            let len = reader.read_collection_length("list")?;
            if element_tag == 0 && len != 0 {
                return Err(BlockEntityNbtError::NonEmptyEndList);
            }
            if len != 0 {
                reader.check_collection_limit(
                    len,
                    minimum_payload_size(element_tag)?,
                    "list elements",
                )?;
            }
            for _ in 0..len {
                state.visit_tag()?;
                scan_payload(element_tag, reader, state, nested_depth)?;
            }
            Ok(())
        }
        10 => {
            let nested_depth = state.enter_container(depth)?;
            loop {
                let child_tag = reader.read_u8("compound tag")?;
                if child_tag == 0 {
                    return Ok(());
                }
                state.visit_tag()?;
                let _ = reader.read_string("tag name")?;
                scan_payload(child_tag, reader, state, nested_depth)?;
            }
        }
        11 => {
            let len = reader.read_collection_length("int array")?;
            reader.check_collection_limit(len, 1, "int array elements")?;
            for _ in 0..len {
                reader.skip_zigzag_i32("int array element")?;
            }
            Ok(())
        }
        12 => {
            let len = reader.read_collection_length("long array")?;
            reader.check_collection_limit(len, 1, "long array elements")?;
            for _ in 0..len {
                reader.skip_zigzag_i64("long array element")?;
            }
            Ok(())
        }
        _ => Err(BlockEntityNbtError::UnknownTag { tag }),
    }
}

/// Returns the constant minimum encoded bytes for one NBT payload value.
fn minimum_payload_size(tag: u8) -> Result<usize, BlockEntityNbtError> {
    match tag {
        1 => Ok(1),
        2 => Ok(2),
        3 | 4 | 7 | 8 | 10 | 11 | 12 => Ok(1),
        5 => Ok(4),
        6 => Ok(8),
        9 => Ok(2),
        _ => Err(BlockEntityNbtError::UnknownTag { tag }),
    }
}

struct Reader<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    const fn position(&self) -> usize {
        self.position
    }

    fn read_u8(&mut self, context: &'static str) -> Result<u8, BlockEntityNbtError> {
        let value = self.read_exact(1, context)?[0];
        Ok(value)
    }

    fn read_exact(
        &mut self,
        len: usize,
        context: &'static str,
    ) -> Result<&'a [u8], BlockEntityNbtError> {
        self.require_remaining(len, context)?;
        let end = self.position + len;
        if end > MAX_BLOCK_ENTITY_NBT_BYTES {
            return Err(BlockEntityNbtError::TooManyBytes {
                max: MAX_BLOCK_ENTITY_NBT_BYTES,
            });
        }
        let bytes = &self.input[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    /// Proves that a declared field has enough bytes without advancing the reader.
    fn require_remaining(
        &self,
        len: usize,
        context: &'static str,
    ) -> Result<(), BlockEntityNbtError> {
        let remaining = self.input.len().saturating_sub(self.position);
        if remaining < len {
            Err(BlockEntityNbtError::UnexpectedEof {
                context,
                needed: len,
                remaining,
            })
        } else {
            Ok(())
        }
    }

    /// Applies the collection work limit after proving the constant minimum payload exists.
    fn check_collection_limit(
        &self,
        len: usize,
        minimum_element_bytes: usize,
        context: &'static str,
    ) -> Result<(), BlockEntityNbtError> {
        if len <= MAX_NBT_COLLECTION_LENGTH {
            return Ok(());
        }
        let minimum_bytes = len.saturating_mul(minimum_element_bytes);
        self.require_remaining(minimum_bytes, context)?;
        Err(BlockEntityNbtError::CollectionTooLong {
            len,
            max: MAX_NBT_COLLECTION_LENGTH,
        })
    }

    fn skip(&mut self, len: usize, context: &'static str) -> Result<(), BlockEntityNbtError> {
        let _ = self.read_exact(len, context)?;
        Ok(())
    }

    fn read_var_u32(&mut self, context: &'static str) -> Result<u32, BlockEntityNbtError> {
        let mut value = 0_u32;
        for index in 0..5 {
            let byte = self.read_u8(context)?;
            if index == 4 {
                if byte & 0x80 != 0 {
                    return Err(BlockEntityNbtError::VarIntTooLong);
                }
                if byte & 0x70 != 0 {
                    return Err(BlockEntityNbtError::VarIntOverflow);
                }
            }
            value |= u32::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(BlockEntityNbtError::VarIntTooLong)
    }

    fn read_zigzag_i32(&mut self, context: &'static str) -> Result<i32, BlockEntityNbtError> {
        let value = self.read_var_u32(context)?;
        Ok(((value >> 1) as i32) ^ -((value & 1) as i32))
    }

    fn skip_zigzag_i32(&mut self, context: &'static str) -> Result<(), BlockEntityNbtError> {
        let _ = self.read_zigzag_i32(context)?;
        Ok(())
    }

    fn skip_zigzag_i64(&mut self, context: &'static str) -> Result<(), BlockEntityNbtError> {
        for index in 0..10 {
            let byte = self.read_u8(context)?;
            if index == 9 {
                if byte & 0x80 != 0 {
                    return Err(BlockEntityNbtError::VarLongTooLong);
                }
                if byte & 0x7e != 0 {
                    return Err(BlockEntityNbtError::VarLongOverflow);
                }
            }
            if byte & 0x80 == 0 {
                return Ok(());
            }
        }
        Err(BlockEntityNbtError::VarLongTooLong)
    }

    fn read_collection_length(
        &mut self,
        context: &'static str,
    ) -> Result<usize, BlockEntityNbtError> {
        let value = self.read_zigzag_i32(context)?;
        if value < 0 {
            return Err(BlockEntityNbtError::NegativeLength { value });
        }
        Ok(value as usize)
    }

    fn read_string(&mut self, context: &'static str) -> Result<&'a str, BlockEntityNbtError> {
        let len = self.read_var_u32(context)? as usize;
        self.require_remaining(len, context)?;
        if len > MAX_NBT_STRING_BYTES {
            return Err(BlockEntityNbtError::StringTooLong {
                len,
                max: MAX_NBT_STRING_BYTES,
            });
        }
        std::str::from_utf8(self.read_exact(len, context)?)
            .map_err(|_| BlockEntityNbtError::InvalidUtf8)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockEntityNbtError {
    #[error("block-entity NBT root tag must be Compound, got {tag}")]
    RootNotCompound { tag: u8 },
    #[error("unknown NBT tag {tag}")]
    UnknownTag { tag: u8 },
    #[error("unexpected end while reading {context}: need {needed} bytes, have {remaining}")]
    UnexpectedEof {
        context: &'static str,
        needed: usize,
        remaining: usize,
    },
    #[error("NBT VarInt does not terminate within five bytes")]
    VarIntTooLong,
    #[error("NBT VarInt overflows u32")]
    VarIntOverflow,
    #[error("NBT VarLong does not terminate within ten bytes")]
    VarLongTooLong,
    #[error("NBT VarLong overflows u64")]
    VarLongOverflow,
    #[error("NBT length is negative: {value}")]
    NegativeLength { value: i32 },
    #[error("NBT collection has {len} elements, exceeding {max}")]
    CollectionTooLong { len: usize, max: usize },
    #[error("NBT string has {len} bytes, exceeding {max}")]
    StringTooLong { len: usize, max: usize },
    #[error("NBT string is not valid UTF-8")]
    InvalidUtf8,
    #[error("NBT compound/list depth exceeds {max}")]
    DepthExceeded { max: usize },
    #[error("NBT contains more than {max} tags")]
    TooManyTags { max: usize },
    #[error("block-entity NBT exceeds {max} encoded bytes")]
    TooManyBytes { max: usize },
    #[error("NBT List<TagEnd> must be empty")]
    NonEmptyEndList,
    #[error("duplicate root block-entity field {field}")]
    DuplicateRootField { field: &'static str },
    #[error("root block-entity field {field} must use tag {expected}, got {actual}")]
    InvalidRootFieldType {
        field: &'static str,
        expected: u8,
        actual: u8,
    },
    #[error("block-entity position must contain all of x, y, and z or none")]
    PartialPosition,
}

impl BlockEntityNbtError {
    /// Returns a stable reason only for malformed NBT bytes, excluding policy
    /// limits and structurally complete semantic shape errors.
    #[must_use]
    pub const fn wire_error_reason(&self) -> Option<&'static str> {
        match self {
            Self::UnknownTag { .. }
            | Self::UnexpectedEof { .. }
            | Self::VarIntTooLong
            | Self::VarIntOverflow
            | Self::VarLongTooLong
            | Self::VarLongOverflow
            | Self::NegativeLength { .. }
            | Self::InvalidUtf8
            | Self::NonEmptyEndList => Some("malformed block-entity NBT wire"),
            _ => None,
        }
    }
}
