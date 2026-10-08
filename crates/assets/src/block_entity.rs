//! Compiled block-entity carrier: one packed texture atlas for the block-entity
//! models plus the version-pinned inventory of block-entity ids and their draw route.
//! Model geometry is authored in the renderer; only textures and the inventory are data.

use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::{AssetError, canonical_source_manifest_sha256};

pub const BLOCK_ENTITY_CARRIER_MAGIC: [u8; 8] = *b"MCBEBEN1";
pub const BLOCK_ENTITY_CARRIER_VERSION: u32 = 1;
/// Runtime texture sampled by the end crystal's additional beam effect.
pub const CRYSTAL_BEAM_TEXTURE: &str = "textures/entity/endercrystal/endercrystal_beam";
pub const MAX_BLOCK_ENTITY_ATLAS_SIDE: u32 = 4096;
pub const MAX_BLOCK_ENTITY_PLACEMENTS: usize = 2048;
pub const MAX_BLOCK_ENTITY_KEY_BYTES: usize = 256;
pub const MAX_BLOCK_ENTITY_CARRIER_BYTES: usize = 72 * 1024 * 1024;

const HEADER_BYTES: usize = 64;
const HASH_BYTES: usize = 32;

/// How the client draws one block-entity id.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum BlockEntityRouteKind {
    /// The block state already renders the whole visual.
    ExistingBlockState = 1,
    /// The block-entity renderer draws a model in addition to the block state.
    Model = 2,
    /// The block-entity renderer draws only text quads over the block state.
    TextOverlay = 3,
    /// The block entity carries data only and draws nothing.
    NoDraw = 4,
}

impl BlockEntityRouteKind {
    #[cfg(test)]
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            1 => Self::ExistingBlockState,
            2 => Self::Model,
            3 => Self::TextOverlay,
            4 => Self::NoDraw,
            _ => return None,
        })
    }
}

/// Block-entity NBT `id` values and their draw route, sorted by id.
pub const BLOCK_ENTITY_ROUTES: &[(&str, BlockEntityRouteKind)] = &[
    ("Banner", BlockEntityRouteKind::Model),
    ("Barrel", BlockEntityRouteKind::ExistingBlockState),
    ("Beacon", BlockEntityRouteKind::Model),
    ("Bed", BlockEntityRouteKind::Model),
    ("Bell", BlockEntityRouteKind::Model),
    ("BlastFurnace", BlockEntityRouteKind::ExistingBlockState),
    ("BrewingStand", BlockEntityRouteKind::NoDraw),
    ("Campfire", BlockEntityRouteKind::Model),
    ("Chest", BlockEntityRouteKind::Model),
    ("Conduit", BlockEntityRouteKind::Model),
    ("CopperGolemStatue", BlockEntityRouteKind::Model),
    ("DecoratedPot", BlockEntityRouteKind::Model),
    ("EnchantTable", BlockEntityRouteKind::Model),
    ("EndGateway", BlockEntityRouteKind::Model),
    ("EndPortal", BlockEntityRouteKind::Model),
    ("EnderChest", BlockEntityRouteKind::Model),
    ("FlowerPot", BlockEntityRouteKind::Model),
    ("Furnace", BlockEntityRouteKind::ExistingBlockState),
    ("GlowItemFrame", BlockEntityRouteKind::Model),
    ("HangingSign", BlockEntityRouteKind::TextOverlay),
    ("Hopper", BlockEntityRouteKind::NoDraw),
    ("ItemFrame", BlockEntityRouteKind::Model),
    ("Jukebox", BlockEntityRouteKind::NoDraw),
    ("Lectern", BlockEntityRouteKind::Model),
    ("MobSpawner", BlockEntityRouteKind::Model),
    ("Note", BlockEntityRouteKind::NoDraw),
    ("ShulkerBox", BlockEntityRouteKind::Model),
    ("Sign", BlockEntityRouteKind::TextOverlay),
    ("Skull", BlockEntityRouteKind::Model),
    ("Smoker", BlockEntityRouteKind::ExistingBlockState),
];

/// The pixel rect of one pack texture inside the atlas; `name` is its
/// pack-relative path without extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockEntityPlacement {
    pub name: Box<str>,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Decoded, validated carrier with placements sorted by name.
#[derive(Clone)]
pub struct RuntimeBlockEntityAssets {
    identity: [u8; 32],
    source_manifest_sha256: [u8; 32],
    atlas_width: u32,
    atlas_height: u32,
    rgba8: Arc<[u8]>,
    placements: Arc<[BlockEntityPlacement]>,
}

impl std::fmt::Debug for RuntimeBlockEntityAssets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeBlockEntityAssets")
            .field("atlas", &(self.atlas_width, self.atlas_height))
            .field("placements", &self.placements.len())
            .finish_non_exhaustive()
    }
}

