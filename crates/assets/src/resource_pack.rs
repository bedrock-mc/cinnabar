use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    path::Path,
};

use png::{ColorType, Decoder, Transformations};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::ZipArchive;

use crate::font::{
    CompiledFontCatalog, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog,
};
use crate::{MAX_LANGUAGE_ENTRIES, MAX_LANGUAGE_TOTAL_BYTES, parse_language_bytes};

#[path = "resource_pack_blocks.rs"]
mod resource_pack_blocks;
#[path = "resource_pack_encryption.rs"]
mod resource_pack_encryption;
#[path = "resource_pack_items.rs"]
mod resource_pack_items;
use resource_pack_blocks::merge_block_texture_manifest;
use resource_pack_encryption::decrypt_pack_entries;
pub use resource_pack_items::ServerResourcePackItemIcon;
use resource_pack_items::{
    canonical_texture_path, decode_item_icons, decode_pack_texture, merge_item_texture_manifest,
    normalize_texture_tile,
};

const MAX_PACK_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PACK_ENTRIES: usize = 8_192;
const MAX_PACK_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PACK_UNCOMPRESSED_BYTES: u64 = 128 * 1024 * 1024;
const GLYPH_GRID_SIDE: u32 = 16;
const MAX_ITEM_ICON_COUNT: usize = 16_384;
const MAX_ITEM_ICON_SIDE: u32 = 256;
const MAX_ITEM_ICON_DECODED_BYTES: usize = 4 * 1024 * 1024;
type EffectiveBlockRoutes = Box<[(Box<str>, u32, Box<str>)]>;

/// A borrowed, already-verified resource-pack archive from the login layer.
/// The slice must contain the compressed archive bytes and is never retained
/// after [`compile_server_resource_packs`] returns.
#[derive(Clone, Copy, Debug)]
pub struct ServerResourcePackInput<'a> {
    pub uuid: &'a str,
    pub version: &'a str,
    pub name: &'a str,
    pub content_key: &'a str,
    pub bytes: &'a [u8],
}

/// Runtime data extracted from the ordered server resource-pack stack.
///
/// The font overlay, translation map, item atlas, and block texture routes are
/// consumed by the UI/chunk runtimes. Later packs replace earlier entries at
/// the same logical path or manifest key, matching server stack precedence.
#[derive(Clone, Debug, Default)]
pub struct ServerResourcePackCatalog {
    font_overlay: Option<RuntimeFontCatalog>,
    translations: BTreeMap<Box<str>, Box<str>>,
    item_texture_paths: Box<[Box<str>]>,
    block_texture_paths: Box<[Box<str>]>,
    block_texture_routes: EffectiveBlockRoutes,
    item_texture_assets: Box<[ServerResourcePackAsset]>,
    block_texture_assets: Box<[ServerResourcePackAsset]>,
    item_icons: Box<[ServerResourcePackItemIcon]>,
    ignored_encrypted_packs: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerResourcePackAsset {
    path: Box<str>,
    bytes: Box<[u8]>,
}

impl ServerResourcePackAsset {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl ServerResourcePackCatalog {
    pub fn font_overlay(&self) -> Option<&RuntimeFontCatalog> {
        self.font_overlay.as_ref()
    }

    pub fn translations(&self) -> &BTreeMap<Box<str>, Box<str>> {
        &self.translations
    }

    pub fn item_texture_paths(&self) -> &[Box<str>] {
        &self.item_texture_paths
    }

    pub fn block_texture_paths(&self) -> &[Box<str>] {
        &self.block_texture_paths
    }

    /// Returns the effective server texture path for a terrain manifest route.
    #[must_use]
    pub fn block_texture_path(&self, key: &str, variant: u32) -> Option<&str> {
        self.block_texture_routes
            .binary_search_by(|(route_key, route_variant, _)| {
                route_key
                    .as_ref()
                    .cmp(key)
                    .then(route_variant.cmp(&variant))
            })
            .ok()
            .map(|index| self.block_texture_routes[index].2.as_ref())
    }

