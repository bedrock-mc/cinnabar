//! Block-entity asset compiler: packs the pack textures that block-entity models and
//! overlays sample into one atlas and stamps it with the pinned inventory.
//!
//! Textures past [`MAX_TEXTURE_SIDE`] are item or full-screen art, skipped and counted.

use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use ::image::{ImageFormat, ImageReader, Limits};
use assets::{
    AssetError, BlockEntityPlacement, MAX_BLOCK_ENTITY_ATLAS_SIDE, MAX_BLOCK_ENTITY_PLACEMENTS,
    canonical_source_manifest_sha256, encode_block_entity_catalog,
};
use sha2::{Digest, Sha256};

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_TEXTURE_SIDE: u32 = 256;
/// Animation strips packed frame-stacked; only these names may exceed the side bound in height.
const STRIP_TEXTURES: &[&str] = &[
    "textures/blocks/conduit_wind_horizontal",
    "textures/blocks/conduit_wind_vertical",
];
const MAX_STRIP_HEIGHT: u32 = 1024;
const ATLAS_WIDTH: u32 = 1024;
const GUTTER: u32 = 1;

/// Whole directories whose textures (minus `_mers`) are packed.
const SOURCE_DIRECTORIES: &[&str] = &[
    "textures/entity/banner",
    "textures/entity/bed",
    "textures/entity/bell",
    "textures/entity/chest",
    "textures/entity/copper_golem",
    "textures/entity/shulker",
    "textures/entity/skulls",
];

/// Individual textures packed by exact logical name (no extension).
const SOURCE_FILES: &[&str] = &[
    "textures/blocks/bell_bottom",
    "textures/blocks/bell_side",
    "textures/blocks/bell_top",
    "textures/blocks/conduit_base",
    "textures/blocks/conduit_cage",
    "textures/blocks/conduit_closed",
    "textures/blocks/conduit_open",
    "textures/blocks/conduit_wind_horizontal",
    "textures/blocks/conduit_wind_vertical",
    "textures/blocks/decorated_pot_base",
    "textures/blocks/decorated_pot_side",
    "textures/blocks/end_gateway",
    "textures/blocks/glow_item_frame",
    "textures/blocks/itemframe_background",
    "textures/blocks/lectern_base",
    "textures/blocks/lectern_front",
    "textures/blocks/lectern_sides",
    "textures/blocks/lectern_top",
    "textures/blocks/mob_spawner",
    "textures/entity/alex",
    "textures/entity/beacon_beam",
    "textures/entity/dragon/dragon",
    "textures/entity/enchanting_table_book",
    "textures/entity/end_portal",
    "textures/entity/piglin/piglin",
    "textures/entity/steve",
    "textures/environment/end_portal_colors",
];

#[derive(Debug)]
pub struct CompiledBlockEntityCarrier {
    pub bytes: Vec<u8>,
    pub report: BlockEntityCompileReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockEntityCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub atlas_size: [u32; 2],
    pub textures_packed: usize,
    pub textures_skipped_oversized: usize,
    pub textures_skipped_undecodable: usize,
}

/// Compiles the block-entity atlas, pinned to the canonical `source_manifest` digest.
pub fn compile_block_entity_assets(
    pack: &Path,
    source_manifest: &[u8],
) -> Result<CompiledBlockEntityCarrier, AssetError> {
    let source_manifest_sha256 = canonical_source_manifest_sha256(source_manifest);
    let mut sources = BTreeMap::<String, PathBuf>::new();
    for directory in SOURCE_DIRECTORIES {
        collect_directory(pack, directory, &mut sources)?;
    }
    collect_globbed_entity_textures(pack, &mut sources)?;
    collect_globbed_block_textures(pack, &mut sources)?;
    for name in SOURCE_FILES {
        if let Some(path) = resolve_texture(pack, name)? {
            sources.insert((*name).to_owned(), path);
        }
    }
    for stage in 0..10 {
        let name = format!("textures/environment/destroy_stage_{stage}");
        if let Some(path) = resolve_texture(pack, &name)? {
            sources.insert(name, path);
        }
    }

    let mut oversized = 0;
    let mut undecodable = 0;
    let mut decoded = Vec::new();
    for (name, path) in sources {
        match decode(&path, &name)? {
            Outcome::Texture(texture) => decoded.push((name, texture)),
            Outcome::Oversized => oversized += 1,
            Outcome::Undecodable => undecodable += 1,
        }
    }
    if decoded.len() > MAX_BLOCK_ENTITY_PLACEMENTS {
        return Err(invalid("too many block-entity textures"));
    }
    let (width, height, rgba8, placements) = pack_atlas(decoded)?;
    let bytes = encode_block_entity_catalog(source_manifest, width, height, &rgba8, &placements)?;
    Ok(CompiledBlockEntityCarrier {
        report: BlockEntityCompileReport {
            source_manifest_sha256,
            carrier_sha256: Sha256::digest(&bytes).into(),
            atlas_size: [width, height],
            textures_packed: placements.len(),
            textures_skipped_oversized: oversized,
            textures_skipped_undecodable: undecodable,
        },
        bytes,
    })
}