impl RuntimeBlockEntityAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() > MAX_BLOCK_ENTITY_CARRIER_BYTES
            || bytes.len() < HEADER_BYTES + HASH_BYTES
            || bytes[..8] != BLOCK_ENTITY_CARRIER_MAGIC
            || read_u32(bytes, 8)? != BLOCK_ENTITY_CARRIER_VERSION
        {
            return Err(invalid("unsupported block-entity carrier header"));
        }
        let atlas_width = read_u32(bytes, 12)?;
        let atlas_height = read_u32(bytes, 16)?;
        let placement_count = read_u32(bytes, 20)? as usize;
        let source_manifest_sha256 = read_array::<32>(bytes, 24)?;
        let payload_end = read_u32(bytes, 56)? as usize;
        if atlas_width == 0
            || atlas_height == 0
            || atlas_width > MAX_BLOCK_ENTITY_ATLAS_SIDE
            || atlas_height > MAX_BLOCK_ENTITY_ATLAS_SIDE
            || placement_count > MAX_BLOCK_ENTITY_PLACEMENTS
            || source_manifest_sha256 == [0; 32]
            || read_u32(bytes, 60)? != 0
            || payload_end < HEADER_BYTES
            || payload_end.checked_add(HASH_BYTES) != Some(bytes.len())
        {
            return Err(invalid("noncanonical block-entity carrier layout"));
        }
        let identity = crate::encoding::sealed_identity(bytes, payload_end)
            .ok_or_else(|| invalid("block-entity carrier hash mismatch"))?;
        let pixel_bytes = pixel_length(atlas_width, atlas_height)?;
        let mut cursor = HEADER_BYTES;
        let pixels_end = cursor
            .checked_add(pixel_bytes)
            .filter(|end| *end <= payload_end)
            .ok_or_else(|| invalid("block-entity atlas runs past the payload"))?;
        let rgba8: Arc<[u8]> = Arc::from(&bytes[cursor..pixels_end]);
        cursor = pixels_end;
        let mut placements: Vec<BlockEntityPlacement> = Vec::with_capacity(placement_count);
        for _ in 0..placement_count {
            let length = read_u16(bytes, cursor)? as usize;
            cursor += 2;
            let end = cursor
                .checked_add(length)
                .filter(|end| {
                    *end <= payload_end && length != 0 && length <= MAX_BLOCK_ENTITY_KEY_BYTES
                })
                .ok_or_else(|| invalid("block-entity placement key exceeds bounds"))?;
            let name = std::str::from_utf8(&bytes[cursor..end])
                .map_err(|_| invalid("block-entity placement key is not UTF-8"))?;
            cursor = end;
            let x = read_u32(bytes, cursor)?;
            let y = read_u32(bytes, cursor + 4)?;
            let width = read_u32(bytes, cursor + 8)?;
            let height = read_u32(bytes, cursor + 12)?;
            cursor += 16;
            if width == 0
                || height == 0
                || x.checked_add(width).is_none_or(|edge| edge > atlas_width)
                || y.checked_add(height).is_none_or(|edge| edge > atlas_height)
                || placements
                    .last()
                    .is_some_and(|previous| previous.name.as_ref() >= name)
            {
                return Err(invalid("block-entity placement is invalid or unsorted"));
            }
            placements.push(BlockEntityPlacement {
                name: name.into(),
                x,
                y,
                width,
                height,
            });
        }
        if cursor != payload_end {
            return Err(invalid("trailing block-entity carrier payload"));
        }
        Ok(Self {
            identity,
            source_manifest_sha256,
            atlas_width,
            atlas_height,
            rgba8,
            placements: placements.into(),
        })
    }

    #[must_use]
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub const fn atlas_size(&self) -> [u32; 2] {
        [self.atlas_width, self.atlas_height]
    }

    #[must_use]
    pub fn atlas_rgba8(&self) -> &Arc<[u8]> {
        &self.rgba8
    }

    #[must_use]
    pub fn placements(&self) -> &[BlockEntityPlacement] {
        &self.placements
    }

    #[must_use]
    pub fn placement(&self, name: &str) -> Option<&BlockEntityPlacement> {
        self.placements
            .binary_search_by(|entry| entry.name.as_ref().cmp(name))
            .ok()
            .map(|index| &self.placements[index])
    }
}

/// The draw route for a block-entity NBT `id`, if it is in the pinned inventory.
#[must_use]
pub fn block_entity_route(id: &str) -> Option<BlockEntityRouteKind> {
    BLOCK_ENTITY_ROUTES
        .binary_search_by(|(entry, _)| (*entry).cmp(id))
        .ok()
        .map(|index| BLOCK_ENTITY_ROUTES[index].1)
}

