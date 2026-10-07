//! Compiled JSON-UI asset carrier: the packed `textures/ui` atlas, each
//! texture's page placement, the nine-slice texture sidecars, and the raw
//! `ui/*.json` catalog the clean-room resolver consumes at runtime.
//!
//! The raw ui-json bytes are kept verbatim (not pre-resolved) because a joined
//! server pack overrides ui json at runtime, so resolution happens then, not
//! here. Textures pack into gutter-separated atlas pages; a placement records
//! the page and pixel rect, and a sidecar records the native `base_size` plus
//! any nine-slice insets keyed by the same `textures/ui/...` logical path the
//! ui json references. This carries data only; nothing here renders, and the
//! carrier is not yet wired into startup.

use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::AssetError;

pub const UI_CARRIER_MAGIC: [u8; 8] = *b"MCBEUI01";
pub const UI_CARRIER_VERSION: u32 = 1;
pub const MAX_UI_ATLAS_PAGES: usize = 16;
/// Bound on an atlas page's width and height in pixels.
pub const MAX_UI_ATLAS_SIDE: u32 = 4096;
pub const MAX_UI_TEXTURES: usize = 8192;
pub const MAX_UI_SIDECARS: usize = 8192;
pub const MAX_UI_FILES: usize = 4096;
/// Package data the credits renderer loads alongside the JSON-UI definitions.
pub const UI_CREDITS_FILES: [&str; 3] = [
    "credits/end.txt",
    "credits/credits.json",
    "credits/quote.txt",
];
/// Bound on a texture placement or sidecar or ui-file logical key.
pub const MAX_UI_KEY_BYTES: usize = 512;
/// Bound on one stored raw ui-json file.
pub const MAX_UI_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_UI_CARRIER_BYTES: usize = 96 * 1024 * 1024;

const HEADER_BYTES: usize = 72;
const HASH_BYTES: usize = 32;
const NINE_SLICE_PRESENT: u8 = 1;

/// One baked atlas page: raw RGBA8, `width * height * 4` bytes, row-major.
#[derive(Clone, Eq, PartialEq)]
pub struct UiAtlasPage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

impl std::fmt::Debug for UiAtlasPage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UiAtlasPage")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// Where one `textures/ui/...` texture lives in the atlas: its page and pixel rect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiTexturePlacement {
    pub path: Box<str>,
    pub page: u16,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// Nine-slice insets in source pixels; a zero inset collapses that border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiNineSlice {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// A texture's native size and optional nine-slice split, from its sidecar json.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiSidecar {
    pub base_size: [f32; 2],
    pub nineslice: Option<UiNineSlice>,
}

/// A raw ui-json file kept verbatim, keyed by its pack-relative path.
#[derive(Clone, Eq, PartialEq)]
pub struct UiFile {
    pub path: Box<str>,
    pub bytes: Arc<[u8]>,
}

impl std::fmt::Debug for UiFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UiFile")
            .field("path", &self.path)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// One sidecar record paired with its logical texture path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiSidecarEntry<'a> {
    pub path: &'a str,
    pub meta: UiSidecar,
}

/// A texture's normalized atlas location: its page index and 0..1 UV sub-rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTextureUv {
    pub page: usize,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// Decoded, validated JSON-UI carrier. Placements, sidecars, and files are each
/// sorted by path for binary-search lookup.
#[derive(Clone)]
pub struct RuntimeUiAssets {
    source_manifest_sha256: [u8; 32],
    pages: Arc<[UiAtlasPage]>,
    textures: Arc<[UiTexturePlacement]>,
    sidecar_paths: Arc<[Box<str>]>,
    sidecar_meta: Arc<[UiSidecar]>,
    files: Arc<[UiFile]>,
}

impl std::fmt::Debug for RuntimeUiAssets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeUiAssets")
            .field("pages", &self.pages.len())
            .field("textures", &self.textures.len())
            .field("sidecars", &self.sidecar_paths.len())
            .field("files", &self.files.len())
            .finish_non_exhaustive()
    }
}

