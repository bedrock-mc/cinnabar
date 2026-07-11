use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::{
    AssetError, BLOB_MAGIC, BLOB_VERSION, BlockFace, BlockFlags, BlockVisual, DIAGNOSTIC_MATERIAL,
    MAX_MATERIALS, MAX_TEXTURE_LAYERS, MIP_COUNT, Material, TILE_SIZE, TextureArray, TextureMip,
};

const HEADER_BYTES: usize = 88;
const TRAILING_HASH_BYTES: usize = 32;
const VISUAL_BYTES: usize = 28;
const HASH_ENTRY_BYTES: usize = 8;
const MATERIAL_BYTES: usize = 8;
const MAX_VISUALS: usize = 65_536;

/// The network-ID representation selected explicitly from StartGame for one session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkIdMode {
    Sequential,
    Hashed,
}

/// One resolved face's compact material reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedFace {
    material_id: u32,
}

impl ResolvedFace {
    #[must_use]
    pub const fn material_id(self) -> u32 {
        self.material_id
    }
}

/// An immutable block visual returned by a session-mode-specific lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedBlock {
    visual: BlockVisual,
    known: bool,
}

impl ResolvedBlock {
    const fn known(visual: BlockVisual) -> Self {
        Self {
            visual,
            known: true,
        }
    }

    const fn diagnostic() -> Self {
        Self {
            visual: BlockVisual {
                faces: [DIAGNOSTIC_MATERIAL; 6],
                flags: BlockFlags::empty(),
            },
            known: false,
        }
    }

    #[must_use]
    pub const fn is_known(self) -> bool {
        self.known
    }

    #[must_use]
    pub const fn flags(self) -> BlockFlags {
        self.visual.flags
    }

    #[must_use]
    pub const fn face(self, face: BlockFace) -> ResolvedFace {
        ResolvedFace {
            material_id: self.visual.faces[face as usize],
        }
    }
}

/// Validated runtime tables used directly by worker mesh jobs and render preparation.
pub struct RuntimeAssets {
    visuals: Box<[BlockVisual]>,
    hashed: Box<[(u32, u32)]>,
    materials: Box<[Material]>,
    textures: TextureArray,
    missing: AtomicU64,
}

impl RuntimeAssets {
    /// Decodes and validates a complete `MCBEAS01` blob before allocating runtime sections.
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        let header = Header::decode(bytes)?;
        header.validate_layout(bytes.len())?;
        validate_hash(bytes, header.payload_length)?;

        let visual_bytes = &bytes[header.visuals_offset..header.hashes_offset];
        let hash_bytes = &bytes[header.hashes_offset..header.materials_offset];
        let material_bytes = &bytes[header.materials_offset..header.textures_offset];
        let texture_bytes =
            &bytes[header.textures_offset..header.textures_offset + header.textures_length];

        validate_visuals(visual_bytes, header.material_count)?;
        validate_hashes(hash_bytes, header.visual_count)?;
        validate_materials(material_bytes, header.layer_count)?;

        let visuals = decode_visuals(visual_bytes);
        let hashed = decode_hashes(hash_bytes);
        let materials = decode_materials(material_bytes);
        let textures = decode_textures(texture_bytes, header.layer_count)?;