struct Texture {
    width: u32,
    height: u32,
    rgba8: Vec<u8>,
}

enum Outcome {
    Texture(Texture),
    Oversized,
    Undecodable,
}

fn collect_directory(
    pack: &Path,
    directory: &str,
    sources: &mut BTreeMap<String, PathBuf>,
) -> Result<(), AssetError> {
    for entry in list(&pack.join(directory))? {
        if let Some(name) = texture_name(&entry, pack) {
            sources.insert(name, entry);
        }
    }
    Ok(())
}

/// Signs and hanging signs are flat files in `textures/entity`.
fn collect_globbed_entity_textures(
    pack: &Path,
    sources: &mut BTreeMap<String, PathBuf>,
) -> Result<(), AssetError> {
    for entry in list(&pack.join("textures/entity"))? {
        let Some(name) = texture_name(&entry, pack) else {
            continue;
        };
        let file = name.rsplit('/').next().unwrap_or_default();
        if file.contains("sign") {
            sources.insert(name, entry);
        }
    }
    Ok(())
}

/// Decorated-pot sherd patterns are flat files in `textures/blocks`.
fn collect_globbed_block_textures(
    pack: &Path,
    sources: &mut BTreeMap<String, PathBuf>,
) -> Result<(), AssetError> {
    for entry in list(&pack.join("textures/blocks"))? {
        let Some(name) = texture_name(&entry, pack) else {
            continue;
        };
        if name.ends_with("_pottery_pattern") {
            sources.insert(name, entry);
        }
    }
    Ok(())
}

/// Files of a directory in sorted order; a missing directory is empty.
fn list(directory: &Path) -> Result<Vec<PathBuf>, AssetError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(AssetError::TextureIo {
                key: directory.display().to_string().into(),
                path: directory.to_path_buf(),
                source,
            });
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| AssetError::TextureIo {
            key: directory.display().to_string().into(),
            path: directory.to_path_buf(),
            source,
        })?;
        if entry.path().is_file() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

/// Pack-relative logical name of a `.png`/`.tga` color texture; `None` for anything else.
fn texture_name(path: &Path, pack: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if extension != "png" && extension != "tga" {
        return None;
    }
    let relative = path.strip_prefix(pack).ok()?.with_extension("");
    let name = relative.to_str()?.replace('\\', "/");
    (!name.ends_with("_mers")).then_some(name)
}

fn resolve_texture(pack: &Path, name: &str) -> Result<Option<PathBuf>, AssetError> {
    for extension in ["png", "tga"] {
        let path = pack.join(format!("{name}.{extension}"));
        if path.try_exists().map_err(|source| AssetError::TextureIo {
            key: name.into(),
            path: path.clone(),
            source,
        })? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn decode(path: &Path, name: &str) -> Result<Outcome, AssetError> {
    let io_error = |source| AssetError::TextureIo {
        key: name.into(),
        path: path.to_path_buf(),
        source,
    };
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(io_error)?
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Ok(Outcome::Oversized);
    }
    let format = match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("tga") => ImageFormat::Tga,
        _ => ImageFormat::Png,
    };
    let Ok((width, height)) =
        ImageReader::with_format(Cursor::new(&bytes), format).into_dimensions()
    else {
        return Ok(Outcome::Undecodable);
    };
    let max_height = if STRIP_TEXTURES.contains(&name) {
        MAX_STRIP_HEIGHT
    } else {
        MAX_TEXTURE_SIDE
    };
    if width == 0 || height == 0 || width > MAX_TEXTURE_SIDE || height > max_height {
        return Ok(Outcome::Oversized);
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_SIDE);
    limits.max_image_height = Some(max_height);
    limits.max_alloc = Some(4 * 1024 * 1024);
    reader.limits(limits);
    let Ok(image) = reader.decode() else {
        return Ok(Outcome::Undecodable);
    };
    Ok(Outcome::Texture(Texture {
        width,
        height,
        rgba8: image.into_rgba8().into_raw(),
    }))
}

