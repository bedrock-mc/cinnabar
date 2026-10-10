//! A content-addressed local cache avoids decoding unchanged authored maps on each launch.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use assets::{TextureArray, TextureMip};
use pack_compiler::pbr::PbrPack;
use sha2::{Digest, Sha256};

use super::{LOW_PBR_TILE_SIZE, Source};

const CACHE_PATH: &str = ".local/enhanced-pbr.cache";
// Includes overlay masks, animation timing, and resolved terrain aliases.
const MAGIC: &[u8; 8] = b"CINPBR11";
const MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub(super) struct Payload {
    pub(super) color: [TextureArray; 2],
    pub(super) normal: [TextureArray; 2],
    pub(super) material: [TextureArray; 2],
    pub(super) references: Box<[u32]>,
}

impl Payload {
    pub(super) fn into_assets(self) -> Option<render::EnhancedTextureAssets> {
        render::EnhancedTextureAssets::new(self.color, self.normal, self.material, self.references)
    }
}

fn files(root: &Path, result: &mut Vec<PathBuf>) -> Option<()> {
    for entry in fs::read_dir(root).ok()? {
        let entry = entry.ok()?;
        let kind = entry.file_type().ok()?;
        if kind.is_symlink() {
            return None;
        }
        let path = entry.path();
        if kind.is_dir() {
            let name = entry.file_name();
            if [".git", "src", "node_modules", "target", ".local"]
                .iter()
                .any(|skip| name == *skip)
            {
                continue;
            }
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

pub(super) fn fingerprint(
    packs: &[PbrPack],
    sources: &std::collections::BTreeMap<String, Vec<Source>>,
) -> Option<[u8; 32]> {
    let mut digest = Sha256::new();
    digest.update(MAGIC);
    digest.update(assets::PBR_TILE_SIZE.to_le_bytes());
    digest.update(LOW_PBR_TILE_SIZE.to_le_bytes());
    digest.update(assets::PBR_HEIGHT_SCALE.to_bits().to_le_bytes());
    for flag in [
        assets::PBR_REF_COLOR,
        assets::PBR_REF_NORMAL,
        assets::PBR_REF_HEIGHT,
        assets::PBR_REF_MATERIAL,
        assets::PBR_REF_LABPBR,
        assets::PBR_REF_OCCLUSION,
        assets::PBR_REF_SUBSURFACE,
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
        for alias in sources.keys() {
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
    for (alias, entries) in sources {
        digest.update((alias.len() as u64).to_le_bytes());
        digest.update(alias.as_bytes());
        for source in entries {
            digest.update(source.texture.to_le_bytes());
            digest.update((source.frame as u64).to_le_bytes());
            digest.update((source.count as u64).to_le_bytes());
            digest.update([u8::from(source.cutout), u8::from(source.overlay_mask)]);
        }
    }
    Some(digest.finalize().into())
}

fn word(input: &mut impl Read) -> Option<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn array(input: &mut impl Read, remaining: &mut u64) -> Option<TextureArray> {
    let layers = word(input)?;
    let side = word(input)?;
    let levels = word(input)?;
    *remaining = remaining.checked_sub(12)?;
    if layers == 0
        || layers > assets::MAX_TEXTURE_LAYERS as u32
        || !side.is_power_of_two()
        || side > assets::PBR_TILE_SIZE
        || levels != side.ilog2() + 1
    {
        return None;
    }
    let mut mips = Vec::with_capacity(levels as usize);
    for level in 0..levels {
        let size = side >> level;
        let count = (layers as u64)
            .checked_mul(size as u64)?
            .checked_mul(size as u64)?
            .checked_mul(4)?;
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

fn decode(input: &mut impl Read, size: u64, key: &[u8; 32]) -> Option<Payload> {
    if size > MAX_CACHE_BYTES {
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
    let reference_bytes = (assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS * 4) as u64;
    if remaining != reference_bytes {
        return None;
    }
    let references = (0..assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS)
        .map(|_| word(input))
        .collect::<Option<Vec<_>>>()?;
    Some(Payload {
        color,
        normal,
        material,
        references: references.into_boxed_slice(),
    })
}

pub(super) fn load(key: &[u8; 32]) -> Option<Payload> {
    let mut file = fs::File::open(CACHE_PATH).ok()?;
    let size = file.metadata().ok()?.len();
    let result = decode(&mut file, size, key)?;
    Some(result)
}

fn encode(output: &mut impl Write, key: &[u8; 32], payload: &Payload) -> std::io::Result<()> {
    output.write_all(MAGIC)?;
    output.write_all(key)?;
    for page in payload
        .color
        .iter()
        .chain(&payload.normal)
        .chain(&payload.material)
    {
        output.write_all(&page.layers.to_le_bytes())?;
        output.write_all(&page.mips.first().map_or(0, |mip| mip.size).to_le_bytes())?;
        output.write_all(&(page.mips.len() as u32).to_le_bytes())?;
        for mip in &page.mips {
            output.write_all(&mip.rgba8)?;
        }
    }
    for reference in &payload.references {
        output.write_all(&reference.to_le_bytes())?;
    }
    Ok(())
}

pub(super) fn save(key: &[u8; 32], payload: &Payload) {
    let temporary = PathBuf::from(format!("{CACHE_PATH}.{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        fs::create_dir_all(Path::new(CACHE_PATH).parent().unwrap())?;
        let mut file = fs::File::create(&temporary)?;
        encode(&mut file, key, payload)?;
        file.sync_all()?;
        drop(file);
        if Path::new(CACHE_PATH).is_file() {
            fs::remove_file(CACHE_PATH)?;
        }
        fs::rename(&temporary, CACHE_PATH)
    })();
    if let Err(error) = result {
        eprintln!("authored Enhanced texture cache: {error}");
        let _ = fs::remove_file(temporary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_rejects_wrong_keys_truncation_and_oversized_allocation_claims() {
        let key = [7; 32];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&key);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&(assets::PBR_TILE_SIZE.ilog2() + 1).to_le_bytes());
        assert!(decode(&mut bytes.as_slice(), bytes.len() as u64, &key).is_none());
        assert!(decode(&mut bytes.as_slice(), bytes.len() as u64, &[8; 32]).is_none());
        assert!(decode(&mut &bytes[..10], 10, &key).is_none());
    }
}
