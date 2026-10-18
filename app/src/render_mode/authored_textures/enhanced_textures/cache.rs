//! A content-addressed local cache avoids decoding unchanged authored maps on each launch.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use assets::texture::{TextureArray, TextureMip};
use pack_compiler::pbr::PbrPack;
use sha2::{Digest, Sha256};

use super::{LOW_PBR_TILE_SIZE, Source};

const CACHE_PATH: &str = ".local/enhanced-pbr.cache";
// Includes unclamped signed normal filtering and opacity-weighted resizing.
const MAGIC: &[u8; 8] = b"CINPBR16";
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
static OVERSIZED_CACHE_WARNED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Payload {
    pub(super) color: [TextureArray; 2],
    pub(super) normal: [TextureArray; 2],
    pub(super) material: [TextureArray; 2],
    pub(super) references: Box<[u32]>,
}

impl Payload {
    /// Validates cached references before exposing the decoded pages.
    pub(super) fn into_assets(self) -> Option<render_scene::EnhancedTextureAssets> {
        render_scene::EnhancedTextureAssets::new(
            self.color,
            self.normal,
            self.material,
            self.references,
        )
    }
}

/// Collects relevant pack files in every directory while rejecting symlinks.
fn files(root: &Path, result: &mut Vec<PathBuf>) -> Option<()> {
    for entry in fs::read_dir(root).ok()? {
        let entry = entry.ok()?;
        let kind = entry.file_type().ok()?;
        if kind.is_symlink() {
            return None;
        }
        let path = entry.path();
        if kind.is_dir() {
            files(&path, result)?;
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "png" | "tga" | "jpg" | "jpeg" | "json" | "mcmeta" | "properties"
                    )
                })
        {
            result.push(path);
        }
    }
    Some(())
}

/// Hashes pack contents, decode options and carrier ownership within a byte budget.
pub(super) fn fingerprint(
    packs: &[PbrPack],
    sources: &std::collections::BTreeMap<(String, String), Vec<Source>>,
) -> Option<[u8; 32]> {
    let mut digest = Sha256::new();
    digest.update(MAGIC);
    digest.update(assets::texture::PBR_TILE_SIZE.to_le_bytes());
    digest.update(LOW_PBR_TILE_SIZE.to_le_bytes());
    digest.update(assets::texture::PBR_HEIGHT_SCALE.to_bits().to_le_bytes());
    for flag in [
        assets::texture::PBR_REF_COLOR,
        assets::texture::PBR_REF_NORMAL,
        assets::texture::PBR_REF_HEIGHT,
        assets::texture::PBR_REF_MATERIAL,
        assets::texture::PBR_REF_LABPBR,
        assets::texture::PBR_REF_OCCLUSION,
        assets::texture::PBR_REF_SUBSURFACE,
    ] {
        digest.update(flag.to_le_bytes());
    }
    let mut total = 0u64;
    let mut scratch = [0u8; 64 * 1024];
    for pack in packs {
        digest.update(pack.root().to_string_lossy().as_bytes());
        digest.update([
            pack.format() as u8,
            pack.normal_format() as u8,
            pack.color_style() as u8,
            u8::from(pack.prefers_complete_material()),
        ]);
        digest.update(pack.material_group().unwrap_or("").as_bytes());
        for (key, alias) in sources.keys() {
            digest.update(key.as_bytes());
            digest.update(alias.as_bytes());
            match pack.terrain_path(alias) {
                Ok(path) => digest.update(path.unwrap_or("").as_bytes()),
                Err(error) => digest.update(error.as_bytes()),
            }
        }
        let mut paths = Vec::new();
        files(pack.root(), &mut paths)?;
        if paths.len() > 8192 {
            return None;
        }
        paths.sort();
        for path in paths {
            let mut file = fs::File::open(&path).ok()?;
            let size = file.metadata().ok()?.len();
            total = total.checked_add(size)?;
            if total > MAX_CACHE_BYTES {
                return None;
            }
            digest.update(
                path.strip_prefix(pack.root())
                    .ok()?
                    .to_string_lossy()
                    .as_bytes(),
            );
            digest.update(size.to_le_bytes());
            loop {
                let read = file.read(&mut scratch).ok()?;
                if read == 0 {
                    break;
                }
                digest.update(&scratch[..read]);
            }
        }
    }
    for ((key, alias), entries) in sources {
        digest.update((key.len() as u64).to_le_bytes());
        digest.update(key.as_bytes());
        digest.update((alias.len() as u64).to_le_bytes());
        digest.update(alias.as_bytes());
        for source in entries {
            digest.update(source.material.to_le_bytes());
            digest.update(source.texture.to_le_bytes());
            digest.update((source.frame as u64).to_le_bytes());
            digest.update((source.count as u64).to_le_bytes());
            digest.update([u8::from(source.cutout), u8::from(source.overlay_mask)]);
            digest.update([u8::from(source.baked_tint.is_some())]);
            if let Some(tint) = source.baked_tint {
                digest.update(tint);
            }
        }
    }
    Some(digest.finalize().into())
}

