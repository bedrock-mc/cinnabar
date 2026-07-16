use sha2::{Digest, Sha256};

use crate::AssetError;

pub const ATMOSPHERE_BLOB_MAGIC: [u8; 8] = *b"MCBEATM1";
pub const ATMOSPHERE_BLOB_VERSION: u32 = 1;
const HEADER_BYTES: usize = 128;
const DESCRIPTOR_BYTES: usize = 112;
const HASH_BYTES: usize = 32;
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const FORMAT_RGBA8_SRGB: u32 = 1;
const CELESTIAL_TILE_SIZE: u32 = 32;
const CELESTIAL_BORDER_TEXELS: usize = (4 * CELESTIAL_TILE_SIZE - 4) as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AtmosphereRole {
    Sun = 1,
    MoonPhases = 2,
    Clouds = 3,
}

impl AtmosphereRole {
    pub const ALL: [Self; 3] = [Self::Sun, Self::MoonPhases, Self::Clouds];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sun => "sun",
            Self::MoonPhases => "moon phases",
            Self::Clouds => "clouds",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtmosphereTexture {
    pub role: AtmosphereRole,
    pub source_path: Box<str>,
    pub source_bytes: u32,
    pub source_sha256: [u8; 32],
    pub pixels_sha256: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledAtmosphereAssets {
    pub source_manifest_sha256: [u8; 32],
    pub textures: Box<[AtmosphereTexture]>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CelestialTile {
    Sun,
    MoonPhase(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CelestialBorderTexel {
    pub tile: CelestialTile,
    pub coordinate: [u32; 2],
    pub rgba8: [u8; 4],
}

#[must_use]
pub fn composite_celestial(
    destination: [f32; 3],
    sampled_rgb: [f32; 3],
    coverage: f32,
) -> [f32; 3] {
    [
        destination[0] + sampled_rgb[0] * coverage,
        destination[1] + sampled_rgb[1] * coverage,
        destination[2] + sampled_rgb[2] * coverage,
    ]
}

pub struct RuntimeAtmosphereAssets {
    source_manifest_sha256: [u8; 32],
    textures: Box<[AtmosphereTexture]>,
}

impl RuntimeAtmosphereAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES {
            return Err(invalid("truncated MCBEATM1 blob"));
        }
        if bytes[..8] != ATMOSPHERE_BLOB_MAGIC
            || u32_at(bytes, 8)? != ATMOSPHERE_BLOB_VERSION
            || u32_at(bytes, 12)? as usize != AtmosphereRole::ALL.len()
        {
            return Err(invalid("unsupported MCBEATM1 header"));
        }
        let source_manifest_sha256 = array_at::<32>(bytes, 16)?;
        if source_manifest_sha256 == [0; 32] || bytes[80..HEADER_BYTES] != [0; 48] {
            return Err(invalid("invalid MCBEATM1 provenance or reserved header"));
        }
        let descriptors_offset = usize_at(bytes, 48)?;
        let paths_offset = usize_at(bytes, 56)?;
        let payload_offset = usize_at(bytes, 64)?;
        let payload_end = usize_at(bytes, 72)?;
        let expected_paths = checked_add(
            HEADER_BYTES,
            checked_mul(
                AtmosphereRole::ALL.len(),
                DESCRIPTOR_BYTES,
                "atmosphere descriptors",
            )?,
            "atmosphere paths",
        )?;
        if descriptors_offset != HEADER_BYTES
            || paths_offset != expected_paths
            || payload_offset < paths_offset
            || payload_end < payload_offset
            || bytes.len() != checked_add(payload_end, HASH_BYTES, "atmosphere hash")?
        {
            return Err(invalid("noncanonical MCBEATM1 section layout"));
        }
        let digest = Sha256::digest(&bytes[..payload_end]);
        if &bytes[payload_end..] != digest.as_slice() {
            return Err(invalid("MCBEATM1 envelope hash mismatch"));
        }

        let specs = source_specs();
        let mut expected_path_offset = paths_offset;
        let mut expected_payload_offset = payload_offset;
        let mut textures = Vec::with_capacity(specs.len());
        for (index, (expected_role, expected_path, expected_width, expected_height)) in
            specs.into_iter().enumerate()
        {
            let descriptor = checked_add(
                descriptors_offset,
                checked_mul(index, DESCRIPTOR_BYTES, "atmosphere descriptor")?,
                "atmosphere descriptor",
            )?;
            let role = role_from_u32(u32_at(bytes, descriptor)?)?;
            let width = u32_at(bytes, descriptor + 4)?;
            let height = u32_at(bytes, descriptor + 8)?;
            let format = u32_at(bytes, descriptor + 12)?;
            let path_offset = usize_at(bytes, descriptor + 16)?;
            let path_length = u32_at(bytes, descriptor + 24)? as usize;
            let source_bytes = u32_at(bytes, descriptor + 28)?;
            let texture_offset = usize_at(bytes, descriptor + 32)?;
            let texture_length = usize_at(bytes, descriptor + 40)?;
            let source_sha256 = array_at::<32>(bytes, descriptor + 48)?;
            let pixels_sha256 = array_at::<32>(bytes, descriptor + 80)?;
            let expected_texture_length = pixel_length(expected_width, expected_height)?;
            if role != expected_role
                || width != expected_width
                || height != expected_height
                || format != FORMAT_RGBA8_SRGB
                || source_bytes == 0
                || source_bytes as usize > MAX_SOURCE_BYTES
                || path_offset != expected_path_offset
                || path_length != expected_path.len()
                || texture_offset != expected_payload_offset
                || texture_length != expected_texture_length
                || source_sha256 == [0; 32]
            {
                return Err(invalid("noncanonical MCBEATM1 texture descriptor"));
            }
            let path_end = checked_add(path_offset, path_length, "atmosphere path")?;
            if path_end > payload_offset
                || bytes.get(path_offset..path_end) != Some(expected_path.as_bytes())
            {
                return Err(invalid("unexpected MCBEATM1 source path"));
            }
            let texture_end = checked_add(texture_offset, texture_length, "atmosphere pixels")?;
            if texture_end > payload_end {
                return Err(invalid("MCBEATM1 texture payload is out of range"));
            }
            let rgba8 = bytes[texture_offset..texture_end]
                .to_vec()
                .into_boxed_slice();
            if Sha256::digest(&rgba8).as_slice() != pixels_sha256 {
                return Err(invalid("MCBEATM1 texture pixel hash mismatch"));
            }
            textures.push(AtmosphereTexture {
                role,
                source_path: expected_path.into(),
                source_bytes,
                source_sha256,
                pixels_sha256,
                width,
                height,
                rgba8,
            });
            expected_path_offset = path_end;
            expected_payload_offset = texture_end;
        }
        if expected_path_offset != payload_offset || expected_payload_offset != payload_end {
            return Err(invalid("MCBEATM1 sections contain gaps or trailing data"));
        }
        Ok(Self {
            source_manifest_sha256,
            textures: textures.into_boxed_slice(),
        })
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub fn textures(&self) -> &[AtmosphereTexture] {
        &self.textures
    }

    #[must_use]
    pub fn texture(&self, role: AtmosphereRole) -> Option<&AtmosphereTexture> {
        self.textures.iter().find(|texture| texture.role == role)
    }

    pub fn celestial_border_texels(
        &self,
    ) -> Result<impl ExactSizeIterator<Item = CelestialBorderTexel>, AssetError> {
        let sun = self
            .texture(AtmosphereRole::Sun)
            .ok_or_else(|| invalid("decoded atmosphere assets are missing the sun texture"))?;
        validate_celestial_texture(sun, 32, 32)?;
        let moon = self.texture(AtmosphereRole::MoonPhases).ok_or_else(|| {
            invalid("decoded atmosphere assets are missing the moon phase texture")
        })?;
        validate_celestial_texture(moon, 128, 64)?;

        let mut borders = Vec::with_capacity(9 * CELESTIAL_BORDER_TEXELS);
        append_celestial_border(&mut borders, sun, CelestialTile::Sun, [0, 0]);
        for phase in 0_u8..8 {
            let origin = [
                u32::from(phase % 4) * CELESTIAL_TILE_SIZE,
                u32::from(phase / 4) * CELESTIAL_TILE_SIZE,
            ];
            append_celestial_border(&mut borders, moon, CelestialTile::MoonPhase(phase), origin);
        }
        Ok(borders.into_iter())
    }
}

fn validate_celestial_texture(
    texture: &AtmosphereTexture,
    expected_width: u32,
    expected_height: u32,
) -> Result<(), AssetError> {
    if texture.width != expected_width
        || texture.height != expected_height
        || texture.rgba8.len() != pixel_length(expected_width, expected_height)?
    {
        return Err(invalid(format!(
            "decoded {} texture is {}x{} with {} RGBA bytes; expected {expected_width}x{expected_height}",
            texture.role.label(),
            texture.width,
            texture.height,
            texture.rgba8.len()
        )));
    }
    Ok(())
}

fn append_celestial_border(
    borders: &mut Vec<CelestialBorderTexel>,
    texture: &AtmosphereTexture,
    tile: CelestialTile,
    origin: [u32; 2],
) {
    for y in 0..CELESTIAL_TILE_SIZE {
        for x in 0..CELESTIAL_TILE_SIZE {
            if x != 0 && x != CELESTIAL_TILE_SIZE - 1 && y != 0 && y != CELESTIAL_TILE_SIZE - 1 {
                continue;
            }
            let atlas_x = origin[0] + x;
            let atlas_y = origin[1] + y;
            let offset = ((atlas_y * texture.width + atlas_x) * 4) as usize;
            borders.push(CelestialBorderTexel {
                tile,
                coordinate: [x, y],
                rgba8: texture.rgba8[offset..offset + 4]
                    .try_into()
                    .expect("validated celestial texture contains every border texel"),
            });
        }
    }
}

pub fn encode_atmosphere_blob(
    compiled: &CompiledAtmosphereAssets,
) -> Result<Box<[u8]>, AssetError> {
    validate_compiled(compiled)?;
    let descriptors_offset = HEADER_BYTES;
    let paths_offset = checked_add(
        descriptors_offset,
        checked_mul(
            compiled.textures.len(),
            DESCRIPTOR_BYTES,
            "atmosphere descriptors",
        )?,
        "atmosphere paths",
    )?;
    let paths_length = compiled.textures.iter().try_fold(0usize, |sum, texture| {
        checked_add(sum, texture.source_path.len(), "atmosphere paths")
    })?;
    let payload_offset = checked_add(paths_offset, paths_length, "atmosphere payload")?;
    let payload_length = compiled.textures.iter().try_fold(0usize, |sum, texture| {
        checked_add(sum, texture.rgba8.len(), "atmosphere payload")
    })?;
    let payload_end = checked_add(payload_offset, payload_length, "atmosphere payload")?;
    let total_length = checked_add(payload_end, HASH_BYTES, "atmosphere hash")?;
    let mut bytes = Vec::with_capacity(total_length);
    bytes.extend_from_slice(&ATMOSPHERE_BLOB_MAGIC);
    push_u32(&mut bytes, ATMOSPHERE_BLOB_VERSION);
    push_u32(
        &mut bytes,
        u32::try_from(compiled.textures.len())
            .map_err(|_| invalid("atmosphere texture count overflow"))?,
    );
    bytes.extend_from_slice(&compiled.source_manifest_sha256);
    for offset in [
        descriptors_offset,
        paths_offset,
        payload_offset,
        payload_end,
    ] {
        push_u64(&mut bytes, offset)?;
    }
    bytes.resize(HEADER_BYTES, 0);

    let mut path_offset = paths_offset;
    let mut texture_offset = payload_offset;
    for texture in &compiled.textures {
        push_u32(&mut bytes, texture.role as u32);
        push_u32(&mut bytes, texture.width);
        push_u32(&mut bytes, texture.height);
        push_u32(&mut bytes, FORMAT_RGBA8_SRGB);
        push_u64(&mut bytes, path_offset)?;
        push_u32(
            &mut bytes,
            u32::try_from(texture.source_path.len())
                .map_err(|_| invalid("atmosphere path length overflow"))?,
        );
        push_u32(&mut bytes, texture.source_bytes);
        push_u64(&mut bytes, texture_offset)?;
        push_u64(&mut bytes, texture.rgba8.len())?;
        bytes.extend_from_slice(&texture.source_sha256);
        bytes.extend_from_slice(&texture.pixels_sha256);
        path_offset = checked_add(path_offset, texture.source_path.len(), "atmosphere path")?;
        texture_offset = checked_add(texture_offset, texture.rgba8.len(), "atmosphere pixels")?;
    }
    for texture in &compiled.textures {
        bytes.extend_from_slice(texture.source_path.as_bytes());
    }
    for texture in &compiled.textures {
        bytes.extend_from_slice(&texture.rgba8);
    }
    debug_assert_eq!(bytes.len(), payload_end);
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    Ok(bytes.into_boxed_slice())
}

const fn source_specs() -> [(AtmosphereRole, &'static str, u32, u32); 3] {
    [
        (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            256,
            256,
        ),
    ]
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

fn validate_compiled(compiled: &CompiledAtmosphereAssets) -> Result<(), AssetError> {
    if compiled.source_manifest_sha256 == [0; 32]
        || compiled.textures.len() != AtmosphereRole::ALL.len()
    {
        return Err(invalid("invalid atmosphere provenance or texture count"));
    }
    for (texture, (role, path, width, height)) in compiled.textures.iter().zip(source_specs()) {
        if texture.role != role
            || texture.source_path.as_ref() != path
            || texture.width != width
            || texture.height != height
            || texture.source_bytes == 0
            || texture.source_bytes as usize > MAX_SOURCE_BYTES
            || texture.source_sha256 == [0; 32]
            || texture.rgba8.len() != pixel_length(width, height)?
            || Sha256::digest(&texture.rgba8).as_slice() != texture.pixels_sha256
        {
            return Err(invalid("noncanonical compiled atmosphere texture"));
        }
    }
    Ok(())
}

fn role_from_u32(value: u32) -> Result<AtmosphereRole, AssetError> {
    match value {
        1 => Ok(AtmosphereRole::Sun),
        2 => Ok(AtmosphereRole::MoonPhases),
        3 => Ok(AtmosphereRole::Clouds),
        _ => Err(invalid("unsupported atmosphere role")),
    }
}

fn pixel_length(width: u32, height: u32) -> Result<usize, AssetError> {
    checked_mul(
        checked_mul(width as usize, height as usize, "atmosphere pixels")?,
        4,
        "atmosphere pixels",
    )
}

fn checked_add(left: usize, right: usize, section: &'static str) -> Result<usize, AssetError> {
    left.checked_add(right)
        .ok_or(AssetError::BlobSizeOverflow { section })
}

fn checked_mul(left: usize, right: usize, section: &'static str) -> Result<usize, AssetError> {
    left.checked_mul(right)
        .ok_or(AssetError::BlobSizeOverflow { section })
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: usize) -> Result<(), AssetError> {
    bytes.extend_from_slice(
        &u64::try_from(value)
            .map_err(|_| AssetError::BlobSizeOverflow {
                section: "atmosphere offset",
            })?
            .to_le_bytes(),
    );
    Ok(())
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, AssetError> {
    Ok(u32::from_le_bytes(array_at(bytes, offset)?))
}

fn usize_at(bytes: &[u8], offset: usize) -> Result<usize, AssetError> {
    usize::try_from(u64::from_le_bytes(array_at(bytes, offset)?))
        .map_err(|_| invalid("MCBEATM1 offset exceeds platform"))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    let end = checked_add(offset, N, "atmosphere field")?;
    bytes
        .get(offset..end)
        .ok_or_else(|| invalid("truncated MCBEATM1 field"))?
        .try_into()
        .map_err(|_| invalid("invalid MCBEATM1 field"))
}
