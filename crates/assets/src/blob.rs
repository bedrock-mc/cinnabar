use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

use crate::{
    AssetError, CompiledAssets, DIAGNOSTIC_MATERIAL, MAX_MATERIALS, MAX_TEXTURE_LAYERS, Material,
    image::MIP_COUNT, image::TILE_SIZE,
};

pub const BLOB_MAGIC: [u8; 8] = *b"MCBEAS01";
pub const BLOB_VERSION: u32 = 1;

const HEADER_BYTES: usize = 88;
const HASH_BYTES: usize = 32;
const VISUAL_BYTES: usize = 28;
const HASH_ENTRY_BYTES: usize = 8;
const MATERIAL_BYTES: usize = 8;
const MAX_VISUALS: usize = 65_536;

/// Serializes deterministic compiler output with checked offsets and a trailing SHA-256.
pub fn encode_blob(compiled: &CompiledAssets) -> Result<Box<[u8]>, AssetError> {
    let texture_bytes = validate_compiled(compiled)?;
    let visual_bytes = section_size(compiled.visuals.len(), VISUAL_BYTES, "visual")?;
    let hash_bytes = section_size(compiled.hashed.len(), HASH_ENTRY_BYTES, "hash lookup")?;
    let material_bytes = section_size(compiled.materials.len(), MATERIAL_BYTES, "material")?;

    let visuals_offset = HEADER_BYTES;
    let hashes_offset = add_section(visuals_offset, visual_bytes, "visual")?;
    let materials_offset = add_section(hashes_offset, hash_bytes, "hash lookup")?;
    let textures_offset = add_section(materials_offset, material_bytes, "material")?;
    let payload_length = add_section(textures_offset, texture_bytes, "texture")?;
    let total_length = add_section(payload_length, HASH_BYTES, "blob hash")?;

    let mut bytes = Vec::with_capacity(total_length);
    bytes.extend_from_slice(&BLOB_MAGIC);
    push_u32(&mut bytes, BLOB_VERSION);
    push_u32(&mut bytes, TILE_SIZE);
    push_u32(&mut bytes, MIP_COUNT);
    push_count(&mut bytes, compiled.visuals.len(), "visual count")?;
    push_count(&mut bytes, compiled.hashed.len(), "hash count")?;
    push_count(&mut bytes, compiled.materials.len(), "material count")?;
    push_u32(&mut bytes, compiled.textures.layers);
    push_u32(&mut bytes, 0);
    push_offset(&mut bytes, visuals_offset, "visual offset")?;
    push_offset(&mut bytes, hashes_offset, "hash offset")?;
    push_offset(&mut bytes, materials_offset, "material offset")?;
    push_offset(&mut bytes, textures_offset, "texture offset")?;
    push_offset(&mut bytes, texture_bytes, "texture length")?;
    push_offset(&mut bytes, payload_length, "payload length")?;
    debug_assert_eq!(bytes.len(), HEADER_BYTES);

    for visual in &compiled.visuals {
        for material in visual.faces {
            push_u32(&mut bytes, material);
        }
        bytes.push(visual.flags.bits());
        bytes.extend_from_slice(&[0; 3]);
    }
    debug_assert_eq!(bytes.len(), hashes_offset);
    for &(hash, visual) in &compiled.hashed {
        push_u32(&mut bytes, hash);
        push_u32(&mut bytes, visual);
    }
    debug_assert_eq!(bytes.len(), materials_offset);
    for material in &compiled.materials {
        push_u32(&mut bytes, material.layer);
        push_u32(&mut bytes, material.flags);
    }
    debug_assert_eq!(bytes.len(), textures_offset);
    for mip in &compiled.textures.mips {
        bytes.extend_from_slice(&mip.rgba8);
    }
    debug_assert_eq!(bytes.len(), payload_length);

    let hash = Sha256::digest(&bytes);
    bytes.extend_from_slice(&hash);
    debug_assert_eq!(bytes.len(), total_length);
    Ok(bytes.into_boxed_slice())
}