impl RuntimeUiAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() > MAX_UI_CARRIER_BYTES {
            return Err(invalid("ui carrier exceeds bound"));
        }
        if bytes.len() < HEADER_BYTES + HASH_BYTES
            || bytes[..8] != UI_CARRIER_MAGIC
            || read_u32(bytes, 8)? != UI_CARRIER_VERSION
        {
            return Err(invalid("unsupported ui carrier header"));
        }
        let page_count = read_u32(bytes, 12)? as usize;
        let texture_count = read_u32(bytes, 16)? as usize;
        let sidecar_count = read_u32(bytes, 20)? as usize;
        let file_count = read_u32(bytes, 24)? as usize;
        if read_u32(bytes, 28)? != 0 {
            return Err(invalid("noncanonical ui carrier padding"));
        }
        let source_manifest_sha256 = read_array::<32>(bytes, 32)?;
        let payload_end = read_usize(bytes, 64)?;
        if page_count > MAX_UI_ATLAS_PAGES
            || texture_count > MAX_UI_TEXTURES
            || sidecar_count > MAX_UI_SIDECARS
            || file_count > MAX_UI_FILES
            || source_manifest_sha256 == [0; 32]
            || payload_end < HEADER_BYTES
            || bytes.len()
                != payload_end
                    .checked_add(HASH_BYTES)
                    .ok_or_else(|| invalid("ui carrier length overflow"))?
        {
            return Err(invalid("noncanonical ui carrier layout"));
        }
        if Sha256::digest(&bytes[..payload_end]).as_slice() != &bytes[payload_end..] {
            return Err(invalid("ui carrier envelope hash mismatch"));
        }

        let mut cursor = HEADER_BYTES;
        let pages = decode_pages(bytes, &mut cursor, page_count, payload_end)?;
        let textures = decode_textures(bytes, &mut cursor, texture_count, payload_end, &pages)?;
        let (sidecar_paths, sidecar_meta) =
            decode_sidecars(bytes, &mut cursor, sidecar_count, payload_end)?;
        let files = decode_files(bytes, &mut cursor, file_count, payload_end)?;
        if cursor != payload_end {
            return Err(invalid("trailing ui carrier payload"));
        }
        Ok(Self {
            source_manifest_sha256,
            pages: pages.into(),
            textures: textures.into(),
            sidecar_paths: sidecar_paths.into(),
            sidecar_meta: sidecar_meta.into(),
            files: files.into(),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub fn atlas_pages(&self) -> &[UiAtlasPage] {
        &self.pages
    }

    #[must_use]
    pub fn textures(&self) -> &[UiTexturePlacement] {
        &self.textures
    }

    /// The atlas placement for one `textures/ui/...` logical path.
    #[must_use]
    pub fn texture(&self, path: &str) -> Option<&UiTexturePlacement> {
        self.textures
            .binary_search_by(|entry| entry.path.as_ref().cmp(path))
            .ok()
            .map(|index| &self.textures[index])
    }

    /// The page index and normalized UV sub-rect for one texture path.
    #[must_use]
    pub fn texture_uv(&self, path: &str) -> Option<UiTextureUv> {
        let placement = self.texture(path)?;
        let page = self.pages.get(placement.page as usize)?;
        let (pw, ph) = (page.width as f32, page.height as f32);
        Some(UiTextureUv {
            page: placement.page as usize,
            u0: placement.x as f32 / pw,
            v0: placement.y as f32 / ph,
            u1: (placement.x as u32 + placement.width as u32) as f32 / pw,
            v1: (placement.y as u32 + placement.height as u32) as f32 / ph,
        })
    }

    /// The native `base_size`/nine-slice for one texture path, from its sidecar.
    #[must_use]
    pub fn sidecar(&self, path: &str) -> Option<UiSidecar> {
        self.sidecar_paths
            .binary_search_by(|entry| entry.as_ref().cmp(path))
            .ok()
            .map(|index| self.sidecar_meta[index])
    }

    pub fn sidecars(&self) -> impl Iterator<Item = UiSidecarEntry<'_>> {
        self.sidecar_paths
            .iter()
            .zip(self.sidecar_meta.iter())
            .map(|(path, meta)| UiSidecarEntry { path, meta: *meta })
    }

    #[must_use]
    pub fn ui_files(&self) -> &[UiFile] {
        &self.files
    }

    /// Raw bytes of one stored `ui/*.json` (or index) file, keyed by pack path.
    #[must_use]
    pub fn ui_file(&self, path: &str) -> Option<&[u8]> {
        self.files
            .binary_search_by(|entry| entry.path.as_ref().cmp(path))
            .ok()
            .map(|index| self.files[index].bytes.as_ref())
    }
}