        Ok(Self {
            visuals,
            hashed,
            materials,
            textures,
            missing: AtomicU64::new(0),
        })
    }

    /// Resolves only in the explicitly selected network-ID namespace.
    #[must_use]
    pub fn resolve(&self, mode: NetworkIdMode, value: u32) -> ResolvedBlock {
        let resolved = match mode {
            NetworkIdMode::Sequential => self.visuals.get(value as usize).copied(),
            NetworkIdMode::Hashed => self
                .hashed
                .binary_search_by_key(&value, |entry| entry.0)
                .ok()
                .and_then(|index| self.visuals.get(self.hashed[index].1 as usize))
                .copied(),
        };

        resolved.map_or_else(
            || {
                self.record_missing();
                ResolvedBlock::diagnostic()
            },
            ResolvedBlock::known,
        )
    }

    /// Returns a material, falling back to material zero for an invalid external ID.
    #[must_use]
    pub fn material(&self, id: u32) -> Material {
        self.materials.get(id as usize).copied().unwrap_or_else(|| {
            self.record_missing();
            self.materials[DIAGNOSTIC_MATERIAL as usize]
        })
    }

    #[must_use]
    pub const fn texture_array(&self) -> &TextureArray {
        &self.textures
    }

    /// Total unknown block-state and material lookups. No per-ID collection is retained.
    #[must_use]
    pub fn missing_count(&self) -> u64 {
        self.missing.load(Ordering::Relaxed)
    }

    fn record_missing(&self) {
        self.missing.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
struct Header {
    visual_count: usize,
    hash_count: usize,
    material_count: usize,
    layer_count: usize,
    visuals_offset: usize,
    hashes_offset: usize,
    materials_offset: usize,
    textures_offset: usize,
    textures_length: usize,
    payload_length: usize,
}

impl Header {
    fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + TRAILING_HASH_BYTES {
            return Err(invalid(format!(
                "blob is {} bytes, shorter than the minimum {}",
                bytes.len(),
                HEADER_BYTES + TRAILING_HASH_BYTES
            )));
        }
        let mut reader = HeaderReader::new(&bytes[..HEADER_BYTES]);
        if reader.read_exact(8)? != BLOB_MAGIC {
            return Err(invalid("invalid MCBEAS01 magic"));
        }
        let version = reader.read_u32()?;
        if version != BLOB_VERSION {
            return Err(invalid(format!(
                "unsupported blob version {version}, expected {BLOB_VERSION}"
            )));
        }
        let tile_size = reader.read_u32()?;
        if tile_size != TILE_SIZE {
            return Err(invalid(format!(
                "unsupported tile size {tile_size}, expected {TILE_SIZE}"
            )));
        }
        let mip_count = reader.read_u32()?;
        if mip_count != MIP_COUNT {
            return Err(invalid(format!(
                "unsupported mip count {mip_count}, expected {MIP_COUNT}"
            )));
        }

        let visual_count = reader.read_u32()? as usize;
        let hash_count = reader.read_u32()? as usize;
        let material_count = reader.read_u32()? as usize;
        let layer_count = reader.read_u32()? as usize;
        if reader.read_u32()? != 0 {
            return Err(invalid("reserved header word is not zero"));
        }

        validate_count("visual", visual_count, MAX_VISUALS)?;
        validate_count("hash lookup", hash_count, MAX_VISUALS)?;
        validate_count("material", material_count, MAX_MATERIALS)?;
        validate_count("texture layer", layer_count, MAX_TEXTURE_LAYERS)?;
        if material_count == 0 {
            return Err(invalid("material table has no diagnostic material"));
        }
        if layer_count == 0 {
            return Err(invalid("texture array has no diagnostic layer"));
        }

        Ok(Self {
            visual_count,
            hash_count,
            material_count,
            layer_count,
            visuals_offset: reader.read_usize()?,
            hashes_offset: reader.read_usize()?,
            materials_offset: reader.read_usize()?,
            textures_offset: reader.read_usize()?,
            textures_length: reader.read_usize()?,
            payload_length: reader.read_usize()?,
        })
    }

    fn validate_layout(self, input_length: usize) -> Result<(), AssetError> {
        let expected_visuals = HEADER_BYTES;
        let expected_hashes = checked_add(
            expected_visuals,
            checked_mul(self.visual_count, VISUAL_BYTES, "visual section")?,
            "visual section",
        )?;
        let expected_materials = checked_add(
            expected_hashes,
            checked_mul(self.hash_count, HASH_ENTRY_BYTES, "hash section")?,
            "hash section",
        )?;
        let expected_textures = checked_add(
            expected_materials,
            checked_mul(self.material_count, MATERIAL_BYTES, "material section")?,
            "material section",
        )?;
        let expected_texture_length = texture_byte_length(self.layer_count)?;
        let expected_payload = checked_add(
            expected_textures,
            expected_texture_length,
            "texture section",
        )?;
        let expected_input = checked_add(expected_payload, TRAILING_HASH_BYTES, "blob hash")?;

        if self.visuals_offset != expected_visuals
            || self.hashes_offset != expected_hashes
            || self.materials_offset != expected_materials
            || self.textures_offset != expected_textures
        {
            return Err(invalid("blob sections are not canonical and contiguous"));
        }
        if self.textures_length != expected_texture_length {
            return Err(invalid(format!(
                "texture section is {} bytes, expected {expected_texture_length}",
                self.textures_length
            )));
        }
        if self.payload_length != expected_payload || input_length != expected_input {
            return Err(invalid(format!(
                "blob length is {input_length} bytes, expected {expected_input}"
            )));
        }
        Ok(())
    }
}