    /// Decodes one effective server block texture and normalizes it to the
    /// 16x16 RGBA8 tile shape required by the chunk texture arrays.
    pub fn block_texture_tile(
        &self,
        path: &str,
    ) -> Result<Option<Box<[u8]>>, ServerResourcePackError> {
        let Some(path) = canonical_texture_path(path) else {
            return Ok(None);
        };
        let Ok(index) = self
            .block_texture_assets
            .binary_search_by(|asset| asset.path().cmp(path.as_ref()))
        else {
            return Ok(None);
        };
        let asset = &self.block_texture_assets[index];
        let (width, height, rgba8) =
            decode_pack_texture(asset.path(), asset.bytes()).map_err(|detail| {
                ServerResourcePackError::BlockTextureDecode {
                    pack: "resource-pack-stack".into(),
                    path: asset.path.clone(),
                    detail,
                }
            })?;
        normalize_texture_tile(width, height, &rgba8)
            .map(Some)
            .map_err(|detail| ServerResourcePackError::BlockTextureDecode {
                pack: "resource-pack-stack".into(),
                path: asset.path.clone(),
                detail,
            })
    }

    pub fn item_texture_assets(&self) -> &[ServerResourcePackAsset] {
        &self.item_texture_assets
    }

    pub fn block_texture_assets(&self) -> &[ServerResourcePackAsset] {
        &self.block_texture_assets
    }

    pub fn item_icons(&self) -> &[ServerResourcePackItemIcon] {
        &self.item_icons
    }

