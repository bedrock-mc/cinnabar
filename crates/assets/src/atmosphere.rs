use std::{
    borrow::Cow,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::AssetError;

pub const ATMOSPHERE_BLOB_MAGIC: [u8; 8] = *b"MCBEATM1";
pub const ATMOSPHERE_BLOB_VERSION: u32 = 1;
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_DECODE_ALLOC: u64 = 512 * 1024;
const HEADER_BYTES: usize = 128;
const DESCRIPTOR_BYTES: usize = 112;
const HASH_BYTES: usize = 32;
const FORMAT_RGBA8_SRGB: u32 = 1;
const CELESTIAL_TILE_SIZE: u32 = 32;
const CELESTIAL_BORDER_TEXELS: usize = (4 * CELESTIAL_TILE_SIZE - 4) as usize;
const PINNED_MANIFEST_SHA256: [u8; 32] =
    decode_sha256(b"c6d5f56b942d703a7acd1f83b2cddb7633069e13412ad5a1c3beae666e2ec6f6");
const PINNED_TAG: &str = "v1.26.30.32-preview";
const PINNED_COMMIT: &str = "020f1cf4b2baef78e635d4ce7498eb16a429dcbb";
const PINNED_ARCHIVE: &str = "bedrock-samples-v1.26.30.32-preview-full.zip";
const PINNED_URL: &str = "https://github.com/Mojang/bedrock-samples/releases/download/v1.26.30.32-preview/bedrock-samples-v1.26.30.32-preview-full.zip";
const PINNED_ARCHIVE_SHA256: &str =
    "12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c";
const PINNED_CACHE_DIR: &str = ".local/assets/bedrock-samples/v1.26.30.32-preview/full";
const SUN_SOURCE_SHA256: [u8; 32] =
    decode_sha256(b"f7273544b691f08aaef76373d526e00793cf1e1aa0e1df8518f738d44a8e526b");
const MOON_PHASES_SOURCE_SHA256: [u8; 32] =
    decode_sha256(b"01c566d48e0cc8618cf6fdce811b61175fc246f12f2e8f2c567d6acd3a2b35d8");
const CLOUDS_SOURCE_SHA256: [u8; 32] =
    decode_sha256(b"4f57cfe866779ef82be0058e244a77b0a279ee75e9eb40ac9ce6eb372445adc8");
const NATIVE_CLOUDS_SOURCE_SHA256: [u8; 32] =
    decode_sha256(b"f19b2f3a483af3a67568dfed4387c7b59fed215edf1cb02bef0470f2b72982a0");
const NATIVE_CLOUDS_PIXELS_SHA256: [u8; 32] =
    decode_sha256(b"95f8808115fcc28c8665324bba1b72dcb1350fbfebd1c9a30009691326695136");
const NATIVE_CLOUDS_SOURCE_BYTES: usize = 7_880;
const NATIVE_CLOUDS_OCCUPIED_TEXELS: usize = 13_356;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AtmosphereRole {
    Sun = 1,
    MoonPhases = 2,
    Clouds = 3,
}

impl AtmosphereRole {
    pub const ALL: [Self; 3] = [Self::Sun, Self::MoonPhases, Self::Clouds];

    const fn label(self) -> &'static str {
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

#[derive(Clone, Copy, Debug, Default)]
pub struct AtmosphereCompileOptions<'a> {
    pub clouds_override: Option<&'a Path>,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    schema: u32,
    tag: Box<str>,
    commit: Box<str>,
    archive: Box<str>,
    url: Box<str>,
    sha256: Box<str>,
    artifact_policy: Box<str>,
    cache_dir: Box<str>,
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

/// Compiles the fixed vanilla atmosphere sources from a bounded local pack.
pub fn compile_atmosphere_assets(
    root: &Path,
    source_manifest: &[u8],
) -> Result<CompiledAtmosphereAssets, AssetError> {
    compile_atmosphere_assets_with_options(
        root,
        source_manifest,
        AtmosphereCompileOptions::default(),
    )
}

/// Compiles the pinned atmosphere sources, optionally replacing only Clouds
/// with the exact matching locally installed 1.26.33.1 texture.
pub fn compile_atmosphere_assets_with_options(
    root: &Path,
    source_manifest: &[u8],
    options: AtmosphereCompileOptions<'_>,
) -> Result<CompiledAtmosphereAssets, AssetError> {
    if source_manifest.len() > MAX_SOURCE_MANIFEST_BYTES {
        return Err(AssetError::AtmosphereManifestTooLarge {
            size: source_manifest.len(),
            max: MAX_SOURCE_MANIFEST_BYTES,
        });
    }
    let canonical_manifest = canonical_manifest_line_endings(source_manifest)?;
    let manifest = serde_json::from_slice::<SourceManifest>(&canonical_manifest)
        .map_err(|source| AssetError::InvalidAtmosphereManifest { source })?;
    let source_manifest_sha256: [u8; 32] = Sha256::digest(&canonical_manifest).into();
    validate_source_manifest(&manifest, source_manifest_sha256)?;
    let specs = source_specs();
    let textures = specs
        .into_iter()
        .map(|(role, source_path, width, height)| {
            if role == AtmosphereRole::Clouds
                && let Some(path) = options.clouds_override
            {
                read_cloud_override(path)
            } else {
                read_texture(root, role, source_path, width, height)
            }
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    Ok(CompiledAtmosphereAssets {
        source_manifest_sha256,
        textures,
    })
}

fn canonical_manifest_line_endings(source: &[u8]) -> Result<Cow<'_, [u8]>, AssetError> {
    if !source.contains(&b'\r') {
        return Ok(Cow::Borrowed(source));
    }

    let mut canonical = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\r' if source.get(index + 1) == Some(&b'\n') => {
                canonical.push(b'\n');
                index += 2;
            }
            b'\r' | b'\n' => {
                return Err(AssetError::InvalidAtmosphereProvenance {
                    detail: "manifest must use uniformly LF or CRLF line endings".into(),
                });
            }
            byte => {
                canonical.push(byte);
                index += 1;
            }
        }
    }
    Ok(Cow::Owned(canonical))
}

