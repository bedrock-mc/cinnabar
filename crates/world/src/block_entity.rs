use std::{collections::BTreeMap, sync::Arc};

use thiserror::Error;

use crate::{ChunkKey, DecodeError, SubChunk, SubChunkKey};

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
/// Maximum encoded block-entity tail accepted in one chunk/subchunk payload.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntityNbt {
    bytes: Arc<[u8]>,
    id: Option<Arc<str>>,
    embedded_position: Option<[i32; 3]>,
}

impl BlockEntityNbt {
    /// Prefix-decodes exactly one named root compound and preserves its exact
    /// encoded bytes. Trailing input belongs to subsequent block entities or
    /// the containing packet and is not consumed.
    pub fn decode_prefix(input: &[u8]) -> Result<(Self, usize), BlockEntityNbtError> {
        let mut reader = Reader::new(input);
        let root = reader.read_u8("root tag")?;
        if root != 10 {
            return Err(BlockEntityNbtError::RootNotCompound { tag: root });
        }
        let _root_name = reader.read_string("root name")?;
        let mut state = ScanState::default();
        state.visit_tag()?;
        state.enter_container(0)?;

        let mut id = None;
        let mut position = [None; 3];
        loop {
            let tag = reader.read_u8("compound tag")?;
            if tag == 0 {
                break;
            }
            state.visit_tag()?;
            let name = reader.read_string("tag name")?;
            match name {
                "id" => {
                    require_root_type(name, tag, 8)?;
                    if id.is_some() {
                        return Err(BlockEntityNbtError::DuplicateRootField { field: "id" });
                    }
                    id = Some(Arc::<str>::from(reader.read_string("id value")?));
                }
                "x" | "y" | "z" => {
                    require_root_type(name, tag, 3)?;
                    let (slot, field) = match name {
                        "x" => (0, "x"),
                        "y" => (1, "y"),
                        "z" => (2, "z"),
                        _ => unreachable!(),
                    };
                    if position[slot].is_some() {
                        return Err(BlockEntityNbtError::DuplicateRootField { field });
                    }
                    position[slot] = Some(reader.read_zigzag_i32("position")?);
                }
                _ => scan_payload(tag, &mut reader, &mut state, 1)?,
            }
        }

        let embedded_position = match position {
            [None, None, None] => None,
            [Some(x), Some(y), Some(z)] => Some([x, y, z]),
            _ => return Err(BlockEntityNbtError::PartialPosition),
        };
        let consumed = reader.position();
        Ok((
            Self {
                bytes: Arc::from(&input[..consumed]),
                id,
                embedded_position,
            },
            consumed,
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
}

/// Fully validated sparse block-entity replacement for one packet scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBlockEntities {
    entities: BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>,
    bytes_consumed: usize,
}

impl DecodedBlockEntities {
    /// Decodes the one-byte LevelChunk border-block count followed by zero or
    /// more concatenated NetworkLittleEndian compounds.
    pub fn decode_level_chunk_tail(
        chunk: ChunkKey,
        payload: &[u8],
    ) -> Result<Self, BlockEntityError> {
        ensure_tail_size(payload)?;
        let (&border_count, entities) = payload
            .split_first()
            .ok_or(BlockEntityError::MissingBorderBlockCount)?;
        if border_count != 0 {
            return Err(BlockEntityError::UnsupportedBorderBlocks {
                count: border_count,
            });
        }
        let mut decoded = decode_scoped_entities(
            BlockEntityScope::Chunk(chunk),
            entities,
            MAX_BLOCK_ENTITIES_PER_CHUNK,
        )?;
        decoded.bytes_consumed += 1;
        Ok(decoded)
    }

    /// Decodes every concatenated block-entity compound after one successful
    /// serialized subchunk.
    pub fn decode_sub_chunk_tail(
        sub_chunk: SubChunkKey,
        payload: &[u8],
    ) -> Result<Self, BlockEntityError> {
        ensure_tail_size(payload)?;
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
        let (nbt, consumed) = BlockEntityNbt::decode_prefix(payload)?;
        if consumed != payload.len() {
            return Err(BlockEntityError::TrailingBytes {
                remaining: payload.len() - consumed,
            });
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
    pub fn decode(key: SubChunkKey, payload: &[u8]) -> Result<Self, DecodeError> {
        let (sub_chunk, consumed) = SubChunk::decode_prefix(payload)?;
        if let Some(actual) = sub_chunk.y_index() {
            let actual = i32::from(actual);
            if actual != key.y {
                return Err(DecodeError::SubChunkIndexMismatch {
                    expected: key.y,
                    actual,
                });
            }
        }
        let block_entities =
            DecodedBlockEntities::decode_sub_chunk_tail(key, &payload[consumed..])?;
        Ok(Self {
            sub_chunk,
            block_entities,
        })
    }

    #[must_use]
    pub fn sub_chunk(&self) -> &SubChunk {
        &self.sub_chunk
    }

    pub(crate) fn into_parts(self) -> (SubChunk, DecodedBlockEntities) {
        (self.sub_chunk, self.block_entities)
    }
}

#[derive(Debug, Clone, Copy)]
enum BlockEntityScope {
    Chunk(ChunkKey),
    SubChunk(SubChunkKey),
}

fn ensure_tail_size(payload: &[u8]) -> Result<(), BlockEntityError> {
    if payload.len() > MAX_BLOCK_ENTITY_TAIL_BYTES {
        Err(BlockEntityError::TailTooLarge {
            len: payload.len(),
            max: MAX_BLOCK_ENTITY_TAIL_BYTES,
        })
    } else {
        Ok(())
    }
}

fn decode_scoped_entities(
    scope: BlockEntityScope,
    payload: &[u8],
    max_entities: usize,
) -> Result<DecodedBlockEntities, BlockEntityError> {
    let mut entities = BTreeMap::new();
    let mut consumed = 0;
    while consumed < payload.len() {
        if entities.len() == max_entities {
            return Err(BlockEntityError::TooManyEntities { max: max_entities });
        }
        let (nbt, used) = BlockEntityNbt::decode_prefix(&payload[consumed..])?;
        let position = nbt
            .embedded_position()
            .ok_or(BlockEntityError::MissingPosition)?;
        let dimension = match scope {
            BlockEntityScope::Chunk(key) => key.dimension,
            BlockEntityScope::SubChunk(key) => key.dimension,
        };
        let key = BlockEntityKey::new(dimension, position[0], position[1], position[2]);
        match scope {
            BlockEntityScope::Chunk(expected) if key.chunk() != expected => {
                return Err(BlockEntityError::OutsideChunk {
                    expected,
                    actual: key,
                });
            }
            BlockEntityScope::SubChunk(expected) if key.sub_chunk() != expected => {
                return Err(BlockEntityError::OutsideSubChunk {
                    expected,
                    actual: key,
                });
            }
            BlockEntityScope::Chunk(_) | BlockEntityScope::SubChunk(_) => {}
        }
        if entities.insert(key, Arc::new(nbt)).is_some() {
            return Err(BlockEntityError::DuplicatePosition { key });
        }
        consumed += used;
    }
    Ok(DecodedBlockEntities {
        entities,
        bytes_consumed: consumed,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockEntityError {
    #[error(transparent)]
    Nbt(#[from] BlockEntityNbtError),
    #[error("LevelChunk block-entity tail is missing the border-block count")]
    MissingBorderBlockCount,
    #[error("LevelChunk uses {count} unsupported border blocks")]
    UnsupportedBorderBlocks { count: u8 },
    #[error("block-entity tail has {len} bytes, exceeding {max}")]
    TailTooLarge { len: usize, max: usize },
    #[error("block-entity tail exceeds {max} sparse records")]
    TooManyEntities { max: usize },
    #[error("chunk block entities retain {len} NBT bytes, exceeding {max}")]
    ChunkEntityBytesTooLarge { len: usize, max: usize },
    #[error("chunk/subchunk block entity is missing its complete x/y/z position")]
    MissingPosition,
    #[error("duplicate block entity at {key:?}")]
    DuplicatePosition { key: BlockEntityKey },
    #[error("block entity {actual:?} is outside chunk {expected:?}")]
    OutsideChunk {
        expected: ChunkKey,
        actual: BlockEntityKey,
    },
    #[error("block entity {actual:?} is outside subchunk {expected:?}")]
    OutsideSubChunk {
        expected: SubChunkKey,
        actual: BlockEntityKey,
    },
    #[error("live block-entity position mismatch: expected {expected:?}, got {actual:?}")]
    PositionMismatch {
        expected: [i32; 3],
        actual: [i32; 3],
    },
    #[error("live block-entity NBT has {remaining} trailing bytes")]
    TrailingBytes { remaining: usize },
}

fn require_root_type(name: &str, actual: u8, expected: u8) -> Result<(), BlockEntityNbtError> {
    if actual == expected {
        Ok(())
    } else {
        Err(BlockEntityNbtError::InvalidRootFieldType {
            field: match name {
                "id" => "id",
                "x" => "x",
                "y" => "y",
                "z" => "z",
                _ => unreachable!(),
            },
            expected,
            actual,
        })
    }
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
            for _ in 0..len {
                reader.skip_zigzag_i32("int array element")?;
            }
            Ok(())
        }
        12 => {
            let len = reader.read_collection_length("long array")?;
            for _ in 0..len {
                reader.skip_zigzag_i64("long array element")?;
            }
            Ok(())
        }
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
        let end = self
            .position
            .checked_add(len)
            .ok_or(BlockEntityNbtError::TooManyBytes {
                max: MAX_BLOCK_ENTITY_NBT_BYTES,
            })?;
        if end > MAX_BLOCK_ENTITY_NBT_BYTES {
            return Err(BlockEntityNbtError::TooManyBytes {
                max: MAX_BLOCK_ENTITY_NBT_BYTES,
            });
        }
        let bytes =
            self.input
                .get(self.position..end)
                .ok_or(BlockEntityNbtError::UnexpectedEof {
                    context,
                    needed: len,
                    remaining: self.input.len().saturating_sub(self.position),
                })?;
        self.position = end;
        Ok(bytes)
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
        let len = value as usize;
        if len > MAX_NBT_COLLECTION_LENGTH {
            Err(BlockEntityNbtError::CollectionTooLong {
                len,
                max: MAX_NBT_COLLECTION_LENGTH,
            })
        } else {
            Ok(len)
        }
    }

    fn read_string(&mut self, context: &'static str) -> Result<&'a str, BlockEntityNbtError> {
        let len = self.read_var_u32(context)? as usize;
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
