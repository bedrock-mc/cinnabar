//! Particle asset compiler: canonicalizes the pack's `particles/*.json` effects and packs the
//! textures they can reference into the hash-pinned particle carrier.
//!
//! `atlas.terrain`/`atlas.items` are resolved at runtime and have no file here; a texture
//! that is oversized or not a decodable PNG is skipped and counted.

use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

use ::image::{ImageFormat, ImageReader, Limits};
use assets::{
    AssetError, MAX_PARTICLE_EFFECT_BYTES, MAX_PARTICLE_TEXTURE_SIDE, ParticleEffectFile,
    ParticleTexture, RuntimeParticleAssets, canonical_source_manifest_sha256,
    encode_particle_catalog,
};
use sha2::{Digest, Sha256};

const MAX_TEXTURE_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_WALK_ENTRIES: usize = 100_000;

#[derive(Debug)]
pub struct CompiledParticleCarrier {
    pub bytes: Vec<u8>,
    pub report: ParticleCompileReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticleCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub textures_packed: usize,
    pub textures_skipped: usize,
    pub effects: usize,
    /// Effect files that are not json, lack an identifier, or exceed the byte bound.
    pub effects_skipped: usize,
    pub texture_pixel_bytes: usize,
}

/// Compiles the pack's particle effects and textures into the carrier, pinned to the
/// canonical `source_manifest` digest.
pub fn compile_particle_assets(
    pack: &Path,
    source_manifest: &[u8],
) -> Result<CompiledParticleCarrier, AssetError> {
    let source_manifest_sha256 = canonical_source_manifest_sha256(source_manifest);
    if source_manifest_sha256 == [0; 32] {
        return Err(invalid("particle source manifest digest is unset"));
    }

    let mut budget = MAX_WALK_ENTRIES;
    let mut json_paths = Vec::new();
    walk(
        &pack.join("particles"),
        pack,
        "json",
        &mut json_paths,
        &mut budget,
    )?;
    let mut effects = Vec::new();
    let mut effects_skipped = 0usize;
    for relative in &json_paths {
        match read_effect(pack, relative)? {
            Some(effect) => effects.push(effect),
            None => effects_skipped += 1,
        }
    }
    effects.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    effects.dedup_by(|a, b| a.identifier == b.identifier);

    let mut png_paths = std::collections::BTreeSet::new();
    for effect in &effects {
        let value: serde_json::Value =
            serde_json::from_slice(&effect.bytes).expect("read_effect emits canonical JSON");
        if let Some(texture) = value
            .pointer("/particle_effect/description/basic_render_parameters/texture")
            .and_then(serde_json::Value::as_str)
            .and_then(texture_file)
        {
            png_paths.insert(texture);
        }
    }
    let flame_file = format!("{}.png", assets::ACTOR_FLAME_TEXTURE);
    if pack.join(&flame_file).is_file() {
        png_paths.insert(flame_file);
    }
    let mut textures = Vec::new();
    let mut textures_skipped = 0usize;
    for relative in &png_paths {
        match decode_texture(pack, relative)? {
            Some(texture) => textures.push(texture),
            None => textures_skipped += 1,
        }
    }
    textures.sort_by(|a, b| a.path.cmp(&b.path));
    textures.dedup_by(|a, b| a.path == b.path);

    let texture_pixel_bytes = textures.iter().map(|texture| texture.rgba8.len()).sum();
    let bytes = encode_particle_catalog(source_manifest_sha256, &textures, &effects)?;
    Ok(CompiledParticleCarrier {
        report: ParticleCompileReport {
            source_manifest_sha256,
            carrier_sha256: Sha256::digest(&bytes).into(),
            textures_packed: textures.len(),
            textures_skipped,
            effects: effects.len(),
            effects_skipped,
            texture_pixel_bytes,
        },
        bytes,
    })
}

/// Converts a pack-local texture reference to a PNG path; atlas names have no file.
fn texture_file(texture: &str) -> Option<String> {
    if !texture.starts_with("textures/")
        || texture.contains(['\\', ':'])
        || texture
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return None;
    }
    Some(format!(
        "{}.png",
        texture.strip_suffix(".png").unwrap_or(texture)
    ))
}

/// Decodes one png; `None` when it is oversized or not a decodable PNG.
fn decode_texture(pack: &Path, relative: &str) -> Result<Option<ParticleTexture>, AssetError> {
    let logical = relative.strip_suffix(".png").unwrap_or(relative);
    let path = pack.join(relative);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AssetError::TextureIo {
                key: logical.into(),
                path: path.clone(),
                source,
            });
        }
    };
    let mut bytes = Vec::new();
    file.take((MAX_TEXTURE_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::TextureIo {
            key: logical.into(),
            path: path.clone(),
            source,
        })?;
    if bytes.len() > MAX_TEXTURE_SOURCE_BYTES {
        return Ok(None);
    }
    let Ok((width, height)) =
        ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png).into_dimensions()
    else {
        return Ok(None);
    };
    if width == 0
        || height == 0
        || width > MAX_PARTICLE_TEXTURE_SIDE
        || height > MAX_PARTICLE_TEXTURE_SIDE
    {
        return Ok(None);
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_PARTICLE_TEXTURE_SIDE);
    limits.max_image_height = Some(MAX_PARTICLE_TEXTURE_SIDE);
    limits.max_alloc =
        Some(u64::from(MAX_PARTICLE_TEXTURE_SIDE) * u64::from(MAX_PARTICLE_TEXTURE_SIDE) * 8);
    reader.limits(limits);
    let Ok(decoded) = reader.decode() else {
        return Ok(None);
    };
    Ok(Some(ParticleTexture {
        path: logical.into(),
        width,
        height,
        rgba8: Arc::from(decoded.into_rgba8().into_raw()),
    }))
}