struct HeaderReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> HeaderReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn read_exact(&mut self, length: usize) -> Result<&'a [u8], AssetError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(AssetError::BlobSizeOverflow { section: "header" })?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| invalid("truncated blob header"))?;
        self.position = end;
        Ok(value)
    }

    fn read_u32(&mut self) -> Result<u32, AssetError> {
        Ok(u32::from_le_bytes(
            self.read_exact(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn read_usize(&mut self) -> Result<usize, AssetError> {
        let value = u64::from_le_bytes(self.read_exact(8)?.try_into().expect("eight-byte slice"));
        usize::try_from(value).map_err(|_| invalid("blob offset exceeds the platform address size"))
    }
}

fn validate_hash(bytes: &[u8], payload_length: usize) -> Result<(), AssetError> {
    let expected = Sha256::digest(&bytes[..payload_length]);
    if &bytes[payload_length..] != expected.as_slice() {
        return Err(invalid("compiled asset SHA-256 mismatch"));
    }
    Ok(())
}

fn validate_visuals(bytes: &[u8], material_count: usize) -> Result<(), AssetError> {
    for (index, record) in bytes.chunks_exact(VISUAL_BYTES).enumerate() {
        for face in 0..6 {
            let material = read_u32(record, face * 4) as usize;
            if material >= material_count {
                return Err(invalid(format!(
                    "visual {index} references material {material}, but there are {material_count} materials"
                )));
            }
        }
        BlockFlags::from_bits(record[24])
            .ok_or_else(|| invalid(format!("visual {index} has unsupported flags")))?;
        if record[25..] != [0; 3] {
            return Err(invalid(format!(
                "visual {index} reserved bytes are not zero"
            )));
        }
    }
    Ok(())
}

fn validate_hashes(bytes: &[u8], visual_count: usize) -> Result<(), AssetError> {
    let mut previous = None;
    for record in bytes.chunks_exact(HASH_ENTRY_BYTES) {
        let hash = read_u32(record, 0);
        let visual = read_u32(record, 4) as usize;
        if previous.is_some_and(|previous| previous >= hash) {
            return Err(invalid("hashed lookup keys are not strictly increasing"));
        }
        if visual >= visual_count {
            return Err(invalid(format!(
                "hash {hash:#010x} references visual {visual}, but there are {visual_count} visuals"
            )));
        }
        previous = Some(hash);
    }
    Ok(())
}

fn validate_materials(bytes: &[u8], layer_count: usize) -> Result<(), AssetError> {
    for (index, record) in bytes.chunks_exact(MATERIAL_BYTES).enumerate() {
        let layer = read_u32(record, 0) as usize;
        let flags = read_u32(record, 4);
        if layer >= layer_count {
            return Err(invalid(format!(
                "material {index} references layer {layer}, but there are {layer_count} layers"
            )));
        }
        if index == DIAGNOSTIC_MATERIAL as usize && (layer != 0 || flags != 0) {
            return Err(invalid("material 0 is not the diagnostic material"));
        }
    }
    Ok(())
}

fn decode_visuals(bytes: &[u8]) -> Box<[BlockVisual]> {
    bytes
        .chunks_exact(VISUAL_BYTES)
        .map(|record| {
            let mut faces = [0; 6];
            for (face, material) in faces.iter_mut().enumerate() {
                *material = read_u32(record, face * 4);
            }
            BlockVisual {
                faces,
                flags: BlockFlags::from_bits(record[24]).expect("flags validated before decode"),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn decode_hashes(bytes: &[u8]) -> Box<[(u32, u32)]> {
    bytes
        .chunks_exact(HASH_ENTRY_BYTES)
        .map(|record| (read_u32(record, 0), read_u32(record, 4)))
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn decode_materials(bytes: &[u8]) -> Box<[Material]> {
    bytes
        .chunks_exact(MATERIAL_BYTES)
        .map(|record| Material {
            layer: read_u32(record, 0),
            flags: read_u32(record, 4),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn decode_textures(bytes: &[u8], layers: usize) -> Result<TextureArray, AssetError> {
    let mut remaining = bytes;
    let mut mips = Vec::with_capacity(MIP_COUNT as usize);
    for level in 0..MIP_COUNT {
        let size = TILE_SIZE >> level;
        let mip_length = checked_mul(size as usize, size as usize, "mip pixels")
            .and_then(|pixels| checked_mul(pixels, 4, "mip RGBA"))
            .and_then(|rgba| checked_mul(rgba, layers, "mip layers"))?;
        let (rgba8, rest) = remaining.split_at(mip_length);
        mips.push(TextureMip {
            size,
            rgba8: rgba8.to_vec().into_boxed_slice(),
        });
        remaining = rest;
    }
    debug_assert!(remaining.is_empty());
    Ok(TextureArray {
        layers: u32::try_from(layers).expect("validated texture layer count fits u32"),
        mips: mips.into_boxed_slice(),
    })
}

fn texture_byte_length(layers: usize) -> Result<usize, AssetError> {
    let mut total = 0;
    for level in 0..MIP_COUNT {
        let size = (TILE_SIZE >> level) as usize;
        let mip = checked_mul(size, size, "mip pixels")
            .and_then(|pixels| checked_mul(pixels, 4, "mip RGBA"))
            .and_then(|rgba| checked_mul(rgba, layers, "mip layers"))?;
        total = checked_add(total, mip, "texture section")?;
    }
    Ok(total)
}

fn validate_count(section: &'static str, count: usize, max: usize) -> Result<(), AssetError> {
    if count > max {
        return Err(invalid(format!(
            "{section} count {count} exceeds the limit of {max}"
        )));
    }
    Ok(())
}

fn checked_mul(left: usize, right: usize, section: &'static str) -> Result<usize, AssetError> {
    left.checked_mul(right)
        .ok_or(AssetError::BlobSizeOverflow { section })
}

fn checked_add(left: usize, right: usize, section: &'static str) -> Result<usize, AssetError> {
    left.checked_add(right)
        .ok_or(AssetError::BlobSizeOverflow { section })
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated fixed-width record"),
    )
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
