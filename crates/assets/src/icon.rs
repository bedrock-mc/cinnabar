//! Compiled item-icon carrier: the exact sprite pixels for every
//! sprite-routed item visual in the pinned pack, keyed by the same
//! `(identifier, metadata)` item-visual keys the entity carrier compiles.
//!
//! Like the HUD and localization carriers, the carrier is provenance-pinned
//! to the canonical `assets/vanilla-source.json` identity and required at
//! startup: hotbar and offhand icons are player-facing pixels, so a missing,
//! malformed, oversized, or stale carrier is a fatal, actionable error
//! rather than a silent degrade. Alias keys reference deduplicated sprite
//! records so metadata variants never duplicate pixels.

use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::AssetError;

pub const ICON_CARRIER_MAGIC: [u8; 8] = *b"MCBEICO1";
pub const ICON_CARRIER_VERSION: u32 = 1;
/// One sprite per compiled item visual at most.
pub const MAX_ICON_SPRITES: usize = crate::item::MAX_ITEM_VISUALS;
/// One entry per item visual plus one per alias at most.
pub const MAX_ICON_ENTRIES: usize =
    crate::item::MAX_ITEM_VISUALS + crate::item::MAX_ITEM_VISUAL_ALIASES;
pub const MAX_ICON_KEY_BYTES: usize = crate::item::MAX_ITEM_IDENTIFIER_BYTES;
/// Vanilla item sprites are 16x16 with a handful of 32x32 outliers; anything
/// larger is not a flat inventory icon.
pub const MAX_ICON_SIDE: u32 = 64;
pub const MAX_ICON_CARRIER_BYTES: usize = 16 * 1024 * 1024;
const HEADER_BYTES: usize = 64;
const HASH_BYTES: usize = 32;

/// One deduplicated RGBA8 sprite.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IconSprite {
    pub width: u16,
    pub height: u16,
    pub rgba8: Arc<[u8]>,
}

/// One `(identifier, metadata)` key resolving to a sprite index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IconEntry {
    pub identifier: Box<str>,
    pub metadata: u32,
    pub sprite: u32,
}

/// Decoded, validated icon catalog with binary-search lookup.
pub struct RuntimeIconCatalog {
    source_manifest_sha256: [u8; 32],
    sprites: Arc<[IconSprite]>,
    entries: Arc<[IconEntry]>,
}