fn validate_source_manifest(
    manifest: &SourceManifest,
    manifest_sha256: [u8; 32],
) -> Result<(), AssetError> {
    let hex = |value: &str, length: usize| {
        value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    let cache_path = Path::new(manifest.cache_dir.as_ref());
    if manifest_sha256 != PINNED_MANIFEST_SHA256
        || manifest.schema != 1
        || !safe_component(&manifest.tag)
        || !safe_component(&manifest.archive)
        || manifest.tag.as_ref() != PINNED_TAG
        || manifest.commit.as_ref() != PINNED_COMMIT
        || !hex(&manifest.commit, 40)
        || manifest.archive.as_ref() != PINNED_ARCHIVE
        || manifest.url.as_ref() != PINNED_URL
        || manifest.sha256.as_ref() != PINNED_ARCHIVE_SHA256
        || !hex(&manifest.sha256, 64)
        || manifest.artifact_policy.as_ref() != "local-only"
        || cache_path.is_absolute()
        || manifest
            .cache_dir
            .split(['/', '\\'])
            .any(|part| part == "..")
        || manifest.cache_dir.as_ref() != PINNED_CACHE_DIR
    {
        return Err(AssetError::InvalidAtmosphereProvenance {
            detail: "manifest bytes and fields must exactly match the reviewed Mojang Bedrock Samples pin".into(),
        });
    }
    Ok(())
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['/', '\\'])
        && value != "."
        && value != ".."
        && Path::new(value)
            .file_name()
            .is_some_and(|name| name == value)
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

fn read_cloud_override(path: &Path) -> Result<AtmosphereTexture, AssetError> {
    let role = AtmosphereRole::Clouds;
    let file = File::open(path).map_err(|source| AssetError::AtmosphereTextureIo {
        role: role.label(),
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::AtmosphereTextureIo {
            role: role.label(),
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(AssetError::AtmosphereTextureTooLarge {
            role: role.label(),
            path: path.to_path_buf(),
            size: bytes.len(),
            max: MAX_SOURCE_BYTES,
        });
    }
    let dimensions = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png)
        .into_dimensions()
        .map_err(|source| AssetError::AtmosphereTextureDecode {
            role: role.label(),
            path: path.to_path_buf(),
            source,
        })?;
    if dimensions != (256, 256) {
        return Err(AssetError::WrongAtmosphereTextureDimensions {
            role: role.label(),
            path: path.to_path_buf(),
            width: dimensions.0,
            height: dimensions.1,
            expected_width: 256,
            expected_height: 256,
        });
    }
    let source_sha256: [u8; 32] = Sha256::digest(&bytes).into();
    if bytes.len() != NATIVE_CLOUDS_SOURCE_BYTES || source_sha256 != NATIVE_CLOUDS_SOURCE_SHA256 {
        return Err(AssetError::AtmosphereTextureHashMismatch {
            role: role.label(),
            path: path.to_path_buf(),
        });
    }

    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(256);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .map_err(|source| AssetError::AtmosphereTextureDecode {
            role: role.label(),
            path: path.to_path_buf(),
            source,
        })?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    let pixels_sha256: [u8; 32] = Sha256::digest(&rgba8).into();
    let occupied_texels = rgba8
        .chunks_exact(4)
        .filter(|pixel| pixel[3] >= 128)
        .count();
    if rgba8.len() != pixel_length(256, 256)?
        || pixels_sha256 != NATIVE_CLOUDS_PIXELS_SHA256
        || occupied_texels != NATIVE_CLOUDS_OCCUPIED_TEXELS
    {
        return Err(AssetError::AtmosphereTextureHashMismatch {
            role: role.label(),
            path: path.to_path_buf(),
        });
    }
    Ok(AtmosphereTexture {
        role,
        source_path: "textures/environment/clouds.png".into(),
        source_bytes: NATIVE_CLOUDS_SOURCE_BYTES as u32,
        source_sha256,
        pixels_sha256,
        width: 256,
        height: 256,
        rgba8,
    })
}

fn read_texture(
    root: &Path,
    role: AtmosphereRole,
    source_path: &'static str,
    expected_width: u32,
    expected_height: u32,
) -> Result<AtmosphereTexture, AssetError> {
    let path = root.join(source_path);
    let file = File::open(&path).map_err(|source| AssetError::AtmosphereTextureIo {
        role: role.label(),
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::AtmosphereTextureIo {
            role: role.label(),
            path: path.clone(),
            source,
        })?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(AssetError::AtmosphereTextureTooLarge {
            role: role.label(),
            path,
            size: bytes.len(),
            max: MAX_SOURCE_BYTES,
        });
    }
    let source_sha256: [u8; 32] = Sha256::digest(&bytes).into();
    if source_sha256 != expected_source_sha256(role) {
        return Err(AssetError::AtmosphereTextureHashMismatch {
            role: role.label(),
            path,
        });
    }
    let dimensions = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png)
        .into_dimensions()
        .map_err(|source| AssetError::AtmosphereTextureDecode {
            role: role.label(),
            path: path.clone(),
            source,
        })?;
    if dimensions != (expected_width, expected_height) {
        return Err(AssetError::WrongAtmosphereTextureDimensions {
            role: role.label(),
            path,
            width: dimensions.0,
            height: dimensions.1,
            expected_width,
            expected_height,
        });
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(expected_width);
    limits.max_image_height = Some(expected_height);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .map_err(|source| AssetError::AtmosphereTextureDecode {
            role: role.label(),
            path: path.clone(),
            source,
        })?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    let expected_len = expected_width as usize * expected_height as usize * 4;
    if rgba8.len() != expected_len {
        return Err(invalid("atmosphere texture RGBA8 length is invalid"));
    }
    Ok(AtmosphereTexture {
        role,
        source_path: source_path.into(),
        source_bytes: u32::try_from(bytes.len()).map_err(|_| AssetError::BlobSizeOverflow {
            section: "atmosphere source size",
        })?,
        source_sha256,
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: expected_width,
        height: expected_height,
        rgba8,
    })
}

const fn expected_source_sha256(role: AtmosphereRole) -> [u8; 32] {
    match role {
        AtmosphereRole::Sun => SUN_SOURCE_SHA256,
        AtmosphereRole::MoonPhases => MOON_PHASES_SOURCE_SHA256,
        AtmosphereRole::Clouds => CLOUDS_SOURCE_SHA256,
    }
}

const fn decode_sha256(value: &[u8; 64]) -> [u8; 32] {
    let mut decoded = [0_u8; 32];
    let mut index = 0;
    while index < decoded.len() {
        decoded[index] =
            (decode_hex_nibble(value[index * 2]) << 4) | decode_hex_nibble(value[index * 2 + 1]);
        index += 1;
    }
    decoded
}

const fn decode_hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("invalid pinned SHA-256"),
    }
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

#[cfg(test)]
mod cloud_override_tests {
    use std::{fs, path::Path};

    use image::{Rgba, RgbaImage};

    use super::{AssetError, MAX_SOURCE_BYTES, read_cloud_override};

    #[test]
    fn cloud_override_missing_path_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            read_cloud_override(&directory.path().join("missing.png")),
            Err(AssetError::AtmosphereTextureIo { role: "clouds", .. })
        ));
    }

    #[test]
    fn cloud_override_oversized_input_fails_before_decode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.png");
        fs::write(&path, vec![0; MAX_SOURCE_BYTES + 1]).unwrap();
        assert!(matches!(
            read_cloud_override(&path),
            Err(AssetError::AtmosphereTextureTooLarge { role: "clouds", .. })
        ));
    }

    #[test]
    fn cloud_override_wrong_dimensions_are_reported_before_hash_validation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wrong-dimensions.png");
        write_png(&path, 255, 256);
        assert!(matches!(
            read_cloud_override(&path),
            Err(AssetError::WrongAtmosphereTextureDimensions {
                role: "clouds",
                width: 255,
                height: 256,
                expected_width: 256,
                expected_height: 256,
                ..
            })
        ));
    }

    #[test]
    fn cloud_override_wrong_hash_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wrong-hash.png");
        write_png(&path, 256, 256);
        assert!(matches!(
            read_cloud_override(&path),
            Err(AssetError::AtmosphereTextureHashMismatch { role: "clouds", .. })
        ));
    }

    fn write_png(path: &Path, width: u32, height: u32) {
        RgbaImage::from_pixel(width, height, Rgba([0x44, 0x55, 0x66, 0xff]))
            .save(path)
            .unwrap();
    }
}