/// Reads one effect, stripping comments and re-serializing canonical json; `None` when it
/// is unusable.
fn read_effect(pack: &Path, relative: &str) -> Result<Option<ParticleEffectFile>, AssetError> {
    let path = pack.join(relative);
    let file = fs::File::open(&path).map_err(|source| AssetError::Io {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_PARTICLE_EFFECT_BYTES * 2 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.clone(),
            source,
        })?;
    let Ok(value) = pack_json::parse_jsonc::<serde_json::Value>(&bytes) else {
        return Ok(None);
    };
    let Some(identifier) = value
        .pointer("/particle_effect/description/identifier")
        .and_then(serde_json::Value::as_str)
        .filter(|identifier| !identifier.is_empty())
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    let canonical = serde_json::to_vec(&value).map_err(|source| AssetError::Json {
        path: path.clone(),
        source,
    })?;
    if canonical.len() > MAX_PARTICLE_EFFECT_BYTES {
        return Ok(None);
    }
    Ok(Some(ParticleEffectFile {
        identifier: identifier.into(),
        bytes: Arc::from(canonical),
    }))
}

fn walk(
    dir: &Path,
    pack_root: &Path,
    extension: &str,
    out: &mut Vec<String>,
    budget: &mut usize,
) -> Result<(), AssetError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(AssetError::Io {
                path: dir.to_path_buf(),
                source,
            });
        }
    };
    for entry in entries {
        let entry = entry.map_err(|source| AssetError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        if *budget == 0 {
            return Err(invalid("particle asset tree exceeds the traversal bound"));
        }
        *budget -= 1;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| AssetError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            walk(&path, pack_root, extension, out, budget)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
            && let Some(relative) = relative_posix(&path, pack_root)
        {
            out.push(relative);
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

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

/// Decodes the carrier this compiler produced, to confirm a build round-trips.
pub fn decode_particle_carrier(bytes: &[u8]) -> Result<RuntimeParticleAssets, AssetError> {
    RuntimeParticleAssets::decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &[u8] = b"{\"schema\":1}";

    #[test]
    fn packs_textures_referenced_outside_the_particle_directories() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pack = root.join(assets::vanilla_source().resource_pack_dir());
        if !pack.join("particles/oozing_emitter.json").is_file()
            || !pack.join("particles/weaving_emitter.json").is_file()
        {
            eprintln!(
                "skipping packs_textures_referenced_outside_the_particle_directories: missing pinned particle effects"
            );
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let mut textures = Vec::new();
        for file in ["oozing_emitter.json", "weaving_emitter.json"] {
            let bytes = fs::read(pack.join("particles").join(file)).unwrap();
            let value: serde_json::Value = pack_json::parse_jsonc(&bytes).unwrap();
            let texture = value
                .pointer("/particle_effect/description/basic_render_parameters/texture")
                .unwrap()
                .as_str()
                .unwrap();
            textures.push(texture.to_owned());
            write(directory.path(), &format!("particles/{file}"), &bytes);
            write(directory.path(), &format!("{texture}.png"), &png(16, 16));
        }
        let compiled =
            compile_particle_assets(directory.path(), assets::VANILLA_SOURCE_MANIFEST.as_bytes())
                .unwrap();
        let assets = decode_particle_carrier(&compiled.bytes).unwrap();
        for texture in textures {
            assert!(assets.texture(&texture).is_some(), "missing {texture}");
        }
        assert_eq!(compiled.report.textures_packed, 2);
    }

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut buffer = Vec::new();
        ::image::RgbaImage::from_pixel(width, height, ::image::Rgba([9, 8, 7, 255]))
            .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
            .unwrap();
        buffer
    }

    #[test]
    fn compiles_commented_effects_and_textures() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "textures/particle/particles.png", &png(8, 8));
        write(root, "textures/particle/broken.png", b"not a png");
        write(
            root,
            "particles/a.json",
            b"{\"particle_effect\":{\"description\":{\"identifier\":\"minecraft:a\",\"basic_render_parameters\":{\"texture\":\"textures/particle/particles\"}}} // c\n}",
        );
        write(root, "particles/nameless.json", b"{\"particle_effect\":{}}");
        let compiled = compile_particle_assets(root, MANIFEST).unwrap();
        assert_eq!(compiled.report.textures_packed, 1);
        assert_eq!(compiled.report.textures_skipped, 0);
        assert_eq!(compiled.report.effects, 1);
        assert_eq!(compiled.report.effects_skipped, 1);
        let assets = decode_particle_carrier(&compiled.bytes).unwrap();
        assert!(assets.effect("minecraft:a").is_some());
        assert_eq!(
            assets.texture("textures/particle/particles").unwrap().width,
            8
        );
    }
}