impl std::fmt::Debug for RuntimeIconCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeIconCatalog")
            .field("sprites", &self.sprites.len())
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl RuntimeIconCatalog {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() > MAX_ICON_CARRIER_BYTES {
            return Err(invalid("icon carrier exceeds bound"));
        }
        if bytes.len() < HEADER_BYTES + HASH_BYTES
            || bytes[..8] != ICON_CARRIER_MAGIC
            || read_u32(bytes, 8)? != ICON_CARRIER_VERSION
        {
            return Err(invalid("unsupported icon carrier header"));
        }
        let sprite_count = read_u32(bytes, 12)? as usize;
        let entry_count = read_u32(bytes, 16)? as usize;
        if read_u32(bytes, 20)? != 0 {
            return Err(invalid("noncanonical icon carrier padding"));
        }
        let source_manifest_sha256 = read_array::<32>(bytes, 24)?;
        let payload_end = usize::try_from(u64::from_le_bytes(read_array(bytes, 56)?))
            .map_err(|_| invalid("icon carrier payload exceeds platform"))?;
        if sprite_count > MAX_ICON_SPRITES
            || entry_count > MAX_ICON_ENTRIES
            || payload_end < HEADER_BYTES
            || bytes.len()
                != payload_end
                    .checked_add(HASH_BYTES)
                    .ok_or_else(|| invalid("icon carrier length overflow"))?
        {
            return Err(invalid("noncanonical icon carrier layout"));
        }
        if Sha256::digest(&bytes[..payload_end]).as_slice() != &bytes[payload_end..] {
            return Err(invalid("icon carrier envelope hash mismatch"));
        }

        let mut cursor = HEADER_BYTES;
        let mut sprites = Vec::with_capacity(sprite_count);
        for _ in 0..sprite_count {
            let width = read_u16(bytes, cursor)?;
            let height = read_u16(bytes, cursor + 2)?;
            cursor += 4;
            if width == 0
                || height == 0
                || u32::from(width) > MAX_ICON_SIDE
                || u32::from(height) > MAX_ICON_SIDE
            {
                return Err(invalid("icon sprite dimensions exceed bounds"));
            }
            let pixel_bytes = usize::from(width) * usize::from(height) * 4;
            let end = cursor
                .checked_add(pixel_bytes)
                .filter(|end| *end <= payload_end)
                .ok_or_else(|| invalid("icon sprite pixels run past the payload"))?;
            sprites.push(IconSprite {
                width,
                height,
                rgba8: Arc::from(&bytes[cursor..end]),
            });
            cursor = end;
        }

        let mut entries: Vec<IconEntry> = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let key_length = usize::from(read_u16(bytes, cursor)?);
            cursor += 2;
            if key_length == 0 || key_length > MAX_ICON_KEY_BYTES {
                return Err(invalid("icon entry key exceeds bounds"));
            }
            let key_end = cursor
                .checked_add(key_length)
                .filter(|end| *end <= payload_end)
                .ok_or_else(|| invalid("icon entry key runs past the payload"))?;
            let identifier = std::str::from_utf8(&bytes[cursor..key_end])
                .map_err(|_| invalid("icon entry key is not UTF-8"))?;
            cursor = key_end;
            let metadata = read_u32(bytes, cursor)?;
            let sprite = read_u32(bytes, cursor + 4)?;
            cursor += 8;
            if sprite as usize >= sprites.len() {
                return Err(invalid("icon entry references a missing sprite"));
            }
            // Strictly ascending keys give canonical bytes and binary search.
            if entries.last().is_some_and(|previous| {
                (previous.identifier.as_ref(), previous.metadata) >= (identifier, metadata)
            }) {
                return Err(invalid("icon entries are not strictly sorted"));
            }
            entries.push(IconEntry {
                identifier: identifier.into(),
                metadata,
                sprite,
            });
        }
        if cursor != payload_end {
            return Err(invalid("trailing icon carrier payload"));
        }
        Ok(Self {
            source_manifest_sha256,
            sprites: sprites.into(),
            entries: entries.into(),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub fn sprites(&self) -> &[IconSprite] {
        &self.sprites
    }

    #[must_use]
    pub fn entries(&self) -> &[IconEntry] {
        &self.entries
    }

    /// The sprite for one `(identifier, metadata)` item-visual key, falling
    /// back to the metadata-0 variant exactly like the visual-route lookup.
    #[must_use]
    pub fn lookup(&self, identifier: &str, metadata: u32) -> Option<&IconSprite> {
        self.lookup_exact(identifier, metadata).or_else(|| {
            (metadata != 0)
                .then(|| self.lookup_exact(identifier, 0))
                .flatten()
        })
    }

    fn lookup_exact(&self, identifier: &str, metadata: u32) -> Option<&IconSprite> {
        self.entries
            .binary_search_by(|entry| {
                (entry.identifier.as_ref(), entry.metadata).cmp(&(identifier, metadata))
            })
            .ok()
            .map(|index| &self.sprites[self.entries[index].sprite as usize])
    }
}

/// Encodes deduplicated sprites and sorted entries into canonical bytes.
pub fn encode_icon_catalog(
    source_manifest_sha256: [u8; 32],
    sprites: &[IconSprite],
    entries: &[IconEntry],
) -> Result<Vec<u8>, AssetError> {
    if sprites.len() > MAX_ICON_SPRITES || entries.len() > MAX_ICON_ENTRIES {
        return Err(invalid("icon sprite or entry count exceeds bound"));
    }
    let mut payload = Vec::new();
    for sprite in sprites {
        if sprite.width == 0
            || sprite.height == 0
            || u32::from(sprite.width) > MAX_ICON_SIDE
            || u32::from(sprite.height) > MAX_ICON_SIDE
            || sprite.rgba8.len() != usize::from(sprite.width) * usize::from(sprite.height) * 4
        {
            return Err(invalid("icon sprite dimensions or pixels exceed bounds"));
        }
        payload.extend_from_slice(&sprite.width.to_le_bytes());
        payload.extend_from_slice(&sprite.height.to_le_bytes());
        payload.extend_from_slice(&sprite.rgba8);
    }
    let mut previous: Option<(&str, u32)> = None;
    for entry in entries {
        if entry.identifier.is_empty()
            || entry.identifier.len() > MAX_ICON_KEY_BYTES
            || entry.sprite as usize >= sprites.len()
        {
            return Err(invalid("icon entry key or sprite reference exceeds bounds"));
        }
        if previous.is_some_and(|previous| previous >= (entry.identifier.as_ref(), entry.metadata))
        {
            return Err(invalid("icon entries are not strictly sorted"));
        }
        payload.extend_from_slice(&(entry.identifier.len() as u16).to_le_bytes());
        payload.extend_from_slice(entry.identifier.as_bytes());
        payload.extend_from_slice(&entry.metadata.to_le_bytes());
        payload.extend_from_slice(&entry.sprite.to_le_bytes());
        previous = Some((entry.identifier.as_ref(), entry.metadata));
    }
    let payload_end = HEADER_BYTES
        .checked_add(payload.len())
        .filter(|end| end + HASH_BYTES <= MAX_ICON_CARRIER_BYTES)
        .ok_or_else(|| invalid("icon carrier exceeds bound"))?;
    let mut bytes = vec![0u8; HEADER_BYTES];
    bytes[..8].copy_from_slice(&ICON_CARRIER_MAGIC);
    bytes[8..12].copy_from_slice(&ICON_CARRIER_VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(sprites.len() as u32).to_le_bytes());
    bytes[16..20].copy_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes[24..56].copy_from_slice(&source_manifest_sha256);
    bytes[56..64].copy_from_slice(&(payload_end as u64).to_le_bytes());
    bytes.extend_from_slice(&payload);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AssetError> {
    Ok(u16::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AssetError> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or_else(|| invalid("icon carrier field overflow"))?,
        )
        .ok_or_else(|| invalid("truncated icon carrier field"))?
        .try_into()
        .map_err(|_| invalid("invalid icon carrier field"))
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
