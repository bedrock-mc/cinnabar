//! JSON-UI asset compiler: packs the pinned pack's `textures/ui` sprites into
//! gutter-separated atlas pages, ingests their nine-slice sidecars, and stores
//! the raw `ui/*.json` catalog verbatim into the hash-pinned UI carrier.
//!
//! Textures wider or taller than [`MAX_UI_TEXTURE_SIDE`] are skipped from the
//! sprite atlas and counted. Images referenced by UI JSON, including non-UI
//! textures and oversized artwork or animation strips, stay in the carrier:
//! small images join the atlas and oversized ones retain their encoded bytes.
//! The six panorama faces and their overlay also stay as raw files. The raw ui
//! json is kept unresolved because a joined server pack overrides it at runtime.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

use ::image::{ImageFormat, ImageReader, Limits};
use assets::{
    AssetError, RuntimeUiAssets, UiAtlasPage, UiFile, UiNineSlice, UiSidecar, UiTexturePlacement,
    canonical_source_manifest_sha256, encode_ui_catalog,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Per-png source-byte cap; a larger encoding is a full-screen asset, skipped.
const MAX_UI_SOURCE_BYTES: usize = 1024 * 1024;
/// Sprites wider or taller than this are not form textures; skipped and counted.
const MAX_UI_TEXTURE_SIDE: u32 = 256;
const MAX_UI_SIDECAR_BYTES: usize = 64 * 1024;
/// Atlas page width, and the maximum shelf height before spilling to a new page.
const PAGE_SIDE: u32 = 2048;
const GUTTER: u32 = 1;
/// Bounds the recursive walk so a pathological tree cannot exhaust memory.
const MAX_WALK_ENTRIES: usize = 200_000;

#[derive(Debug)]
pub struct CompiledUiCarrier {
    pub bytes: Vec<u8>,
    pub report: UiCompileReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub atlas_pages: usize,
    pub textures_packed: usize,
    /// Sprites past [`MAX_UI_TEXTURE_SIDE`] or the source cap, skipped from the atlas.
    pub textures_skipped_oversized: usize,
    /// Sources that are not decodable PNGs (e.g. a mislabeled JPEG), skipped.
    pub textures_skipped_undecodable: usize,
    pub sidecars: usize,
    /// Sidecar json without a usable `base_size`, skipped and counted.
    pub sidecars_skipped: usize,
    pub ui_files: usize,
    /// Ui json past [`assets::MAX_UI_FILE_BYTES`], skipped and counted.
    pub ui_files_skipped: usize,
    pub atlas_pixel_bytes: usize,
}

/// Compile the pinned pack's UI textures, sidecars, and raw ui json into the
/// carrier, pinned to the canonical `source_manifest` digest.
pub fn compile_ui_assets(
    pack: &Path,
    source_manifest: &[u8],
) -> Result<CompiledUiCarrier, AssetError> {
    let source_manifest_sha256 = canonical_source_manifest_sha256(source_manifest);
    if source_manifest_sha256 == [0; 32] {
        return Err(invalid("ui source manifest digest is unset"));
    }

    let textures_dir = pack.join("textures");
    let ui_dir = pack.join("ui");

    let mut png_paths = Vec::new();
    let mut sidecar_paths = Vec::new();
    let mut budget = MAX_WALK_ENTRIES;
    walk(
        &textures_dir,
        pack,
        &mut png_paths,
        &mut sidecar_paths,
        &mut budget,
    )?;
    let mut ui_paths = Vec::new();
    let mut ignored = Vec::new();
    walk(&ui_dir, pack, &mut ignored, &mut ui_paths, &mut budget)?;
    ui_paths.extend(
        png_paths
            .iter()
            .filter(|relative| is_panorama_file(strip_extension(relative)))
            .cloned(),
    );
    // Release packs carry the title splashes; preview packs have none.
    if pack.join(SPLASHES).is_file() {
        ui_paths.push(SPLASHES.to_owned());
    }
    ui_paths.extend(
        assets::UI_CREDITS_FILES
            .into_iter()
            .filter(|path| pack.join(path).is_file())
            .map(str::to_owned),
    );

    let (mut files, mut ui_files_skipped) = read_ui_files(pack, &ui_paths)?;
    let referenced = referenced_textures(&files);
    png_paths.retain(|path| {
        path.starts_with("textures/ui/") || referenced.contains(strip_extension(path))
    });
    sidecar_paths.retain(|path| {
        path.starts_with("textures/ui/") || referenced.contains(strip_extension(path))
    });
    let (textures, oversized, textures_skipped_undecodable) = read_textures(pack, &png_paths)?;
    let textures_skipped_oversized = oversized.len();
    let raw_paths = oversized
        .into_iter()
        .filter(|path| referenced.contains(strip_extension(path)))
        .filter(|path| !files.iter().any(|file| file.path.as_ref() == path.as_str()))
        .collect::<Vec<_>>();
    let (raw, skipped) = read_ui_files(pack, &raw_paths)?;
    files.extend(raw);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    ui_files_skipped += skipped;
    let packed = pack_atlas(textures)?;
    let (sidecars, sidecars_skipped) = read_sidecars(pack, &sidecar_paths)?;

    let atlas_pixel_bytes = packed.pages.iter().map(|page| page.rgba8.len()).sum();
    let bytes = encode_ui_catalog(
        source_manifest_sha256,
        &packed.pages,
        &packed.placements,
        &sidecars,
        &files,
    )?;
    Ok(CompiledUiCarrier {
        report: UiCompileReport {
            source_manifest_sha256,
            carrier_sha256: Sha256::digest(&bytes).into(),
            atlas_pages: packed.pages.len(),
            textures_packed: packed.placements.len(),
            textures_skipped_oversized,
            textures_skipped_undecodable,
            sidecars: sidecars.len(),
            sidecars_skipped,
            ui_files: files.len(),
            ui_files_skipped,
            atlas_pixel_bytes,
        },
        bytes,
    })
}

/// A texture decoded and ready to pack: its logical path and RGBA8 pixels.
struct DecodedUiTexture {
    path: String,
    width: u32,
    height: u32,
    rgba8: Box<[u8]>,
}

struct PackedAtlas {
    pages: Vec<UiAtlasPage>,
    placements: Vec<UiTexturePlacement>,
}

/// The outcome of reading one candidate texture. Only host I/O is fatal; a
/// source that is not a bounded form sprite is skipped and counted.
enum TextureOutcome {
    Packed(DecodedUiTexture),
    /// Source or decoded dimensions past the sprite bound (full-screen art).
    Oversized,
    /// Not a decodable PNG (e.g. a mislabeled JPEG in the pinned pack).
    Undecodable,
}

/// Decoded texture counts: the packable set plus the two skip tallies.
type TextureSet = (Vec<DecodedUiTexture>, Vec<String>, usize);

fn read_textures(pack: &Path, png_paths: &[String]) -> Result<TextureSet, AssetError> {
    let mut textures = Vec::new();
    let mut oversized = Vec::new();
    let mut undecodable = 0usize;
    for relative in png_paths {
        let path = pack.join(relative);
        match decode_ui_texture(&path, strip_extension(relative))? {
            TextureOutcome::Packed(texture) => textures.push(texture),
            TextureOutcome::Oversized => oversized.push(relative.clone()),
            TextureOutcome::Undecodable => undecodable += 1,
        }
    }
    Ok((textures, oversized, undecodable))
}

/// UI variables can carry image paths too, so collect literal strings throughout
/// the catalog. Only files discovered by the bounded texture walk are admitted.
fn referenced_textures(files: &[UiFile]) -> BTreeSet<String> {
    fn visit(value: &Value, paths: &mut BTreeSet<String>) {
        match value {
            Value::String(path) if path.starts_with("textures/") => {
                paths.insert(path.strip_suffix(".png").unwrap_or(path).to_owned());
            }
            Value::Array(values) => values.iter().for_each(|value| visit(value, paths)),
            Value::Object(values) => values.values().for_each(|value| visit(value, paths)),
            _ => {}
        }
    }
    let mut paths = BTreeSet::new();
    for file in files.iter().filter(|file| file.path.starts_with("ui/")) {
        let bytes = file
            .bytes
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&file.bytes);
        if let Ok(value) = serde_json::from_slice(&assets::strip_json_comments(bytes)) {
            visit(&value, &mut paths);
        }
    }
    paths
}