    pub fn ignored_encrypted_packs(&self) -> usize {
        self.ignored_encrypted_packs
    }
}

#[derive(Debug, Error)]
pub enum ServerResourcePackError {
    #[error("resource pack {pack} exceeds the {limit} byte archive limit")]
    ArchiveTooLarge { pack: Box<str>, limit: usize },
    #[error("resource pack {pack} has too many archive entries")]
    TooManyEntries { pack: Box<str>, limit: usize },
    #[error("resource pack {pack} contains an unsafe archive path")]
    UnsafePath { pack: Box<str> },
    #[error("resource pack {pack} entry {path} exceeds the {limit} byte entry limit")]
    EntryTooLarge {
        pack: Box<str>,
        path: Box<str>,
        limit: u64,
    },
    #[error("resource pack {pack} exceeds its uncompressed byte limit")]
    UncompressedTooLarge { pack: Box<str>, limit: u64 },
    #[error("resource pack {pack} archive is invalid: {source}")]
    Archive {
        pack: Box<str>,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("resource pack {pack} encrypted contents are invalid: {detail}")]
    EncryptedPack { pack: Box<str>, detail: Box<str> },
    #[error("resource pack {pack} file {path} has an invalid translation: {detail}")]
    Translation {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
    #[error("resource pack {pack} glyph page {path} is invalid: {detail}")]
    GlyphPage {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
    #[error("resource pack {pack} glyph page {path} could not be decoded: {detail}")]
    GlyphDecode {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
    #[error("resource pack {pack} font overlay is invalid: {source}")]
    FontCatalog {
        pack: Box<str>,
        #[source]
        source: crate::FontCatalogError,
    },
    #[error("resource pack stack could not be merged: {source}")]
    Merge {
        #[source]
        source: crate::FontCatalogError,
    },
    #[error("resource pack {pack} asset {path} cannot be retained: {detail}")]
    TextureAsset {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
    #[error("resource pack {pack} item texture manifest {path} is invalid: {detail}")]
    ItemTextureManifest {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
    #[error("resource pack {pack} item texture {path} could not be decoded: {detail}")]
    ItemTextureDecode {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
    #[error("resource pack {pack} block texture {path} could not be decoded: {detail}")]
    BlockTextureDecode {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
    #[error("resource pack {pack} terrain texture manifest {path} is invalid: {detail}")]
    BlockTextureManifest {
        pack: Box<str>,
        path: Box<str>,
        detail: Box<str>,
    },
}

#[derive(Debug)]
struct PackEntry {
    path: Box<str>,
    bytes: Box<[u8]>,
}

/// Extracts bounded, vanilla-compatible text and glyph assets from a server
/// pack stack. The input order is the server-selected order: later packs take
/// precedence over earlier packs for duplicate translation keys and glyphs.
pub fn compile_server_resource_packs(
    base_font: &RuntimeFontCatalog,
    packs: &[ServerResourcePackInput<'_>],
) -> Result<ServerResourcePackCatalog, ServerResourcePackError> {
    let mut catalog = ServerResourcePackCatalog::default();
    let mut item_paths = BTreeSet::new();
    let mut block_paths = BTreeSet::new();
    let mut item_assets = BTreeMap::new();
    let mut block_assets = BTreeMap::new();
    let mut item_routes = BTreeMap::<(Box<str>, u32), (Box<str>, Box<str>)>::new();
    let mut block_routes = BTreeMap::<(Box<str>, u32), (Box<str>, Box<str>)>::new();
    let mut retained_texture_bytes = 0usize;
    let mut translation_bytes = 0usize;

    for pack in packs {
        let pack_label = pack_label(pack);
        let entries = match read_pack_entries(pack, &pack_label) {
            Ok(entries) => entries,
            Err(_error) if !pack.content_key.is_empty() => {
                catalog.ignored_encrypted_packs += 1;
                continue;
            }
            Err(error) => return Err(error),
        };
        let mut pack_translations = BTreeMap::new();
        for entry in &entries {
            if entry.path.starts_with("texts/") && entry.path.ends_with(".lang") {
                let locale = entry
                    .path
                    .rsplit('/')
                    .next()
                    .and_then(|value| value.strip_suffix(".lang"))
                    .unwrap_or_default();
                if locale == "en_US" {
                    pack_translations = parse_language_bytes(&entry.bytes).map_err(|source| {
                        ServerResourcePackError::Translation {
                            pack: pack_label.clone(),
                            path: entry.path.clone(),
                            detail: source.to_string().into_boxed_str(),
                        }
                    })?;
                }
            }
            if (entry.path.starts_with("textures/items/")
                || entry.path.starts_with("textures/item/"))
                && is_texture_asset_path(&entry.path)
            {
                item_paths.insert(entry.path.clone());
                insert_effective_asset(
                    &mut item_assets,
                    &mut retained_texture_bytes,
                    &pack_label,
                    entry,
                )?;
            }
            if (entry.path.starts_with("textures/blocks/")
                || entry.path.starts_with("textures/block/"))
                && is_texture_asset_path(&entry.path)
            {
                block_paths.insert(entry.path.clone());
                insert_effective_asset(
                    &mut block_assets,
                    &mut retained_texture_bytes,
                    &pack_label,
                    entry,
                )?;
            }
            if entry.path.as_ref() == "textures/item_texture.json" {
                item_paths.insert(entry.path.clone());
                insert_effective_asset(
                    &mut item_assets,
                    &mut retained_texture_bytes,
                    &pack_label,
                    entry,
                )?;
                merge_item_texture_manifest(&pack_label, &entry.bytes, &mut item_routes)?;
            }
            if entry.path.as_ref() == "textures/terrain_texture.json" {
                block_paths.insert(entry.path.clone());
                insert_effective_asset(
                    &mut block_assets,
                    &mut retained_texture_bytes,
                    &pack_label,
                    entry,
                )?;
                merge_block_texture_manifest(&pack_label, &entry.bytes, &mut block_routes)?;
            }
        }
        let referenced_paths = block_routes
            .values()
            .filter(|(route_pack, _)| route_pack.as_ref() == pack_label.as_ref())
            .map(|(_, path)| path.clone())
            .collect::<BTreeSet<_>>();
        for path in referenced_paths {
            if block_assets.contains_key(&path) {
                continue;
            }
            let Some(entry) = entries.iter().find(|entry| entry.path == path) else {
                continue;
            };
            block_paths.insert(path);
            insert_effective_asset(
                &mut block_assets,
                &mut retained_texture_bytes,
                &pack_label,
                entry,
            )?;
        }
        for (key, value) in pack_translations {
            if let Some(previous) = catalog.translations.get(&key) {
                translation_bytes =
                    translation_bytes.saturating_sub(previous.len().saturating_add(key.len()));
            } else if catalog.translations.len() >= MAX_LANGUAGE_ENTRIES {
                return Err(ServerResourcePackError::Translation {
                    pack: pack_label.clone(),
                    path: "texts/en_US.lang".into(),
                    detail: "merged translation catalog exceeds its entry limit".into(),
                });
            }
            translation_bytes = translation_bytes
                .checked_add(key.len())
                .and_then(|total| total.checked_add(value.len()))
                .ok_or_else(|| ServerResourcePackError::Translation {
                    pack: pack_label.clone(),
                    path: "texts/en_US.lang".into(),
                    detail: "merged translation catalog size overflows usize".into(),
                })?;
            if translation_bytes > MAX_LANGUAGE_TOTAL_BYTES {
                return Err(ServerResourcePackError::Translation {
                    pack: pack_label.clone(),
                    path: "texts/en_US.lang".into(),
                    detail: "merged translation catalog exceeds its byte limit".into(),
                });
            }
            catalog.translations.insert(key, value);
        }

        if let Some(overlay) = compile_font_overlay(base_font, &pack_label, &entries)? {
            catalog.font_overlay = Some(match catalog.font_overlay.take() {
                Some(previous) => previous
                    .merge_overlay(&overlay)
                    .map_err(|source| ServerResourcePackError::Merge { source })?,
                None => overlay,
            });
        }
    }

    catalog.item_texture_paths = item_paths.into_iter().collect();
    catalog.block_texture_paths = block_paths.into_iter().collect();
    catalog.block_texture_routes = block_routes
        .into_iter()
        .map(|((key, variant), (_pack, path))| (key, variant, path))
        .collect();
    catalog.item_icons = decode_item_icons(&item_routes, &item_assets, &block_assets)?;
    catalog.item_texture_assets = item_assets
        .into_iter()
        .map(|(path, bytes)| ServerResourcePackAsset { path, bytes })
        .collect();
    catalog.block_texture_assets = block_assets
        .into_iter()
        .map(|(path, bytes)| ServerResourcePackAsset { path, bytes })
        .collect();
    Ok(catalog)
}

const MAX_RETAINED_TEXTURE_BYTES: usize = 128 * 1024 * 1024;

fn is_texture_asset_path(path: &str) -> bool {
    path.ends_with(".png") || path.ends_with(".tga") || path.ends_with(".json")
}

fn insert_effective_asset(
    assets: &mut BTreeMap<Box<str>, Box<[u8]>>,
    retained_bytes: &mut usize,
    pack: &str,
    entry: &PackEntry,
) -> Result<(), ServerResourcePackError> {
    if let Some(previous) = assets.get(&entry.path) {
        *retained_bytes = retained_bytes.saturating_sub(previous.len());
    }
    *retained_bytes = retained_bytes
        .checked_add(entry.bytes.len())
        .ok_or_else(|| ServerResourcePackError::TextureAsset {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "retained asset size overflows usize".into(),
        })?;
    if *retained_bytes > MAX_RETAINED_TEXTURE_BYTES {
        return Err(ServerResourcePackError::TextureAsset {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "retained item and block assets exceed their byte limit".into(),
        });
    }
    assets.insert(entry.path.clone(), entry.bytes.clone());
    Ok(())
}

fn pack_label(pack: &ServerResourcePackInput<'_>) -> Box<str> {
    format!("{}_{}", pack.uuid, pack.version).into_boxed_str()
}

fn read_pack_entries(
    pack: &ServerResourcePackInput<'_>,
    pack_label: &str,
) -> Result<Vec<PackEntry>, ServerResourcePackError> {
    if pack.bytes.len() > MAX_PACK_ARCHIVE_BYTES {
        return Err(ServerResourcePackError::ArchiveTooLarge {
            pack: pack_label.into(),
            limit: MAX_PACK_ARCHIVE_BYTES,
        });
    }
    let mut archive = ZipArchive::new(Cursor::new(pack.bytes)).map_err(|source| {
        ServerResourcePackError::Archive {
            pack: pack_label.into(),
            source,
        }
    })?;
    if archive.len() > MAX_PACK_ENTRIES {
        return Err(ServerResourcePackError::TooManyEntries {
            pack: pack_label.into(),
            limit: MAX_PACK_ENTRIES,
        });
    }
    let mut total_uncompressed = 0u64;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|source| ServerResourcePackError::Archive {
                pack: pack_label.into(),
                source,
            })?;
        if file.is_dir() {
            continue;
        }
        let path = canonical_path(file.enclosed_name().as_deref()).ok_or_else(|| {
            ServerResourcePackError::UnsafePath {
                pack: pack_label.into(),
            }
        })?;
        let declared_size = file.size();
        if declared_size > MAX_PACK_ENTRY_BYTES {
            return Err(ServerResourcePackError::EntryTooLarge {
                pack: pack_label.into(),
                path,
                limit: MAX_PACK_ENTRY_BYTES,
            });
        }
        let mut bytes = Vec::new();
        file.take(MAX_PACK_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| ServerResourcePackError::Translation {
                pack: pack_label.into(),
                path: path.clone(),
                detail: format!("could not read archive entry: {source}").into_boxed_str(),
            })?;
        if bytes.len() as u64 > MAX_PACK_ENTRY_BYTES {
            return Err(ServerResourcePackError::EntryTooLarge {
                pack: pack_label.into(),
                path,
                limit: MAX_PACK_ENTRY_BYTES,
            });
        }
        total_uncompressed = total_uncompressed.saturating_add(bytes.len() as u64);
        if total_uncompressed > MAX_PACK_UNCOMPRESSED_BYTES {
            return Err(ServerResourcePackError::UncompressedTooLarge {
                pack: pack_label.into(),
                limit: MAX_PACK_UNCOMPRESSED_BYTES,
            });
        }
        entries.push(PackEntry {
            path,
            bytes: bytes.into_boxed_slice(),
        });
    }
    if pack.content_key.is_empty() {
        Ok(entries)
    } else {
        decrypt_pack_entries(pack_label, pack.content_key, &mut entries)?;
        Ok(entries)
    }
}

fn canonical_path(path: Option<&Path>) -> Option<Box<str>> {
    let path = path?;
    let text = path.to_str()?.replace('\\', "/");
    let components = text.split('/').collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| component.is_empty() || *component == "." || *component == "..")
    {
        return None;
    }
    let start = components
        .iter()
        .position(|component| matches!(*component, "font" | "texts" | "textures"));
    let canonical = match start {
        Some(start) => components[start..].join("/"),
        None => text.clone(),
    };
    Some(canonical.into_boxed_str())
}

fn compile_font_overlay(
    base_font: &RuntimeFontCatalog,
    pack: &str,
    entries: &[PackEntry],
) -> Result<Option<CompiledFontCatalog>, ServerResourcePackError> {
    let mut pages = Vec::new();
    let mut glyph_sources = Vec::new();
    let mut seen_paths = BTreeSet::new();
    for entry in entries {
        if !is_active_glyph_page(&entry.path) {
            continue;
        }
        if !seen_paths.insert(entry.path.clone()) {
            return Err(ServerResourcePackError::GlyphPage {
                pack: pack.into(),
                path: entry.path.clone(),
                detail: "duplicate glyph page path".into(),
            });
        }
        let Some((page, page_glyphs)) = decode_glyph_page(base_font, pack, entry)? else {
            continue;
        };
        let page_path = page.source_path.clone();
        pages.push(page);
        for glyph in page_glyphs {
            glyph_sources.push((page_path.clone(), glyph));
        }
    }
    if pages.is_empty() || glyph_sources.is_empty() {
        return Ok(None);
    }
    pages.sort_by(|left, right| {
        (&left.source_path, left.source_sha256).cmp(&(&right.source_path, right.source_sha256))
    });
    let page_indices = pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.source_path.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut glyphs = Vec::with_capacity(glyph_sources.len());
    for (path, mut glyph) in glyph_sources {
        let Some(page_index) = page_indices.get(&path).copied() else {
            return Err(ServerResourcePackError::GlyphPage {
                pack: pack.into(),
                path,
                detail: "glyph page was not retained".into(),
            });
        };
        glyph.page = u16::try_from(page_index).map_err(|_| ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: path.clone(),
            detail: "glyph page index overflows metrics".into(),
        })?;
        glyphs.push(glyph);
    }
    glyphs.sort_by_key(|glyph| {
        (
            glyph.codepoint as u32,
            &pages[usize::from(glyph.page)].source_path,
            pages[usize::from(glyph.page)].source_sha256,
        )
    });
    for pair in glyphs.windows(2) {
        if pair[0].codepoint == pair[1].codepoint {
            return Err(ServerResourcePackError::GlyphPage {
                pack: pack.into(),
                path: "font/".into(),
                detail: "duplicate glyph codepoint".into(),
            });
        }
    }
    let bytes = encode_font_catalog(base_font.identity().source_manifest_sha256, &glyphs, &pages)
        .map_err(|source| ServerResourcePackError::FontCatalog {
        pack: pack.into(),
        source,
    })?;
    CompiledFontCatalog::decode(&bytes, base_font.identity().source_manifest_sha256)
        .map(Some)
        .map_err(|source| ServerResourcePackError::FontCatalog {
            pack: pack.into(),
            source,
        })
}

fn decode_glyph_page(
    _base_font: &RuntimeFontCatalog,
    pack: &str,
    entry: &PackEntry,
) -> Result<Option<(FontTexturePage, Vec<GlyphMetrics>)>, ServerResourcePackError> {
    let page_prefix =
        glyph_page_prefix(&entry.path).ok_or_else(|| ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "invalid glyph page name".into(),
        })?;
    let page_number =
        u32::from_str_radix(page_prefix, 16).map_err(|_| ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "glyph page name is not hexadecimal".into(),
        })?;
    let codepoint_base = if page_number <= 0x10ff {
        page_number.checked_shl(8)
    } else if page_number <= 0x10ffff && page_number & 0xff == 0 {
        Some(page_number)
    } else {
        None
    }
    .ok_or_else(|| ServerResourcePackError::GlyphPage {
        pack: pack.into(),
        path: entry.path.clone(),
        detail: "glyph page number is outside Unicode".into(),
    })?;

    let mut decoder = Decoder::new(Cursor::new(&entry.bytes));
    decoder.set_transformations(
        Transformations::EXPAND | Transformations::STRIP_16 | Transformations::ALPHA,
    );
    let mut reader = decoder
        .read_info()
        .map_err(|source| glyph_decode_error(pack, &entry.path, source))?;
    let width = reader.info().width;
    let height = reader.info().height;
    if width == 0
        || height == 0
        || width > crate::MAX_FONT_PAGE_SIDE
        || height > crate::MAX_FONT_PAGE_SIDE
        || width % GLYPH_GRID_SIDE != 0
        || height % GLYPH_GRID_SIDE != 0
    {
        return Err(ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "glyph page dimensions are not a bounded 16x16 grid".into(),
        });
    }
    let decoded_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "glyph page allocation overflow".into(),
        })?;
    if decoded_bytes > 64 * 1024 * 1024 {
        return Err(ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "glyph page allocation exceeds the decoder limit".into(),
        });
    }
    let output_size =
        reader
            .output_buffer_size()
            .ok_or_else(|| ServerResourcePackError::GlyphPage {
                pack: pack.into(),
                path: entry.path.clone(),
                detail: "glyph page output size overflows the decoder limit".into(),
            })?;
    if output_size > decoded_bytes {
        return Err(ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "glyph page output exceeds the RGBA8 allocation bound".into(),
        });
    }
    let mut decoded = vec![0; output_size];
    let output = reader
        .next_frame(&mut decoded)
        .map_err(|source| glyph_decode_error(pack, &entry.path, source))?;
    let rgba8 = rgba8_from_png(
        output.color_type,
        &decoded[..output.buffer_size()],
        width,
        height,
    )
    .ok_or_else(|| ServerResourcePackError::GlyphPage {
        pack: pack.into(),
        path: entry.path.clone(),
        detail: "glyph page did not decode to a supported 8-bit color format".into(),
    })?;
    let cell_width = width / GLYPH_GRID_SIDE;
    let cell_height = height / GLYPH_GRID_SIDE;
    if cell_width > 511 || cell_height > 511 {
        return Err(ServerResourcePackError::GlyphPage {
            pack: pack.into(),
            path: entry.path.clone(),
            detail: "glyph cells exceed representable metrics".into(),
        });
    }
    let mut page_glyphs = Vec::new();
    for cell in 0..256u32 {
        let cell_x = (cell % GLYPH_GRID_SIDE) * cell_width;
        let cell_y = (cell / GLYPH_GRID_SIDE) * cell_height;
        let mut bounds = None::<(u32, u32, u32, u32)>;
        for y in 0..cell_height {
            for x in 0..cell_width {
                let pixel = ((cell_y + y) * width + cell_x + x) as usize * 4;
                if rgba8[pixel + 3] == 0 {
                    continue;
                }
                bounds = Some(match bounds {
                    Some((min_x, min_y, max_x, max_y)) => (
                        min_x.min(x),
                        min_y.min(y),
                        max_x.max(x + 1),
                        max_y.max(y + 1),
                    ),
                    None => (x, y, x + 1, y + 1),
                });
            }
        }
        if bounds.is_none() {
            continue;
        }
        let codepoint = char::from_u32(codepoint_base + cell).ok_or_else(|| {
            ServerResourcePackError::GlyphPage {
                pack: pack.into(),
                path: entry.path.clone(),
                detail: "glyph page contains a non-Unicode codepoint".into(),
            }
        })?;
        let bearing_y =
            -(i16::try_from(cell_height).map_err(|_| ServerResourcePackError::GlyphPage {
                pack: pack.into(),
                path: entry.path.clone(),
                detail: "glyph cell height overflows metrics".into(),
            })?);
        page_glyphs.push(GlyphMetrics {
            codepoint,
            page: 0,
            uv: [
                u16::try_from(cell_x).unwrap_or(u16::MAX),
                u16::try_from(cell_y).unwrap_or(u16::MAX),
                u16::try_from(cell_x + cell_width).unwrap_or(u16::MAX),
                u16::try_from(cell_y + cell_height).unwrap_or(u16::MAX),
            ],
            bearing: [0, bearing_y],
            advance_64: i16::try_from(cell_width * 64).map_err(|_| {
                ServerResourcePackError::GlyphPage {
                    pack: pack.into(),
                    path: entry.path.clone(),
                    detail: "glyph cell advance overflows metrics".into(),
                }
            })?,
        });
    }
    if page_glyphs.is_empty() {
        return Ok(None);
    }
    let source_sha256 = Sha256::digest(&entry.bytes).into();
    let pixels_sha256 = Sha256::digest(&rgba8).into();
    let page = FontTexturePage {
        source_path: entry.path.clone(),
        source_bytes: u32::try_from(entry.bytes.len()).map_err(|_| {
            ServerResourcePackError::GlyphPage {
                pack: pack.into(),
                path: entry.path.clone(),
                detail: "glyph page source length overflows metrics".into(),
            }
        })?,
        source_sha256,
        pixels_sha256,
        width,
        height,
        rgba8: rgba8.into_boxed_slice(),
    };
    Ok(Some((page, page_glyphs)))
}