fn decode_pages(
    bytes: &[u8],
    cursor: &mut usize,
    count: usize,
    payload_end: usize,
) -> Result<Vec<UiAtlasPage>, AssetError> {
    let mut pages = Vec::with_capacity(count);
    for _ in 0..count {
        let width = u32::from(read_u16(bytes, *cursor)?);
        let height = u32::from(read_u16(bytes, *cursor + 2)?);
        *cursor += 4;
        if width == 0 || height == 0 || width > MAX_UI_ATLAS_SIDE || height > MAX_UI_ATLAS_SIDE {
            return Err(invalid("ui atlas page dimensions exceed bounds"));
        }
        let pixel_bytes = pixel_length(width, height)?;
        let end = cursor
            .checked_add(pixel_bytes)
            .filter(|end| *end <= payload_end)
            .ok_or_else(|| invalid("ui atlas page runs past the payload"))?;
        pages.push(UiAtlasPage {
            width,
            height,
            rgba8: Arc::from(&bytes[*cursor..end]),
        });
        *cursor = end;
    }
    Ok(pages)
}

fn decode_textures(
    bytes: &[u8],
    cursor: &mut usize,
    count: usize,
    payload_end: usize,
    pages: &[UiAtlasPage],
) -> Result<Vec<UiTexturePlacement>, AssetError> {
    let mut textures: Vec<UiTexturePlacement> = Vec::with_capacity(count);
    for _ in 0..count {
        let path = read_key(bytes, cursor, payload_end)?;
        let page = read_u16(bytes, *cursor)?;
        let x = read_u16(bytes, *cursor + 2)?;
        let y = read_u16(bytes, *cursor + 4)?;
        let width = read_u16(bytes, *cursor + 6)?;
        let height = read_u16(bytes, *cursor + 8)?;
        *cursor += 10;
        let atlas = pages
            .get(page as usize)
            .ok_or_else(|| invalid("ui texture references a missing atlas page"))?;
        if width == 0
            || height == 0
            || u32::from(x) + u32::from(width) > atlas.width
            || u32::from(y) + u32::from(height) > atlas.height
        {
            return Err(invalid("ui texture placement is out of page bounds"));
        }
        if textures
            .last()
            .is_some_and(|previous| previous.path.as_ref() >= path.as_str())
        {
            return Err(invalid("ui texture placements are not strictly sorted"));
        }
        textures.push(UiTexturePlacement {
            path: path.into(),
            page,
            x,
            y,
            width,
            height,
        });
    }
    Ok(textures)
}

fn decode_sidecars(
    bytes: &[u8],
    cursor: &mut usize,
    count: usize,
    payload_end: usize,
) -> Result<(Vec<Box<str>>, Vec<UiSidecar>), AssetError> {
    let mut paths: Vec<Box<str>> = Vec::with_capacity(count);
    let mut meta = Vec::with_capacity(count);
    for _ in 0..count {
        let path = read_key(bytes, cursor, payload_end)?;
        let base_size = [read_f32(bytes, *cursor)?, read_f32(bytes, *cursor + 4)?];
        let flags = *bytes
            .get(*cursor + 8)
            .ok_or_else(|| invalid("truncated ui sidecar flags"))?;
        *cursor += 9;
        let nineslice = match flags {
            0 => None,
            NINE_SLICE_PRESENT => {
                let slice = UiNineSlice {
                    left: read_f32(bytes, *cursor)?,
                    top: read_f32(bytes, *cursor + 4)?,
                    right: read_f32(bytes, *cursor + 8)?,
                    bottom: read_f32(bytes, *cursor + 12)?,
                };
                *cursor += 16;
                Some(slice)
            }
            _ => return Err(invalid("invalid ui sidecar flags")),
        };
        if paths
            .last()
            .is_some_and(|previous| previous.as_ref() >= path.as_str())
        {
            return Err(invalid("ui sidecars are not strictly sorted"));
        }
        if !sidecar_is_finite(base_size, nineslice) {
            return Err(invalid("ui sidecar has non-finite metric"));
        }
        paths.push(path.into());
        meta.push(UiSidecar {
            base_size,
            nineslice,
        });
    }
    Ok((paths, meta))
}