/// Encodes an atlas and its placements; `placements` must be sorted by name with unique keys.
pub fn encode_block_entity_catalog(
    source_manifest: &[u8],
    atlas_width: u32,
    atlas_height: u32,
    rgba8: &[u8],
    placements: &[BlockEntityPlacement],
) -> Result<Vec<u8>, AssetError> {
    let source_manifest_sha256 = canonical_source_manifest_sha256(source_manifest);
    if atlas_width == 0
        || atlas_height == 0
        || atlas_width > MAX_BLOCK_ENTITY_ATLAS_SIDE
        || atlas_height > MAX_BLOCK_ENTITY_ATLAS_SIDE
        || rgba8.len() != pixel_length(atlas_width, atlas_height)?
        || placements.len() > MAX_BLOCK_ENTITY_PLACEMENTS
    {
        return Err(invalid("block-entity atlas exceeds bounds"));
    }
    let mut bytes = Vec::with_capacity(HEADER_BYTES + rgba8.len() + placements.len() * 64);
    bytes.extend_from_slice(&BLOCK_ENTITY_CARRIER_MAGIC);
    bytes.extend_from_slice(&BLOCK_ENTITY_CARRIER_VERSION.to_le_bytes());
    bytes.extend_from_slice(&atlas_width.to_le_bytes());
    bytes.extend_from_slice(&atlas_height.to_le_bytes());
    bytes.extend_from_slice(&(placements.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&source_manifest_sha256);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(rgba8);
    for placement in placements {
        let name = placement.name.as_bytes();
        if name.is_empty() || name.len() > MAX_BLOCK_ENTITY_KEY_BYTES {
            return Err(invalid("block-entity placement key exceeds bounds"));
        }
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name);
        for value in [placement.x, placement.y, placement.width, placement.height] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    let payload_end = u32::try_from(bytes.len()).map_err(|_| invalid("carrier length overflow"))?;
    bytes[56..60].copy_from_slice(&payload_end.to_le_bytes());
    let hash = Sha256::digest(&bytes);
    bytes.extend_from_slice(&hash);
    if bytes.len() > MAX_BLOCK_ENTITY_CARRIER_BYTES {
        return Err(invalid("block-entity carrier exceeds bound"));
    }
    // Round-trip so an unsorted or out-of-bounds placement fails at compile time.
    RuntimeBlockEntityAssets::decode(&bytes)?;
    Ok(bytes)
}

fn pixel_length(width: u32, height: u32) -> Result<usize, AssetError> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("block-entity pixel length overflow"))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(offset..offset + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| invalid("truncated block-entity carrier"))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AssetError> {
    Ok(u16::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AssetError> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(name: &str, x: u32) -> BlockEntityPlacement {
        BlockEntityPlacement {
            name: name.into(),
            x,
            y: 0,
            width: 2,
            height: 2,
        }
    }

    #[test]
    fn route_table_is_sorted_and_resolves_ids() {
        assert!(
            BLOCK_ENTITY_ROUTES
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
        );
        assert_eq!(
            block_entity_route("Chest"),
            Some(BlockEntityRouteKind::Model)
        );
        assert_eq!(
            block_entity_route("Sign"),
            Some(BlockEntityRouteKind::TextOverlay)
        );
        assert_eq!(block_entity_route("Nope"), None);
        assert_eq!(
            BlockEntityRouteKind::from_byte(4),
            Some(BlockEntityRouteKind::NoDraw)
        );
    }

    #[test]
    fn carrier_round_trips_and_rejects_corruption() {
        let pixels = vec![7u8; 4 * 4 * 4];
        let placements = [placement("a", 0), placement("b", 2)];
        let bytes = encode_block_entity_catalog(b"{}", 4, 4, &pixels, &placements).unwrap();
        let decoded = RuntimeBlockEntityAssets::decode(&bytes).unwrap();
        assert_eq!(decoded.atlas_size(), [4, 4]);
        assert_eq!(decoded.placement("b").unwrap().x, 2);
        assert!(decoded.placement("c").is_none());
        let mut corrupt = bytes.clone();
        corrupt[HEADER_BYTES] ^= 1;
        assert!(RuntimeBlockEntityAssets::decode(&corrupt).is_err());
    }

    #[test]
    fn encode_rejects_unsorted_and_out_of_bounds_placements() {
        let pixels = vec![0u8; 4 * 4 * 4];
        assert!(
            encode_block_entity_catalog(
                b"{}",
                4,
                4,
                &pixels,
                &[placement("b", 0), placement("a", 2)]
            )
            .is_err()
        );
        assert!(encode_block_entity_catalog(b"{}", 4, 4, &pixels, &[placement("a", 3)]).is_err());
    }
}