/// Reads one little-endian reference or array-header word.
fn word(input: &mut impl Read) -> Option<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// Requires the same complete, bounded mip chain for cache reads and writes.
fn array_header_is_valid(layers: u32, side: u32, levels: u32) -> bool {
    layers > 0
        && layers <= super::MAX_AUTHORED_LAYERS
        && side.is_power_of_two()
        && side <= assets::texture::PBR_TILE_SIZE
        && levels == side.ilog2() + 1
}

/// Counts layer-major RGBA bytes without allocating or reading the pixel data.
fn mip_byte_len(layers: u32, size: u32) -> Option<u64> {
    u64::from(layers)
        .checked_mul(u64::from(size))?
        .checked_mul(u64::from(size))?
        .checked_mul(4)
}

/// Reads a complete mip chain within the remaining cache byte budget.
fn array(input: &mut impl Read, remaining: &mut u64) -> Option<TextureArray> {
    let layers = word(input)?;
    let side = word(input)?;
    let levels = word(input)?;
    *remaining = remaining.checked_sub(12)?;
    if !array_header_is_valid(layers, side, levels) {
        return None;
    }
    let mut mips = Vec::with_capacity(levels as usize);
    for level in 0..levels {
        let size = side >> level;
        let count = mip_byte_len(layers, size)?;
        *remaining = remaining.checked_sub(count)?;
        let mut rgba8 = vec![0; usize::try_from(count).ok()?];
        input.read_exact(&mut rgba8).ok()?;
        mips.push(TextureMip {
            size,
            rgba8: rgba8.into_boxed_slice(),
        });
    }
    Some(TextureArray {
        layers,
        mips: mips.into_boxed_slice(),
    })
}

/// Reads six bounded arrays and their reference table for the expected key.
fn decode(input: &mut impl Read, size: u64, key: &[u8; 32], max_bytes: u64) -> Option<Payload> {
    if size > max_bytes {
        return None;
    }
    let mut magic = [0; 8];
    input.read_exact(&mut magic).ok()?;
    let mut stored_key = [0; 32];
    input.read_exact(&mut stored_key).ok()?;
    if &magic != MAGIC || &stored_key != key {
        return None;
    }
    let mut remaining = size.checked_sub(40)?;
    let color = [array(input, &mut remaining)?, array(input, &mut remaining)?];
    let normal = [array(input, &mut remaining)?, array(input, &mut remaining)?];
    let material = [array(input, &mut remaining)?, array(input, &mut remaining)?];
    let reference_words = word(input)? as usize;
    remaining = remaining.checked_sub(4)?;
    if remaining != reference_words as u64 * 4 {
        return None;
    }
    let references = (0..reference_words)
        .map(|_| word(input))
        .collect::<Option<Vec<_>>>()?;
    Some(Payload {
        color,
        normal,
        material,
        references: references.into_boxed_slice(),
    })
}

/// Loads a bounded cache payload for the current pack fingerprint.
pub(super) fn load(key: &[u8; 32]) -> Option<Payload> {
    let mut file = fs::File::open(CACHE_PATH).ok()?;
    let size = file.metadata().ok()?.len();
    let result = decode(&mut file, size, key, MAX_CACHE_BYTES)?;
    Some(result)
}

/// Writes validated mip arrays and their reference table in cache order.
fn encode(output: &mut impl Write, key: &[u8; 32], payload: &Payload) -> std::io::Result<()> {
    output.write_all(MAGIC)?;
    output.write_all(key)?;
    for page in payload
        .color
        .iter()
        .chain(&payload.normal)
        .chain(&payload.material)
    {
        let side = page.mips.first().map_or(0, |mip| mip.size);
        let levels = u32::try_from(page.mips.len())
            .map_err(|_| std::io::Error::other("too many cache mip levels"))?;
        if !array_header_is_valid(page.layers, side, levels)
            || page.mips.iter().enumerate().any(|(level, mip)| {
                mip.size != side >> level
                    || mip_byte_len(page.layers, mip.size) != u64::try_from(mip.rgba8.len()).ok()
            })
        {
            return Err(std::io::Error::other("invalid cache texture array"));
        }
        output.write_all(&page.layers.to_le_bytes())?;
        output.write_all(&side.to_le_bytes())?;
        output.write_all(&levels.to_le_bytes())?;
        for mip in &page.mips {
            output.write_all(&mip.rgba8)?;
        }
    }
    let references = u32::try_from(payload.references.len())
        .map_err(|_| std::io::Error::other("too many cache references"))?;
    output.write_all(&references.to_le_bytes())?;
    for reference in &payload.references {
        output.write_all(&reference.to_le_bytes())?;
    }
    Ok(())
}

