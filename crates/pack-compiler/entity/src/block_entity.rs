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
pub(crate) const MAX_TEXTURE_SIDE: u32 = 256;
/// Animation strips packed frame-stacked; only these names may exceed the side bound in height.
const STRIP_TEXTURES: &[&str] = &[
    assets::block_entity_geometry::textures::AtlasTexture::BlocksConduitWindHorizontal.path(),
    assets::block_entity_geometry::textures::AtlasTexture::BlocksConduitWindVertical.path(),
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

/// Encoded block-entity assets and their compilation diagnostics.
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
    for name in assets::block_entity_geometry::textures::AtlasTexture::ALL
        .iter()
        .map(|role| role.path())
        .chain([
            assets::CRYSTAL_BEAM_TEXTURE,
            assets::block_entity_geometry::CONDUIT_TEXTURE.0,
            assets::block_entity_geometry::POT_BASE_TEXTURE.0,
            assets::block_entity_geometry::POT_SIDE_TEXTURE.0,
        ])
        .chain(
            assets::block_entity_geometry::ITEM_FRAME_TEXTURES
                .iter()
                .copied(),
        )
        .chain(
            assets::block_entity_geometry::LECTERN_TEXTURES
                .iter()
                .copied(),
        )
        .chain(
            assets::block_entity_geometry::chalkboard::ChalkboardSize::ALL
                .map(|size| size.texture()),
        )
    {
        if let Some(path) = resolve_texture(pack, name)? {
            sources.insert(name.to_owned(), path);
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

/// Computes the shelf placements and height, including gutters and unused shelf space.
fn atlas_layout(
    textures: &[(String, Texture)],
) -> Result<(u32, Vec<BlockEntityPlacement>), AssetError> {
    let mut ordered: Vec<_> = textures.iter().collect();
    ordered.sort_by(|left, right| {
        right
            .1
            .height
            .cmp(&left.1.height)
            .then_with(|| left.0.cmp(&right.0))
    });
    let mut placements = Vec::with_capacity(textures.len());
    let (mut x, mut y, mut shelf_height) = (0u32, 0u32, 0u32);
    for (name, texture) in ordered {
        if texture.width > ATLAS_WIDTH || texture.height > MAX_BLOCK_ENTITY_ATLAS_SIDE {
            return Err(invalid("block-entity texture exceeds the atlas bounds"));
        }
        if x + texture.width > ATLAS_WIDTH {
            x = 0;
            y += shelf_height + GUTTER;
            shelf_height = 0;
        }
        placements.push(BlockEntityPlacement {
            name: name.as_str().into(),
            x,
            y,
            width: texture.width,
            height: texture.height,
        });
        x += texture.width + GUTTER;
        shelf_height = shelf_height.max(texture.height);
        if y + shelf_height > MAX_BLOCK_ENTITY_ATLAS_SIDE {
            return Err(invalid("block-entity atlas exceeds the height bound"));
        }
    }
    let height = (y + shelf_height).max(1);
    placements.sort_by(|left, right| left.name.cmp(&right.name));
    Ok((height, placements))
}

/// Shelf-packs textures tallest first into a fixed-width atlas.
fn pack_atlas(
    mut textures: Vec<(String, Texture)>,
) -> Result<(u32, u32, Vec<u8>, Vec<BlockEntityPlacement>), AssetError> {
    let (height, placements) = atlas_layout(&textures)?;
    textures.sort_by(|left, right| left.0.cmp(&right.0));
    let mut rgba8 = vec![0u8; ATLAS_WIDTH as usize * height as usize * 4];
    for ((_, texture), placement) in textures.iter().zip(&placements) {
        for row in 0..texture.height {
            let source = row as usize * texture.width as usize * 4;
            let target =
                ((placement.y + row) as usize * ATLAS_WIDTH as usize + placement.x as usize) * 4;
            let length = texture.width as usize * 4;
            rgba8[target..target + length].copy_from_slice(&texture.rgba8[source..source + length]);
        }
    }
    Ok((ATLAS_WIDTH, height, rgba8, placements))
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

/// Decoded color pixels supplied by the shared pack texture resolver.
pub struct BlockEntityTexture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// Rebuilds the carrier atlas from winning logical texture reads and its untouched base pixels.
/// The caller admits sizes against its complete render budget, including appended and dynamic rows.
/// Missing, invalid, or unpackable replacements retain the base tile; removing a pack restores it.
pub fn overlay_block_entity_assets(
    base: &assets::RuntimeBlockEntityAssets,
    accepts_size: impl Fn([u32; 2]) -> bool,
    mut read: impl FnMut(&str) -> Option<BlockEntityTexture>,
) -> Result<assets::RuntimeBlockEntityAssets, AssetError> {
    let [atlas_width, _] = base.atlas_size();
    // Reserve every base tile in the layout before admitting replacements in logical name order.
    let mut textures: Vec<_> = base
        .placements()
        .iter()
        .map(|tile| {
            (
                tile.name.to_string(),
                Texture {
                    width: tile.width,
                    height: tile.height,
                    rgba8: Vec::new(),
                },
            )
        })
        .collect();
    let fits = |textures: &[(String, Texture)]| -> Result<(), AssetError> {
        let (height, _) = atlas_layout(textures)?;
        if !accepts_size([ATLAS_WIDTH, height]) {
            return Err(invalid("block-entity atlas exceeds the render budget"));
        }
        Ok(())
    };
    fits(&textures)?;
    for (index, placement) in base.placements().iter().enumerate() {
        if let Some(texture) = read(&placement.name).filter(|texture| {
            texture.width > 0
                && texture.height > 0
                && texture.width <= ATLAS_WIDTH
                && texture.height <= MAX_STRIP_HEIGHT
                && texture.rgba8.len() == texture.width as usize * texture.height as usize * 4
        }) {
            let fallback = std::mem::replace(
                &mut textures[index].1,
                Texture {
                    width: texture.width,
                    height: texture.height,
                    rgba8: texture.rgba8.into_vec(),
                },
            );
            if fits(&textures).is_err() {
                textures[index].1 = fallback;
            }
        }
        if textures[index].1.rgba8.is_empty() {
            let mut rgba8 =
                Vec::with_capacity(placement.width as usize * placement.height as usize * 4);
            for y in placement.y..placement.y + placement.height {
                let start = ((y * atlas_width + placement.x) * 4) as usize;
                rgba8.extend_from_slice(
                    &base.atlas_rgba8()[start..start + placement.width as usize * 4],
                );
            }
            textures[index].1.rgba8 = rgba8;
        }
    }
    for size in assets::block_entity_geometry::chalkboard::ChalkboardSize::ALL {
        let name = size.texture();
        if base.placement(name).is_some() {
            continue;
        }
        let Some(texture) = read(name).filter(|texture| {
            texture.width > 0
                && texture.width <= ATLAS_WIDTH
                && texture.height > 0
                && texture.height <= MAX_STRIP_HEIGHT
                && texture.rgba8.len() == texture.width as usize * texture.height as usize * 4
        }) else {
            continue;
        };
        textures.push((
            name.into(),
            Texture {
                width: texture.width,
                height: texture.height,
                rgba8: texture.rgba8.into_vec(),
            },
        ));
        if textures.len() > MAX_BLOCK_ENTITY_PLACEMENTS || fits(&textures).is_err() {
            textures.pop();
        }
    }
    let (width, height, pixels, placements) = pack_atlas(textures)?;
    base.repack(width, height, &pixels, &placements)
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
    fn crystal_beam_texture_is_packed_for_the_native_additional_effect() {
        let pack = tempfile::tempdir().unwrap();
        let name = assets::CRYSTAL_BEAM_TEXTURE;
        write_png(&pack.path().join(format!("{name}.png")), 16, 16, 50);
        let compiled = compile_block_entity_assets(pack.path(), b"{}").unwrap();
        let runtime = assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).unwrap();
        assert!(runtime.placement(name).is_some());
    }

    #[test]
    fn chalkboard_world_textures_are_packed_without_borrowing_item_icons() {
        use assets::block_entity_geometry::chalkboard::ChalkboardSize;
        let pack = tempfile::tempdir().unwrap();
        for size in ChalkboardSize::ALL {
            write_png(
                &pack.path().join(format!("{}.png", size.texture())),
                32,
                16,
                90,
            );
        }
        write_png(
            &pack.path().join("textures/items/chalkboard_small.png"),
            16,
            16,
            12,
        );
        let compiled = compile_block_entity_assets(pack.path(), b"{}").unwrap();
        let runtime = assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).unwrap();
        for size in ChalkboardSize::ALL {
            assert_eq!(tile_pixel(&runtime, size.texture()), [90, 0, 0, 255]);
        }
        assert!(
            runtime
                .placement("textures/items/chalkboard_small")
                .is_none()
        );
    }

    #[test]
    fn chalkboard_pack_adds_world_artwork_absent_from_startup_and_removal_restores_it() {
        use assets::block_entity_geometry::chalkboard::ChalkboardSize;
        let pack = tempfile::tempdir().unwrap();
        write_png(
            &pack.path().join("textures/entity/chest/normal.png"),
            64,
            64,
            10,
        );
        let compiled = compile_block_entity_assets(pack.path(), b"{}").unwrap();
        let base = assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).unwrap();
        let changed = overlay_block_entity_assets(
            &base,
            |_| true,
            |name| {
                ChalkboardSize::ALL
                    .into_iter()
                    .any(|size| size.texture() == name)
                    .then(|| BlockEntityTexture {
                        width: 32,
                        height: 16,
                        rgba8: [20, 60, 100, 255].repeat(32 * 16).into_boxed_slice(),
                    })
            },
        )
        .unwrap();
        for size in ChalkboardSize::ALL {
            assert_eq!(tile_pixel(&changed, size.texture()), [20, 60, 100, 255]);
        }
        let removed = overlay_block_entity_assets(&base, |_| true, |_| None).unwrap();
        assert_eq!(removed.identity(), base.identity());
    }

    /// Reads the first pixel of one logical atlas tile.
    fn tile_pixel(base: &assets::RuntimeBlockEntityAssets, name: &str) -> [u8; 4] {
        let tile = base.placement(name).unwrap();
        let offset = ((tile.y * base.atlas_size()[0] + tile.x) * 4) as usize;
        base.atlas_rgba8()[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn overlays_repack_uvs_preserve_missing_tiles_and_restore_base() {
        let pack = tempfile::tempdir().unwrap();
        let chest = "textures/entity/chest/normal";
        let sign = "textures/entity/sign_oak";
        write_png(&pack.path().join(format!("{chest}.png")), 64, 64, 10);
        write_png(&pack.path().join(format!("{sign}.png")), 64, 32, 20);
        let compiled = compile_block_entity_assets(pack.path(), b"{}").unwrap();
        let base = assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).unwrap();
        let changed = overlay_block_entity_assets(
            &base,
            |_| true,
            |name| {
                (name == sign).then(|| BlockEntityTexture {
                    width: 128,
                    height: 128,
                    rgba8: [200, 0, 0, 255].repeat(128 * 128).into_boxed_slice(),
                })
            },
        )
        .unwrap();
        assert_eq!(tile_pixel(&changed, sign), [200, 0, 0, 255]);
        assert_eq!(tile_pixel(&changed, chest), tile_pixel(&base, chest));
        assert_eq!(changed.placement(sign).unwrap().width, 128);
        assert!(changed.atlas_size()[1] > base.atlas_size()[1]);
        let restored = overlay_block_entity_assets(&base, |_| true, |_| None).unwrap();
        assert_eq!(restored.identity(), base.identity());
        let invalid = overlay_block_entity_assets(
            &base,
            |_| true,
            |_| {
                Some(BlockEntityTexture {
                    width: 64,
                    height: 64,
                    rgba8: Box::default(),
                })
            },
        )
        .unwrap();
        assert_eq!(invalid.identity(), base.identity());
    }

    #[test]
    fn frame_carriers_preserve_distinct_rim_and_backing_replacement_pixels() {
        let pack = tempfile::tempdir().unwrap();
        for (index, name) in assets::block_entity_geometry::ITEM_FRAME_TEXTURES
            .into_iter()
            .enumerate()
        {
            write_png(
                &pack.path().join(format!("{name}.png")),
                32,
                32,
                70 + index as u8,
            );
        }
        let compiled = compile_block_entity_assets(pack.path(), b"{}").unwrap();
        let runtime = assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).unwrap();
        for (index, name) in assets::block_entity_geometry::ITEM_FRAME_TEXTURES
            .into_iter()
            .enumerate()
        {
            let placement = runtime
                .placement(name)
                .expect("frame material must be carried into the runtime atlas");
            assert_eq!([placement.width, placement.height], [32, 32]);
            let offset = ((placement.y * runtime.atlas_size()[0] + placement.x) * 4) as usize;
            assert_eq!(runtime.atlas_rgba8()[offset], 70 + index as u8);
        }
    }

    #[test]
    fn oversized_overlay_set_retains_base_tiles_after_the_atlas_limit() {
        let placements: Vec<_> = (0..8)
            .map(|index| BlockEntityPlacement {
                name: format!("textures/entity/chest/tile_{index}").into(),
                x: index,
                y: 0,
                width: 1,
                height: 1,
            })
            .collect();
        let bytes = encode_block_entity_catalog(
            b"{}",
            ATLAS_WIDTH,
            1,
            &vec![10; ATLAS_WIDTH as usize * 4],
            &placements,
        )
        .unwrap();
        let base = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
        let overlaid = overlay_block_entity_assets(
            &base,
            |_| true,
            |_| {
                Some(BlockEntityTexture {
                    width: ATLAS_WIDTH,
                    height: MAX_STRIP_HEIGHT,
                    rgba8: vec![200; (ATLAS_WIDTH * MAX_STRIP_HEIGHT * 4) as usize]
                        .into_boxed_slice(),
                })
            },
        )
        .unwrap();

        // Three large replacements fit while reserving space for the remaining base tiles.
        for (index, placement) in placements.iter().enumerate() {
            let tile = overlaid.placement(&placement.name).unwrap();
            let expected = if index < 3 {
                [ATLAS_WIDTH, MAX_STRIP_HEIGHT]
            } else {
                [1, 1]
            };
            assert_eq!([tile.width, tile.height], expected);
            assert_eq!(
                tile_pixel(&overlaid, &placement.name),
                [if index < 3 { 200 } else { 10 }; 4]
            );
        }
    }

    #[test]
    fn shelf_limited_overlays_keep_fitting_replacements_and_fall_back_per_tile() {
        let placements: Vec<_> = (0..9)
            .map(|index| BlockEntityPlacement {
                name: format!("textures/entity/chest/tile_{index}").into(),
                x: index,
                y: 0,
                width: 1,
                height: 1,
            })
            .collect();
        let bytes = encode_block_entity_catalog(
            b"{}",
            ATLAS_WIDTH,
            1,
            &vec![10; ATLAS_WIDTH as usize * 4],
            &placements,
        )
        .unwrap();
        let base = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
        let overlaid = overlay_block_entity_assets(
            &base,
            |_| true,
            |name| {
                let side = if name == placements[8].name.as_ref() {
                    16
                } else {
                    512
                };
                Some(BlockEntityTexture {
                    width: side,
                    height: side,
                    rgba8: vec![200; (side * side * 4) as usize].into_boxed_slice(),
                })
            },
        )
        .unwrap();

        assert!(overlaid.atlas_size()[1] <= MAX_BLOCK_ENTITY_ATLAS_SIDE);
        for (index, placement) in placements.iter().enumerate() {
            let tile = overlaid.placement(&placement.name).unwrap();
            let expected_size = match index {
                0..7 => [512, 512],
                7 => [1, 1],
                _ => [16, 16],
            };
            assert_eq!([tile.width, tile.height], expected_size);
            assert_eq!(
                tile_pixel(&overlaid, &placement.name),
                [if index == 7 { 10 } else { 200 }; 4]
            );
        }
    }

    #[test]
    fn unpackable_base_is_rejected_before_reading_replacements() {
        let height = MAX_BLOCK_ENTITY_ATLAS_SIDE / 2;
        let placements: Vec<_> = (0..3)
            .map(|index| BlockEntityPlacement {
                name: format!("tile_{index}").into(),
                x: 0,
                y: 0,
                width: ATLAS_WIDTH,
                height,
            })
            .collect();
        let bytes = encode_block_entity_catalog(
            b"{}",
            ATLAS_WIDTH,
            height,
            &vec![0; (ATLAS_WIDTH * height * 4) as usize],
            &placements,
        )
        .unwrap();
        let base = assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap();
        let mut reads = 0;
        assert!(
            overlay_block_entity_assets(
                &base,
                |_| true,
                |_| {
                    reads += 1;
                    None
                }
            )
            .is_err()
        );
        assert_eq!(reads, 0);
    }

    #[test]
    fn empty_pack_still_yields_a_valid_carrier() {
        let pack = tempfile::tempdir().unwrap();
        let compiled = compile_block_entity_assets(pack.path(), b"{}").unwrap();
        assert_eq!(compiled.report.textures_packed, 0);
        assert!(assets::RuntimeBlockEntityAssets::decode(&compiled.bytes).is_ok());
    }
}