fn validate_compiled(compiled: &CompiledAssets) -> Result<usize, AssetError> {
    if compiled.visuals.len() > MAX_VISUALS {
        return Err(AssetError::TooManyRegistryRecords {
            count: compiled.visuals.len(),
            max: MAX_VISUALS,
        });
    }
    if compiled.hashed.len() > MAX_VISUALS {
        return Err(AssetError::TooManyRegistryRecords {
            count: compiled.hashed.len(),
            max: MAX_VISUALS,
        });
    }
    if compiled.materials.len() > MAX_MATERIALS {
        return Err(AssetError::TooManyMaterials {
            count: compiled.materials.len(),
            max: MAX_MATERIALS,
        });
    }
    let layers = compiled.textures.layers as usize;
    if layers > MAX_TEXTURE_LAYERS {
        return Err(AssetError::TooManyTextureLayers {
            count: layers,
            max: MAX_TEXTURE_LAYERS,
            key: None,
            path: None,
        });
    }
    if layers == 0 {
        return Err(invalid("texture array has no diagnostic layer"));
    }
    if compiled.materials.first() != Some(&Material { layer: 0, flags: 0 }) {
        return Err(invalid("material 0 is not the diagnostic layer"));
    }
    for (index, material) in compiled.materials.iter().enumerate() {
        if material.layer >= compiled.textures.layers {
            return Err(invalid(format!(
                "material {index} references layer {}, but there are {layers} layers",
                material.layer
            )));
        }
    }
    for (index, visual) in compiled.visuals.iter().enumerate() {
        if let Some(material) = visual
            .faces
            .iter()
            .copied()
            .find(|&material| material as usize >= compiled.materials.len())
        {
            return Err(invalid(format!(
                "visual {index} references material {material}, but there are {} materials",
                compiled.materials.len()
            )));
        }
    }
    let mut previous = None;
    for &(hash, visual) in &compiled.hashed {
        if previous.is_some_and(|previous| previous >= hash) {
            return Err(invalid("hashed lookup keys are not strictly increasing"));
        }
        if visual as usize >= compiled.visuals.len() {
            return Err(invalid(format!(
                "hash {hash:#010x} references visual {visual}, but there are {} visuals",
                compiled.visuals.len()
            )));
        }
        previous = Some(hash);
    }

    if compiled.textures.mips.len() != MIP_COUNT as usize {
        return Err(invalid(format!(
            "texture array has {} mip levels, expected {MIP_COUNT}",
            compiled.textures.mips.len()
        )));
    }
    let mut total = 0_usize;
    for (level, mip) in compiled.textures.mips.iter().enumerate() {
        let expected_size = TILE_SIZE >> level;
        if mip.size != expected_size {
            return Err(invalid(format!(
                "mip {level} is {}x{}, expected {expected_size}x{expected_size}",
                mip.size, mip.size
            )));
        }
        let pixels = section_size(expected_size as usize, expected_size as usize, "mip pixels")?;
        let rgba = section_size(pixels, 4, "mip RGBA")?;
        let expected_bytes = section_size(rgba, layers, "mip layers")?;
        if mip.rgba8.len() != expected_bytes {
            return Err(invalid(format!(
                "mip {level} has {} bytes, expected {expected_bytes}",
                mip.rgba8.len()
            )));
        }
        total = add_section(total, expected_bytes, "texture")?;
    }
    Ok(total)
}

/// Writes a blob through a unique sibling temporary file and an atomic rename.
pub fn write_blob_atomic(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| AssetError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let file_name = path.file_name().ok_or_else(|| AssetError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "output path has no file name"),
    })?;

    let mut temporary = None;
    for attempt in 0..64_u32 {
        let candidate = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            attempt
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(AssetError::Io {
                    path: candidate,
                    source,
                });
            }
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| AssetError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve an atomic output file",
        ),
    })?;

    let write_result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary_path, path)
    })();
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(AssetError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn section_size(count: usize, width: usize, section: &'static str) -> Result<usize, AssetError> {
    count
        .checked_mul(width)
        .ok_or(AssetError::BlobSizeOverflow { section })
}

fn add_section(offset: usize, length: usize, section: &'static str) -> Result<usize, AssetError> {
    offset
        .checked_add(length)
        .ok_or(AssetError::BlobSizeOverflow { section })
}

fn push_count(bytes: &mut Vec<u8>, count: usize, section: &'static str) -> Result<(), AssetError> {
    let count = u32::try_from(count).map_err(|_| AssetError::BlobSizeOverflow { section })?;
    push_u32(bytes, count);
    Ok(())
}

fn push_offset(
    bytes: &mut Vec<u8>,
    offset: usize,
    section: &'static str,
) -> Result<(), AssetError> {
    let offset = u64::try_from(offset).map_err(|_| AssetError::BlobSizeOverflow { section })?;
    bytes.extend_from_slice(&offset.to_le_bytes());
    Ok(())
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

const _: () = assert!(DIAGNOSTIC_MATERIAL == 0);