/// Counts exactly the bytes emitted by the encoder without allocating a second payload.
#[derive(Default)]
struct EncodedSize(u64);

impl Write for EncodedSize {
    /// Accepts an entire write and records its length without inspecting its bytes.
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = u64::try_from(bytes.len())
            .ok()
            .and_then(|size| self.0.checked_add(size))
            .ok_or_else(|| std::io::Error::other("cache size overflow"))?;
        Ok(bytes.len())
    }

    /// Counting has no buffered output to flush.
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Preflights the actual encoding, including every header and reference word.
fn encoded_size(key: &[u8; 32], payload: &Payload) -> std::io::Result<u64> {
    let mut size = EncodedSize::default();
    encode(&mut size, key, payload)?;
    Ok(size.0)
}

/// Writes complete encodings within the reader's limit; oversized inputs touch no files.
/// `path` must name a file with a parent directory.
fn save_to(
    path: &Path,
    key: &[u8; 32],
    payload: &Payload,
    max_bytes: u64,
) -> std::io::Result<bool> {
    if encoded_size(key, payload)? > max_bytes {
        return Ok(false);
    }
    let temporary = path.with_file_name(format!(
        "{}.{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| -> std::io::Result<()> {
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::File::create(&temporary)?;
        encode(&mut file, key, payload)?;
        file.sync_all()?;
        drop(file);
        if path.is_file() {
            fs::remove_file(path)?;
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map(|()| true)
}

/// Shares the loader's budget and reports an oversized optional cache once per process.
pub(super) fn save(key: &[u8; 32], payload: &Payload) {
    match save_to(Path::new(CACHE_PATH), key, payload, MAX_CACHE_BYTES) {
        Ok(true) => {}
        Ok(false) => {
            if !OVERSIZED_CACHE_WARNED.swap(true, Ordering::Relaxed) {
                bevy::log::warn!(
                    "authored Enhanced texture cache exceeds {MAX_CACHE_BYTES} bytes; skipping cache write"
                );
            }
        }
        Err(error) => bevy::log::warn!("authored Enhanced texture cache: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referenced_images_in_any_directory_invalidate_cached_artwork() {
        let root = tempfile::tempdir().unwrap();
        let pack = PbrPack::new(root.path(), None);
        let packs = std::slice::from_ref(&pack);
        for directory in ["src", "target", "node_modules", ".local", ".git"] {
            let texture = format!("textures/blocks/{directory}/cache_fixture");
            let path = root.path().join(format!("{texture}.png"));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 80, 140, 255]))
                .save(&path)
                .unwrap();
            let before = fingerprint(packs, &Default::default()).unwrap();
            let mut cached = Vec::new();
            encode(&mut cached, &before, &payload(1, 1)).unwrap();
            image::RgbaImage::from_pixel(2, 2, image::Rgba([140, 80, 20, 255]))
                .save(&path)
                .unwrap();
            let decoded = pack_compiler::pbr::load_pbr_texture(packs, &texture)
                .unwrap()
                .unwrap();
            assert_eq!(
                decoded.frame(0, 1).unwrap().color.get_pixel(0, 0).0,
                [140, 80, 20, 255]
            );
            let after = fingerprint(packs, &Default::default()).unwrap();
            assert_ne!(before, after, "referenced image in {directory}");
            assert!(
                decode(
                    &mut cached.as_slice(),
                    cached.len() as u64,
                    &after,
                    MAX_CACHE_BYTES
                )
                .is_none()
            );
        }
    }

    /// Builds small complete mip chains with distinct bytes for each material map and page.
    fn payload(side: u32, layers: u32) -> Payload {
        let array = |seed: u8| TextureArray {
            layers,
            mips: (0..=side.ilog2())
                .map(|level| {
                    let size = side >> level;
                    TextureMip {
                        size,
                        rgba8: vec![seed + level as u8; (layers * size * size * 4) as usize]
                            .into_boxed_slice(),
                    }
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        };
        Payload {
            color: std::array::from_fn(|page| array(16 + page as u8)),
            normal: std::array::from_fn(|page| array(64 + page as u8)),
            material: std::array::from_fn(|page| array(128 + page as u8)),
            references: render_scene::EnhancedTextureAssets::material_references(
                std::collections::BTreeMap::from([((1, 0), assets::texture::PBR_REF_COLOR)]),
            ),
        }
    }

    /// Rejection happens before directories, temporary files, or the previous cache are touched.
    #[test]
    fn oversized_payload_is_not_written_or_allowed_to_replace_a_cache() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache/maps.bin");
        let key = [11; 32];
        let payload = payload(4, 2);
        let limit = encoded_size(&key, &payload).unwrap() - 1;
        assert!(!save_to(&path, &key, &payload, limit).unwrap());
        assert!(!path.parent().unwrap().exists());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"previous cache").unwrap();
        assert!(!save_to(&path, &key, &payload, limit).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"previous cache");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    /// Writer admission at a small byte limit always produces a complete reader round trip.
    #[test]
    fn every_accepted_cache_decodes_at_the_same_limit() {
        let directory = tempfile::tempdir().unwrap();
        let key = [23; 32];
        for side in [1, 2, 4, 8] {
            for layers in [1, 2] {
                let payload = payload(side, layers);
                let size = encoded_size(&key, &payload).unwrap();
                for limit in [size - 1, size, size + 1] {
                    let path = directory
                        .path()
                        .join(format!("{side}-{layers}-{limit}.cache"));
                    let accepted = save_to(&path, &key, &payload, limit).unwrap();
                    assert_eq!(accepted, size <= limit);
                    if !accepted {
                        assert!(!path.exists());
                        continue;
                    }
                    let bytes = fs::read(&path).unwrap();
                    assert_eq!(bytes.len() as u64, size);
                    assert!(bytes.len() as u64 <= limit);
                    let restored = decode(&mut bytes.as_slice(), size, &key, limit).unwrap();
                    assert_eq!(restored, payload);
                    assert!(restored.into_assets().is_some());
                }
            }
        }
    }

    /// Invalid array metadata cannot produce a small but unreadable cache.
    #[test]
    fn malformed_payload_is_rejected_before_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("maps.bin");
        let key = [31; 32];
        let mut payload = payload(2, 1);
        payload.normal[0].mips[1].rgba8 = Box::new([]);
        assert!(save_to(&path, &key, &payload, MAX_CACHE_BYTES).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn cached_material_references_keep_shared_carrier_surfaces_distinct() {
        let array = TextureArray {
            layers: 2,
            mips: Box::new([TextureMip {
                size: 1,
                rgba8: vec![128; 8].into_boxed_slice(),
            }]),
        };
        let payload = Payload {
            color: [array.clone(), array.clone()],
            normal: [array.clone(), array.clone()],
            material: [array.clone(), array],
            references: render_scene::EnhancedTextureAssets::material_references(
                std::collections::BTreeMap::from([
                    ((1, 0), assets::texture::PBR_REF_COLOR),
                    ((2, 0), assets::texture::PBR_REF_COLOR | 1),
                ]),
            ),
        };
        let key = [17; 32];
        let mut bytes = Vec::new();
        encode(&mut bytes, &key, &payload).unwrap();
        let restored = decode(
            &mut bytes.as_slice(),
            bytes.len() as u64,
            &key,
            MAX_CACHE_BYTES,
        )
        .unwrap();
        assert_eq!(restored.references, payload.references);
        let maps = restored.into_assets().unwrap();
        let texture = assets::TextureRef::DIAGNOSTIC;
        assert_eq!(
            maps.material_reference(1, texture),
            assets::texture::PBR_REF_COLOR
        );
        assert_eq!(
            maps.material_reference(2, texture),
            assets::texture::PBR_REF_COLOR | 1
        );
        assert_eq!(maps.material_reference(3, texture), u32::MAX);
    }

    #[test]
    fn cache_rejects_mips_from_clamped_signed_normal_filtering() {
        let key = [7; 32];
        let mut bytes = Vec::new();
        encode(&mut bytes, &key, &payload(1, 1)).unwrap();
        assert!(
            decode(
                &mut bytes.as_slice(),
                bytes.len() as u64,
                &key,
                MAX_CACHE_BYTES
            )
            .is_some()
        );
        bytes[..MAGIC.len()].copy_from_slice(b"CINPBR15");
        assert!(
            decode(
                &mut bytes.as_slice(),
                bytes.len() as u64,
                &key,
                MAX_CACHE_BYTES
            )
            .is_none()
        );
    }

    #[test]
    fn cache_rejects_wrong_keys_truncation_and_oversized_allocation_claims() {
        let key = [7; 32];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&key);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&(assets::texture::PBR_TILE_SIZE.ilog2() + 1).to_le_bytes());
        assert!(
            decode(
                &mut bytes.as_slice(),
                bytes.len() as u64,
                &key,
                MAX_CACHE_BYTES
            )
            .is_none()
        );
        assert!(
            decode(
                &mut bytes.as_slice(),
                bytes.len() as u64,
                &[8; 32],
                MAX_CACHE_BYTES
            )
            .is_none()
        );
        assert!(decode(&mut &bytes[..10], 10, &key, MAX_CACHE_BYTES).is_none());
    }
}