fn decode_files(
    bytes: &[u8],
    cursor: &mut usize,
    count: usize,
    payload_end: usize,
) -> Result<Vec<UiFile>, AssetError> {
    let mut files: Vec<UiFile> = Vec::with_capacity(count);
    for _ in 0..count {
        let path = read_key(bytes, cursor, payload_end)?;
        let length = read_u32(bytes, *cursor)? as usize;
        *cursor += 4;
        if length > MAX_UI_FILE_BYTES {
            return Err(invalid("ui file exceeds byte bound"));
        }
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= payload_end)
            .ok_or_else(|| invalid("ui file runs past the payload"))?;
        if files
            .last()
            .is_some_and(|previous| previous.path.as_ref() >= path.as_str())
        {
            return Err(invalid("ui files are not strictly sorted"));
        }
        files.push(UiFile {
            path: path.into(),
            bytes: Arc::from(&bytes[*cursor..end]),
        });
        *cursor = end;
    }
    Ok(files)
}

/// Encode packed pages, sorted placements, sorted sidecars, and sorted files
/// into the canonical hash-pinned carrier bytes.
pub fn encode_ui_catalog(
    source_manifest_sha256: [u8; 32],
    pages: &[UiAtlasPage],
    textures: &[UiTexturePlacement],
    sidecars: &[(Box<str>, UiSidecar)],
    files: &[UiFile],
) -> Result<Vec<u8>, AssetError> {
    if source_manifest_sha256 == [0; 32] {
        return Err(invalid("ui carrier provenance is unset"));
    }
    if pages.len() > MAX_UI_ATLAS_PAGES
        || textures.len() > MAX_UI_TEXTURES
        || sidecars.len() > MAX_UI_SIDECARS
        || files.len() > MAX_UI_FILES
    {
        return Err(invalid("ui carrier record count exceeds bound"));
    }

    let mut payload = Vec::new();
    for page in pages {
        if page.width == 0
            || page.height == 0
            || page.width > MAX_UI_ATLAS_SIDE
            || page.height > MAX_UI_ATLAS_SIDE
            || page.rgba8.len() != pixel_length(page.width, page.height)?
        {
            return Err(invalid("ui atlas page dimensions or pixels exceed bounds"));
        }
        crate::encoding::append_bounded(
            &mut payload,
            &(page.width as u16).to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &(page.height as u16).to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &page.rgba8,
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
    }

    let mut previous: Option<&str> = None;
    for placement in textures {
        let atlas = pages
            .get(placement.page as usize)
            .ok_or_else(|| invalid("ui texture references a missing atlas page"))?;
        if placement.width == 0
            || placement.height == 0
            || u32::from(placement.x) + u32::from(placement.width) > atlas.width
            || u32::from(placement.y) + u32::from(placement.height) > atlas.height
        {
            return Err(invalid("ui texture placement is out of page bounds"));
        }
        write_key(&mut payload, &placement.path)?;
        if previous.is_some_and(|previous| previous >= placement.path.as_ref()) {
            return Err(invalid("ui texture placements are not strictly sorted"));
        }
        crate::encoding::append_bounded(
            &mut payload,
            &placement.page.to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &placement.x.to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &placement.y.to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &placement.width.to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &placement.height.to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        previous = Some(&placement.path);
    }

    let mut previous: Option<&str> = None;
    for (path, meta) in sidecars {
        if !sidecar_is_finite(meta.base_size, meta.nineslice) {
            return Err(invalid("ui sidecar has non-finite metric"));
        }
        write_key(&mut payload, path)?;
        if previous.is_some_and(|previous| previous >= path.as_ref()) {
            return Err(invalid("ui sidecars are not strictly sorted"));
        }
        crate::encoding::append_bounded(
            &mut payload,
            &meta.base_size[0].to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &meta.base_size[1].to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        match meta.nineslice {
            None => payload.push(0),
            Some(slice) => {
                payload.push(NINE_SLICE_PRESENT);
                crate::encoding::append_bounded(
                    &mut payload,
                    &slice.left.to_le_bytes(),
                    MAX_UI_CARRIER_BYTES,
                    HEADER_BYTES + HASH_BYTES,
                )
                .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
                crate::encoding::append_bounded(
                    &mut payload,
                    &slice.top.to_le_bytes(),
                    MAX_UI_CARRIER_BYTES,
                    HEADER_BYTES + HASH_BYTES,
                )
                .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
                crate::encoding::append_bounded(
                    &mut payload,
                    &slice.right.to_le_bytes(),
                    MAX_UI_CARRIER_BYTES,
                    HEADER_BYTES + HASH_BYTES,
                )
                .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
                crate::encoding::append_bounded(
                    &mut payload,
                    &slice.bottom.to_le_bytes(),
                    MAX_UI_CARRIER_BYTES,
                    HEADER_BYTES + HASH_BYTES,
                )
                .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
            }
        }
        previous = Some(path);
    }

    let mut previous: Option<&str> = None;
    for file in files {
        if file.bytes.len() > MAX_UI_FILE_BYTES {
            return Err(invalid("ui file exceeds byte bound"));
        }
        write_key(&mut payload, &file.path)?;
        if previous.is_some_and(|previous| previous >= file.path.as_ref()) {
            return Err(invalid("ui files are not strictly sorted"));
        }
        crate::encoding::append_bounded(
            &mut payload,
            &(file.bytes.len() as u32).to_le_bytes(),
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        crate::encoding::append_bounded(
            &mut payload,
            &file.bytes,
            MAX_UI_CARRIER_BYTES,
            HEADER_BYTES + HASH_BYTES,
        )
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
        previous = Some(&file.path);
    }

    let payload_end = HEADER_BYTES
        .checked_add(payload.len())
        .filter(|end| end + HASH_BYTES <= MAX_UI_CARRIER_BYTES)
        .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
    let mut bytes = vec![0u8; HEADER_BYTES];
    bytes[..8].copy_from_slice(&UI_CARRIER_MAGIC);
    bytes[8..12].copy_from_slice(&UI_CARRIER_VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(pages.len() as u32).to_le_bytes());
    bytes[16..20].copy_from_slice(&(textures.len() as u32).to_le_bytes());
    bytes[20..24].copy_from_slice(&(sidecars.len() as u32).to_le_bytes());
    bytes[24..28].copy_from_slice(&(files.len() as u32).to_le_bytes());
    bytes[32..64].copy_from_slice(&source_manifest_sha256);
    bytes[64..72].copy_from_slice(&(payload_end as u64).to_le_bytes());
    bytes.extend_from_slice(&payload);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

fn sidecar_is_finite(base_size: [f32; 2], nineslice: Option<UiNineSlice>) -> bool {
    base_size.iter().all(|value| value.is_finite())
        && nineslice.is_none_or(|slice| {
            [slice.left, slice.top, slice.right, slice.bottom]
                .iter()
                .all(|value| value.is_finite())
        })
}

fn read_key(bytes: &[u8], cursor: &mut usize, payload_end: usize) -> Result<String, AssetError> {
    let length = usize::from(read_u16(bytes, *cursor)?);
    *cursor += 2;
    if length == 0 || length > MAX_UI_KEY_BYTES {
        return Err(invalid("ui carrier key length is out of bounds"));
    }
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= payload_end)
        .ok_or_else(|| invalid("ui carrier key runs past the payload"))?;
    let key = std::str::from_utf8(&bytes[*cursor..end])
        .map_err(|_| invalid("ui carrier key is not UTF-8"))?
        .to_owned();
    *cursor = end;
    Ok(key)
}

fn write_key(payload: &mut Vec<u8>, key: &str) -> Result<(), AssetError> {
    if key.is_empty() || key.len() > MAX_UI_KEY_BYTES {
        return Err(invalid("ui carrier key length is out of bounds"));
    }
    crate::encoding::append_bounded(
        payload,
        &(key.len() as u16).to_le_bytes(),
        MAX_UI_CARRIER_BYTES,
        HEADER_BYTES + HASH_BYTES,
    )
    .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
    crate::encoding::append_bounded(
        payload,
        key.as_bytes(),
        MAX_UI_CARRIER_BYTES,
        HEADER_BYTES + HASH_BYTES,
    )
    .ok_or_else(|| invalid("ui carrier exceeds bound"))?;
    Ok(())
}

fn pixel_length(width: u32, height: u32) -> Result<usize, AssetError> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| invalid("ui atlas pixel length overflow"))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AssetError> {
    Ok(u16::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AssetError> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, AssetError> {
    Ok(f32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_usize(bytes: &[u8], offset: usize) -> Result<usize, AssetError> {
    usize::try_from(u64::from_le_bytes(read_array(bytes, offset)?))
        .map_err(|_| invalid("ui carrier offset exceeds platform"))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or_else(|| invalid("ui carrier field overflow"))?,
        )
        .ok_or_else(|| invalid("truncated ui carrier field"))?
        .try_into()
        .map_err(|_| invalid("invalid ui carrier field"))
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(width: u32, height: u32, fill: u8) -> UiAtlasPage {
        UiAtlasPage {
            width,
            height,
            rgba8: Arc::from(vec![fill; (width * height * 4) as usize]),
        }
    }

    type Sample = (
        [u8; 32],
        Vec<UiAtlasPage>,
        Vec<UiTexturePlacement>,
        Vec<(Box<str>, UiSidecar)>,
        Vec<UiFile>,
    );

    fn sample() -> Sample {
        let pages = vec![page(16, 16, 7)];
        let textures = vec![
            UiTexturePlacement {
                path: "textures/ui/button".into(),
                page: 0,
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            UiTexturePlacement {
                path: "textures/ui/panel".into(),
                page: 0,
                x: 8,
                y: 0,
                width: 8,
                height: 8,
            },
        ];
        let sidecars = vec![
            (
                "textures/ui/button".into(),
                UiSidecar {
                    base_size: [8.0, 8.0],
                    nineslice: Some(UiNineSlice {
                        left: 2.0,
                        top: 2.0,
                        right: 2.0,
                        bottom: 2.0,
                    }),
                },
            ),
            (
                "textures/ui/panel".into(),
                UiSidecar {
                    base_size: [8.0, 8.0],
                    nineslice: None,
                },
            ),
        ];
        let files = vec![
            UiFile {
                path: "ui/_ui_defs.json".into(),
                bytes: Arc::from(br#"{"ui_defs":["ui/hud.json"]}"#.to_vec()),
            },
            UiFile {
                path: "ui/hud.json".into(),
                bytes: Arc::from(br#"{"namespace":"hud"}"#.to_vec()),
            },
        ];
        ([9u8; 32], pages, textures, sidecars, files)
    }

    #[test]
    fn round_trips_pages_placements_sidecars_and_files() {
        let (manifest, pages, textures, sidecars, files) = sample();
        let bytes = encode_ui_catalog(manifest, &pages, &textures, &sidecars, &files).unwrap();
        let assets = RuntimeUiAssets::decode(&bytes).unwrap();
        assert_eq!(assets.source_manifest_sha256(), manifest);
        assert_eq!(assets.atlas_pages().len(), 1);
        assert_eq!(assets.atlas_pages()[0].rgba8.len(), 16 * 16 * 4);

        let placement = assets.texture("textures/ui/panel").unwrap();
        assert_eq!((placement.page, placement.x, placement.width), (0, 8, 8));
        let uv = assets.texture_uv("textures/ui/panel").unwrap();
        assert_eq!((uv.u0, uv.u1), (0.5, 1.0));
        assert!(assets.texture("textures/ui/missing").is_none());

        let sidecar = assets.sidecar("textures/ui/button").unwrap();
        assert_eq!(sidecar.nineslice.unwrap().left, 2.0);
        assert!(
            assets
                .sidecar("textures/ui/panel")
                .unwrap()
                .nineslice
                .is_none()
        );

        assert_eq!(
            assets.ui_file("ui/hud.json").unwrap(),
            br#"{"namespace":"hud"}"#
        );
        assert_eq!(assets.ui_files().len(), 2);
        assert_eq!(assets.sidecars().count(), 2);
    }

    #[test]
    fn rejects_unsorted_records_and_zero_provenance() {
        let (manifest, pages, mut textures, sidecars, files) = sample();
        textures.reverse();
        assert!(encode_ui_catalog(manifest, &pages, &textures, &sidecars, &files).is_err());

        let (_, pages, textures, sidecars, files) = sample();
        assert!(encode_ui_catalog([0; 32], &pages, &textures, &sidecars, &files).is_err());
    }

    #[test]
    fn rejects_placement_outside_its_page() {
        let (manifest, pages, mut textures, sidecars, files) = sample();
        textures[0].width = 64; // runs past the 16px page
        assert!(encode_ui_catalog(manifest, &pages, &textures, &sidecars, &files).is_err());
    }

    #[test]
    fn rejects_tampered_envelope_and_header() {
        let (manifest, pages, textures, sidecars, files) = sample();
        let mut bytes = encode_ui_catalog(manifest, &pages, &textures, &sidecars, &files).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        assert!(RuntimeUiAssets::decode(&bytes).is_err());

        let mut versioned =
            encode_ui_catalog(manifest, &pages, &textures, &sidecars, &files).unwrap();
        versioned[8] = 0x02; // unsupported version
        assert!(RuntimeUiAssets::decode(&versioned).is_err());
    }
}