fn is_active_glyph_page(path: &str) -> bool {
    glyph_page_prefix(path).is_some()
}

fn glyph_page_prefix(path: &str) -> Option<&str> {
    let prefix = path
        .strip_prefix("font/glyph_")
        .or_else(|| path.strip_prefix("texts/en_US/font/glyph_"))?;
    prefix.strip_suffix(".png")
}

fn glyph_decode_error(
    pack: &str,
    path: &str,
    source: impl std::fmt::Display,
) -> ServerResourcePackError {
    ServerResourcePackError::GlyphDecode {
        pack: pack.into(),
        path: path.into(),
        detail: source.to_string().into_boxed_str(),
    }
}

fn rgba8_from_png(color_type: ColorType, bytes: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let pixels = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    let mut rgba8 = Vec::with_capacity(pixels.checked_mul(4)?);
    match color_type {
        ColorType::Rgba => {
            if bytes.len() != pixels.checked_mul(4)? {
                return None;
            }
            rgba8.extend_from_slice(bytes);
        }
        ColorType::Rgb => {
            if bytes.len() != pixels.checked_mul(3)? {
                return None;
            }
            for pixel in bytes.chunks_exact(3) {
                rgba8.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        ColorType::Grayscale => {
            if bytes.len() != pixels {
                return None;
            }
            for gray in bytes {
                rgba8.extend_from_slice(&[*gray, *gray, *gray, 255]);
            }
        }
        ColorType::GrayscaleAlpha => {
            if bytes.len() != pixels.checked_mul(2)? {
                return None;
            }
            for pixel in bytes.chunks_exact(2) {
                rgba8.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        ColorType::Indexed => return None,
    }
    Some(rgba8)
}