/// Read and decode one UI png. Host I/O failure is fatal; an oversized or
/// undecodable source yields a skip outcome rather than an error.
fn decode_ui_texture(path: &Path, logical: &str) -> Result<TextureOutcome, AssetError> {
    let file = fs::File::open(path).map_err(|source| AssetError::TextureIo {
        key: logical.into(),
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_UI_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::TextureIo {
            key: logical.into(),
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_UI_SOURCE_BYTES {
        return Ok(TextureOutcome::Oversized);
    }
    let Ok(dimensions) =
        ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png).into_dimensions()
    else {
        return Ok(TextureOutcome::Undecodable);
    };
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > MAX_UI_TEXTURE_SIDE
        || dimensions.1 > MAX_UI_TEXTURE_SIDE
    {
        return Ok(TextureOutcome::Oversized);
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_UI_TEXTURE_SIDE);
    limits.max_image_height = Some(MAX_UI_TEXTURE_SIDE);
    limits.max_alloc = Some((MAX_UI_TEXTURE_SIDE * MAX_UI_TEXTURE_SIDE * 4) as u64);
    reader.limits(limits);
    let Ok(decoded) = reader.decode() else {
        return Ok(TextureOutcome::Undecodable);
    };
    Ok(TextureOutcome::Packed(DecodedUiTexture {
        path: logical.to_owned(),
        width: dimensions.0,
        height: dimensions.1,
        rgba8: decoded.into_rgba8().into_raw().into_boxed_slice(),
    }))
}

/// Shelf-pack textures into gutter-separated pages, each trimmed to its used box.
fn pack_atlas(mut textures: Vec<DecodedUiTexture>) -> Result<PackedAtlas, AssetError> {
    // Tallest first, then widest, then path: deterministic and low-waste.
    textures.sort_by(|a, b| {
        (b.height, b.width, a.path.as_str()).cmp(&(a.height, a.width, b.path.as_str()))
    });

    struct Slot {
        index: usize,
        page: u32,
        x: u32,
        y: u32,
    }
    let mut slots = Vec::with_capacity(textures.len());
    let mut page = 0u32;
    let (mut cursor_x, mut cursor_y, mut shelf_h) = (0u32, 0u32, 0u32);
    for (index, texture) in textures.iter().enumerate() {
        let (w, h) = (texture.width, texture.height);
        if w > PAGE_SIDE || h > PAGE_SIDE {
            return Err(invalid("ui texture exceeds the atlas page bound"));
        }
        if cursor_x + w > PAGE_SIDE {
            cursor_x = 0;
            cursor_y += shelf_h + GUTTER;
            shelf_h = 0;
        }
        if cursor_y + h > PAGE_SIDE {
            page += 1;
            cursor_x = 0;
            cursor_y = 0;
            shelf_h = 0;
        }
        slots.push(Slot {
            index,
            page,
            x: cursor_x,
            y: cursor_y,
        });
        cursor_x += w + GUTTER;
        shelf_h = shelf_h.max(h);
    }

    let page_count = slots.iter().map(|slot| slot.page + 1).max().unwrap_or(0) as usize;
    if page_count > assets::MAX_UI_ATLAS_PAGES {
        return Err(invalid("ui atlas exceeds the page-count bound"));
    }
    let mut dims = vec![(0u32, 0u32); page_count];
    for slot in &slots {
        let texture = &textures[slot.index];
        let entry = &mut dims[slot.page as usize];
        entry.0 = entry.0.max(slot.x + texture.width);
        entry.1 = entry.1.max(slot.y + texture.height);
    }
    let mut buffers: Vec<Vec<u8>> = dims
        .iter()
        .map(|(w, h)| vec![0u8; (*w as usize) * (*h as usize) * 4])
        .collect();
    let mut placements = Vec::with_capacity(slots.len());
    for slot in &slots {
        let texture = &textures[slot.index];
        let (page_w, _) = dims[slot.page as usize];
        blit(
            &mut buffers[slot.page as usize],
            page_w,
            slot.x,
            slot.y,
            &texture.rgba8,
            texture.width,
            texture.height,
        );
        placements.push(UiTexturePlacement {
            path: texture.path.as_str().into(),
            page: slot.page as u16,
            x: slot.x as u16,
            y: slot.y as u16,
            width: texture.width as u16,
            height: texture.height as u16,
        });
    }
    placements.sort_by(|a, b| a.path.cmp(&b.path));

    let pages = dims
        .into_iter()
        .zip(buffers)
        .map(|((width, height), rgba8)| UiAtlasPage {
            width,
            height,
            rgba8: Arc::from(rgba8),
        })
        .collect();
    Ok(PackedAtlas { pages, placements })
}

fn blit(dest: &mut [u8], dest_width: u32, x: u32, y: u32, src: &[u8], w: u32, h: u32) {
    let stride = dest_width as usize * 4;
    let row_bytes = w as usize * 4;
    for row in 0..h as usize {
        let dest_start = (y as usize + row) * stride + x as usize * 4;
        let src_start = row * row_bytes;
        dest[dest_start..dest_start + row_bytes]
            .copy_from_slice(&src[src_start..src_start + row_bytes]);
    }
}

/// Sidecar records keyed by logical texture path, sorted, plus the skip count.
type SidecarSet = (Vec<(Box<str>, UiSidecar)>, usize);

fn read_sidecars(pack: &Path, sidecar_paths: &[String]) -> Result<SidecarSet, AssetError> {
    let mut map: BTreeMap<Box<str>, UiSidecar> = BTreeMap::new();
    let mut skipped = 0usize;
    for relative in sidecar_paths {
        let path = pack.join(relative);
        let bytes = read_bounded(&path, MAX_UI_SIDECAR_BYTES)?;
        let Some(meta) = bytes
            .as_ref()
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            .as_ref()
            .and_then(parse_sidecar)
        else {
            skipped += 1;
            continue;
        };
        let logical = strip_extension(relative);
        map.insert(logical.into(), meta);
    }
    Ok((map.into_iter().collect(), skipped))
}

/// Parse base dimensions and border insets independently; zero dimensions
/// leave the border units in source pixels.
fn parse_sidecar(value: &Value) -> Option<UiSidecar> {
    let object = value.as_object()?;
    let base_size = object
        .get("base_size")
        .and_then(read_size)
        .unwrap_or([0.0, 0.0]);
    let nineslice = object.get("nineslice_size").and_then(parse_nineslice);
    if base_size == [0.0, 0.0] && nineslice.is_none() {
        return None;
    }
    Some(UiSidecar {
        base_size,
        nineslice,
    })
}

fn parse_nineslice(value: &Value) -> Option<UiNineSlice> {
    match value {
        Value::Number(number) => {
            let inset = number.as_f64()? as f32;
            Some(UiNineSlice {
                left: inset,
                top: inset,
                right: inset,
                bottom: inset,
            })
        }
        Value::Array(items) if items.len() == 4 => Some(UiNineSlice {
            left: items[0].as_f64()? as f32,
            top: items[1].as_f64()? as f32,
            right: items[2].as_f64()? as f32,
            bottom: items[3].as_f64()? as f32,
        }),
        _ => None,
    }
}

fn read_size(value: &Value) -> Option<[f32; 2]> {
    let items = value.as_array()?;
    if items.len() < 2 {
        return None;
    }
    Some([items[0].as_f64()? as f32, items[1].as_f64()? as f32])
}

fn read_ui_files(pack: &Path, ui_paths: &[String]) -> Result<(Vec<UiFile>, usize), AssetError> {
    let mut map: BTreeMap<Box<str>, Arc<[u8]>> = BTreeMap::new();
    let mut skipped = 0usize;
    for relative in ui_paths {
        let path = pack.join(relative);
        match read_bounded(&path, assets::MAX_UI_FILE_BYTES)? {
            Some(bytes) => {
                map.insert(relative.as_str().into(), Arc::from(bytes));
            }
            None => skipped += 1,
        }
    }
    let files = map
        .into_iter()
        .map(|(path, bytes)| UiFile { path, bytes })
        .collect();
    Ok((files, skipped))
}

/// Read a file bounded to `max`; `Ok(None)` when it exceeds the bound.
fn read_bounded(path: &Path, max: usize) -> Result<Option<Vec<u8>>, AssetError> {
    let file = fs::File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > max {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// Collect pack-relative POSIX paths of every `.png` and `.json` under `dir`,
/// each stripped of the `pack_root` prefix.
fn walk(
    dir: &Path,
    pack_root: &Path,
    png_out: &mut Vec<String>,
    json_out: &mut Vec<String>,
    budget: &mut usize,
) -> Result<(), AssetError> {
    let entries = fs::read_dir(dir).map_err(|source| AssetError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| AssetError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        if *budget == 0 {
            return Err(invalid("ui asset tree exceeds the traversal bound"));
        }
        *budget -= 1;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| AssetError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            walk(&path, pack_root, png_out, json_out, budget)?;
        } else if file_type.is_file() {
            let Some(relative) = relative_posix(&path, pack_root) else {
                continue;
            };
            match path.extension().and_then(|extension| extension.to_str()) {
                Some(extension) if extension.eq_ignore_ascii_case("png") => png_out.push(relative),
                Some(extension) if extension.eq_ignore_ascii_case("json") => {
                    json_out.push(relative)
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn relative_posix(path: &Path, pack_root: &Path) -> Option<String> {
    let relative = path.strip_prefix(pack_root).ok()?;
    let mut out = String::new();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return None;
        };
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part.to_str()?);
    }
    Some(out)
}

const SPLASHES: &str = "splashes.json";

/// The six panorama faces and their overlay tint.
fn is_panorama_file(logical: &str) -> bool {
    logical
        .strip_prefix("textures/ui/panorama_")
        .is_some_and(|face| matches!(face, "0" | "1" | "2" | "3" | "4" | "5" | "overlay"))
}

fn strip_extension(relative: &str) -> &str {
    relative.rsplit_once('.').map_or(relative, |(stem, _)| stem)
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

/// Decode the carrier this compiler produced; used by tests and the command to
/// confirm a build round-trips.
pub fn decode_ui_carrier(bytes: &[u8]) -> Result<RuntimeUiAssets, AssetError> {
    RuntimeUiAssets::decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const MANIFEST: &[u8] = b"{\"schema\":1}";

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut buffer = Vec::new();
        let image = ::image::RgbaImage::from_pixel(width, height, ::image::Rgba(color));
        image
            .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
            .unwrap();
        buffer
    }

    fn synthetic_pack() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "textures/ui/button.png",
            &png(16, 16, [10, 20, 30, 255]),
        );
        write(
            root,
            "textures/ui/nested/panel.png",
            &png(32, 8, [1, 2, 3, 4]),
        );
        // A full-screen asset past the sprite bound: packed as skipped.
        write(root, "textures/ui/panorama.png", &png(512, 512, [9; 4]));
        write(
            root,
            "textures/ui/button.json",
            br#"{"nineslice_size":4,"base_size":[16,16]}"#,
        );
        write(
            root,
            "textures/ui/nested/panel.json",
            br#"{"base_size":[32,8]}"#,
        );
        write(root, "textures/ui/broken.json", b"{ not json");
        write(
            root,
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/hud_screen.json"]}"#,
        );
        write(root, "ui/_global_variables.json", br#"{"$one":1}"#);
        write(root, "ui/hud_screen.json", br#"{"namespace":"hud"}"#);
        dir
    }

    #[test]
    fn credits_runtime_documents_are_retained_without_becoming_json_ui_definitions() {
        let pack = synthetic_pack();
        let documents: [&[u8]; 3] = [
            b"line PLAYERNAME\n\nnext",
            br#"[{"section":"team","disciplines":[]}]"#,
            b"last",
        ];
        for (path, bytes) in assets::UI_CREDITS_FILES.into_iter().zip(documents) {
            write(pack.path(), path, bytes);
        }
        let compiled = compile_ui_assets(pack.path(), MANIFEST).unwrap();
        let carrier = decode_ui_carrier(&compiled.bytes).unwrap();
        for (path, bytes) in assets::UI_CREDITS_FILES.into_iter().zip(documents) {
            assert_eq!(carrier.ui_file(path), Some(bytes));
        }
        assert!(carrier.ui_file("ui/hud_screen.json").is_some());
    }

    #[test]
    fn scalar_sidecar_size_survives_ui_carrier_compilation() {
        let pack = synthetic_pack();
        write(
            pack.path(),
            "textures/ui/button.json",
            br#"{"base_size":16,"nineslice_size":4}"#,
        );
        let compiled = compile_ui_assets(pack.path(), MANIFEST).unwrap();
        let carrier = decode_ui_carrier(&compiled.bytes).unwrap();
        let sidecar = carrier
            .sidecar("textures/ui/button")
            .expect("a square sprite retains its border metadata");
        assert_eq!(sidecar.base_size, [0.0, 0.0]);
        let insets = sidecar.nineslice.unwrap();
        assert_eq!(
            [insets.left, insets.top, insets.right, insets.bottom],
            [4.0; 4]
        );
    }

    #[test]
    fn numeric_sidecar_size_keeps_source_pixel_fallback() {
        let pack = synthetic_pack();
        write(
            pack.path(),
            "textures/ui/button.json",
            br#"{"base_size":64,"nineslice_size":4}"#,
        );
        let compiled = compile_ui_assets(pack.path(), MANIFEST).unwrap();
        let carrier = decode_ui_carrier(&compiled.bytes).unwrap();
        let sidecar = carrier.sidecar("textures/ui/button").unwrap();
        assert_eq!(sidecar.base_size, [0.0, 0.0]);
        assert_eq!(sidecar.nineslice.unwrap().left, 4.0);
    }

    #[test]
    fn missing_or_unusable_sidecar_size_keeps_independent_border_metadata() {
        for value in [
            serde_json::json!({"nineslice_size": 1}),
            serde_json::json!({"base_size": null, "nineslice_size": 1}),
            serde_json::json!({"base_size": {}, "nineslice_size": 1}),
            serde_json::json!({"base_size": [], "nineslice_size": 1}),
            serde_json::json!({"base_size": [8], "nineslice_size": 1}),
        ] {
            let sidecar = parse_sidecar(&value).unwrap();
            assert_eq!(sidecar.base_size, [0.0, 0.0]);
            assert_eq!(sidecar.nineslice.unwrap().left, 1.0);
        }
        assert!(parse_sidecar(&serde_json::json!({"frames": []})).is_none());
    }

    #[test]
    fn sidecar_size_uses_the_first_two_array_elements() {
        let sidecar = parse_sidecar(&serde_json::json!({"base_size": [9, 13, 99]})).unwrap();
        assert_eq!(sidecar.base_size, [9.0, 13.0]);
    }

    #[test]
    fn compiles_packs_and_round_trips_a_synthetic_pack() {
        let pack = synthetic_pack();
        let compiled = compile_ui_assets(pack.path(), MANIFEST).unwrap();
        assert_eq!(compiled.report.textures_packed, 2);
        assert_eq!(compiled.report.textures_skipped_oversized, 1);
        assert_eq!(compiled.report.textures_skipped_undecodable, 0);
        assert_eq!(compiled.report.sidecars, 2);
        assert_eq!(compiled.report.sidecars_skipped, 1);
        assert_eq!(compiled.report.ui_files, 3);

        let assets = decode_ui_carrier(&compiled.bytes).unwrap();
        assert_eq!(
            assets.source_manifest_sha256(),
            canonical_source_manifest_sha256(MANIFEST)
        );
        let panel = assets.texture("textures/ui/nested/panel").unwrap();
        assert_eq!((panel.width, panel.height), (32, 8));
        assert_eq!(
            assets
                .sidecar("textures/ui/button")
                .unwrap()
                .nineslice
                .unwrap()
                .left,
            4.0
        );
        assert!(assets.texture("textures/ui/panorama").is_none());
        assert_eq!(
            assets.ui_file("ui/hud_screen.json").unwrap(),
            br#"{"namespace":"hud"}"#
        );
        // Placement pixels land where the atlas says they do.
        let uv = assets.texture_uv("textures/ui/button").unwrap();
        assert!(uv.u1 <= 1.0 && uv.v1 <= 1.0);
    }

    #[test]
    fn panorama_faces_are_stored_as_raw_files() {
        let pack = synthetic_pack();
        let face = png(1024, 1024, [40, 80, 120, 255]);
        write(pack.path(), "textures/ui/panorama_0.png", &face);
        let compiled = compile_ui_assets(pack.path(), MANIFEST).unwrap();
        let assets = decode_ui_carrier(&compiled.bytes).unwrap();
        assert!(assets.texture("textures/ui/panorama_0").is_none());
        assert_eq!(
            assets.ui_file("textures/ui/panorama_0.png").unwrap(),
            face.as_slice()
        );
    }

    #[test]
    fn referenced_loading_images_survive_without_the_extracted_pack() {
        let pack = synthetic_pack();
        let dirt = png(16, 16, [90, 60, 30, 255]);
        let title = png(600, 100, [20, 160, 240, 255]);
        let bar = png(640, 8, [40, 200, 80, 255]);
        for (path, bytes) in [
            ("textures/blocks/dirt.png", &dirt),
            ("textures/blocks/unused.png", &dirt),
            ("textures/ui/title.png", &title),
            ("textures/ui/loading_bar.png", &bar),
        ] {
            write(pack.path(), path, bytes);
        }
        let json = br#"{
            "namespace": "progress",
            // "texture": "textures/blocks/unused"
            "$background": "textures/blocks/dirt.png",
            "screen": {"controls": [
                {"title": {"texture": "textures/ui/title"}},
                {"bar": {"texture": "textures/ui/loading_bar"}}
            ]}
        }"#;
        write(pack.path(), "ui/progress_screen.json", json);
        let compiled = compile_ui_assets(pack.path(), MANIFEST).unwrap();
        let carrier = decode_ui_carrier(&compiled.bytes).unwrap();
        std::fs::remove_dir_all(pack.path()).unwrap();
        let background = carrier.texture("textures/blocks/dirt").unwrap();
        let page = &carrier.atlas_pages()[usize::from(background.page)];
        let at = (usize::from(background.y) * page.width as usize + usize::from(background.x)) * 4;
        assert_eq!(&page.rgba8[at..at + 4], &[90, 60, 30, 255]);
        assert!(carrier.texture("textures/blocks/unused").is_none());
        for (path, bytes) in [
            ("textures/ui/title", title.as_slice()),
            ("textures/ui/loading_bar", bar.as_slice()),
        ] {
            assert!(carrier.texture(path).is_none());
            assert_eq!(carrier.ui_file(&format!("{path}.png")), Some(bytes));
        }
        assert_eq!(
            carrier.ui_file("ui/progress_screen.json"),
            Some(json.as_slice())
        );
    }

    #[test]
    fn a_release_pack_carries_its_splashes() {
        let pack = synthetic_pack();
        let splashes = br#"{"splashes":["Haley loves Elan!"]}"#;
        write(pack.path(), "splashes.json", splashes);
        let compiled = compile_ui_assets(pack.path(), MANIFEST).unwrap();
        let assets = decode_ui_carrier(&compiled.bytes).unwrap();
        assert_eq!(
            assets.ui_file("splashes.json").unwrap(),
            splashes.as_slice()
        );
    }
}