/// Shelf-packs textures tallest first into a fixed-width atlas.
fn pack_atlas(
    mut textures: Vec<(String, Texture)>,
) -> Result<(u32, u32, Vec<u8>, Vec<BlockEntityPlacement>), AssetError> {
    textures.sort_by(|left, right| {
        right
            .1
            .height
            .cmp(&left.1.height)
            .then_with(|| left.0.cmp(&right.0))
    });
    let mut placed = Vec::with_capacity(textures.len());
    let (mut x, mut y, mut shelf_height) = (0u32, 0u32, 0u32);
    for (name, texture) in textures {
        if x + texture.width > ATLAS_WIDTH {
            x = 0;
            y += shelf_height + GUTTER;
            shelf_height = 0;
        }
        placed.push((name, texture, x, y));
        x += placed.last().map_or(0, |entry| entry.1.width) + GUTTER;
        shelf_height = shelf_height.max(placed.last().map_or(0, |entry| entry.1.height));
    }
    let height = (y + shelf_height).max(1);
    if height > MAX_BLOCK_ENTITY_ATLAS_SIDE {
        return Err(invalid("block-entity atlas exceeds the height bound"));
    }
    let mut rgba8 = vec![0u8; ATLAS_WIDTH as usize * height as usize * 4];
    let mut placements = Vec::with_capacity(placed.len());
    for (name, texture, px, py) in placed {
        for row in 0..texture.height {
            let source = row as usize * texture.width as usize * 4;
            let target = ((py + row) as usize * ATLAS_WIDTH as usize + px as usize) * 4;
            let length = texture.width as usize * 4;
            rgba8[target..target + length].copy_from_slice(&texture.rgba8[source..source + length]);
        }
        placements.push(BlockEntityPlacement {
            name: name.into(),
            x: px,
            y: py,
            width: texture.width,
            height: texture.height,
        });
    }
    placements.sort_by(|left, right| left.name.cmp(&right.name));
    Ok((ATLAS_WIDTH, height, rgba8, placements))
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn review_texture_directory_errors_are_not_treated_as_absence() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("textures");
        std::fs::write(&directory, b"not a directory").unwrap();
        assert!(super::list(&directory).is_err());
        assert!(
            super::list(&root.path().join("missing"))
                .unwrap()
                .is_empty()
        );
    }

    use super::*;

    fn write_png(path: &Path, width: u32, height: u32, shade: u8) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut buffer = Vec::new();
        let image =
            ::image::RgbaImage::from_pixel(width, height, ::image::Rgba([shade, 0, 0, 255]));
        image
            .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
            .unwrap();
        fs::write(path, buffer).unwrap();
    }

    #[test]
    fn packs_selected_textures_and_skips_oversized_and_mers() {
        let pack = tempfile::tempdir().unwrap();
        let root = pack.path();
        write_png(&root.join("textures/entity/chest/normal.png"), 64, 64, 10);
        write_png(
            &root.join("textures/entity/chest/normal_mers.png"),
            64,
            64,
            11,
        );
        write_png(&root.join("textures/entity/sign_oak.png"), 64, 32, 20);
        write_png(
            &root.join("textures/entity/banner/banner.png"),
            512,
            512,
            30,
        );
        write_png(
            &root.join("textures/environment/destroy_stage_3.png"),
            16,
            16,
            40,
        );
        write_png(&root.join("textures/entity/cow.png"), 64, 32, 50);
        let compiled = compile_block_entity_assets(root, b"{}").unwrap();
        let runtime = assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).unwrap();
        assert!(runtime.placement("textures/entity/chest/normal").is_some());
        assert!(runtime.placement("textures/entity/sign_oak").is_some());
        assert!(
            runtime
                .placement("textures/environment/destroy_stage_3")
                .is_some()
        );
        assert!(
            runtime
                .placement("textures/entity/chest/normal_mers")
                .is_none()
        );
        assert!(runtime.placement("textures/entity/cow").is_none());
        assert_eq!(compiled.report.textures_skipped_oversized, 1);
        let placement = runtime.placement("textures/entity/sign_oak").unwrap();
        let atlas = runtime.atlas_rgba8();
        let offset = ((placement.y * ATLAS_WIDTH + placement.x) * 4) as usize;
        assert_eq!(atlas[offset], 20);
    }

    #[test]
    fn empty_pack_still_yields_a_valid_carrier() {
        let pack = tempfile::tempdir().unwrap();
        let compiled = compile_block_entity_assets(pack.path(), b"{}").unwrap();
        assert_eq!(compiled.report.textures_packed, 0);
        assert!(assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).is_ok());
    }
}
