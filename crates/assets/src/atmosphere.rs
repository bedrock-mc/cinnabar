use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AssetError;

pub const ATMOSPHERE_BLOB_MAGIC: [u8; 8] = *b"MCBEATM2";
pub const ATMOSPHERE_BLOB_VERSION: u32 = 2;
const HEADER_BYTES: usize = 128;
const DESCRIPTOR_BYTES: usize = 112;
const HASH_BYTES: usize = 32;
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_ENVIRONMENT_SECTION_BYTES: usize = 1024 * 1024;
const FORMAT_RGBA8_SRGB: u32 = 1;
const CELESTIAL_TILE_SIZE: u32 = 32;
const CELESTIAL_BORDER_TEXELS: usize = (4 * CELESTIAL_TILE_SIZE - 4) as usize;
pub const MAX_ENVIRONMENT_PROFILES: usize = 1_024;
pub const MAX_ENVIRONMENT_IDENTIFIER_BYTES: usize = 256;
pub const MAX_FOG_DISTANCES: usize = 6;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[repr(u8)]
pub enum FogMedium {
    Air = 0,
    Water = 1,
    Weather = 2,
    Lava = 3,
    LavaResistance = 4,
    PowderSnow = 5,
}

impl FogMedium {
    pub const ALL: [Self; MAX_FOG_DISTANCES] = [
        Self::Air,
        Self::Water,
        Self::Weather,
        Self::Lava,
        Self::LavaResistance,
        Self::PowderSnow,
    ];

    #[must_use]
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "air" => Some(Self::Air),
            "water" => Some(Self::Water),
            "weather" => Some(Self::Weather),
            "lava" => Some(Self::Lava),
            "lava_resistance" => Some(Self::LavaResistance),
            "powder_snow" => Some(Self::PowderSnow),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum FogDistanceMode {
    Fixed = 0,
    RenderRelative = 1,
}

impl FogDistanceMode {
    #[must_use]
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "fixed" => Some(Self::Fixed),
            "render" => Some(Self::RenderRelative),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FogDistance {
    pub medium: FogMedium,
    pub mode: FogDistanceMode,
    pub start_bits: u32,
    pub end_bits: u32,
    pub rgb8: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedFog {
    pub start: f32,
    pub end: f32,
    pub rgb8: u32,
}

impl FogDistance {
    #[must_use]
    pub fn start(self) -> f32 {
        f32::from_bits(self.start_bits)
    }

    #[must_use]
    pub fn end(self) -> f32 {
        f32::from_bits(self.end_bits)
    }

    #[must_use]
    pub fn resolve(self, render_distance_blocks: f32) -> Option<ResolvedFog> {
        if !render_distance_blocks.is_finite() || render_distance_blocks < 0.0 {
            return None;
        }
        let scale = match self.mode {
            FogDistanceMode::Fixed => 1.0,
            FogDistanceMode::RenderRelative => render_distance_blocks,
        };
        Some(ResolvedFog {
            start: self.start() * scale,
            end: self.end() * scale,
            rgb8: self.rgb8,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FogProfile {
    pub identifier: Box<str>,
    pub distances: Box<[FogDistance]>,
}

impl FogProfile {
    #[must_use]
    pub fn distance(&self, medium: FogMedium) -> Option<FogDistance> {
        self.distances
            .iter()
            .find(|distance| distance.medium == medium)
            .copied()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeVisualProfile {
    pub biome_identifier: Box<str>,
    pub fog_identifier: Box<str>,
    pub atmosphere_identifier: Box<str>,
    pub lighting_identifier: Box<str>,
    pub sky_rgb8: Option<u32>,
}

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
    pub biome_profiles: Box<[BiomeVisualProfile]>,
    pub fog_profiles: Box<[FogProfile]>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentSection {
    biome_profiles: Box<[BiomeVisualProfile]>,
    fog_profiles: Box<[FogProfile]>,
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
    biome_profiles: Box<[BiomeVisualProfile]>,
    fog_profiles: Box<[FogProfile]>,
}

impl RuntimeAtmosphereAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES {
            return Err(invalid("truncated MCBEATM2 blob"));
        }
        if bytes[..8] != ATMOSPHERE_BLOB_MAGIC
            || u32_at(bytes, 8)? != ATMOSPHERE_BLOB_VERSION
            || u32_at(bytes, 12)? as usize != AtmosphereRole::ALL.len()
        {
            return Err(invalid("unsupported MCBEATM2 header"));
        }
        let source_manifest_sha256 = array_at::<32>(bytes, 16)?;
        if source_manifest_sha256 == [0; 32] || bytes[104..HEADER_BYTES] != [0; 24] {
            return Err(invalid("invalid MCBEATM2 provenance or reserved header"));
        }
        let descriptors_offset = usize_at(bytes, 48)?;
        let paths_offset = usize_at(bytes, 56)?;
        let payload_offset = usize_at(bytes, 64)?;
        let texture_payload_end = usize_at(bytes, 72)?;
        let environment_offset = usize_at(bytes, 80)?;
        let environment_end = usize_at(bytes, 88)?;
        let biome_profile_count = u32_at(bytes, 96)? as usize;
        let fog_profile_count = u32_at(bytes, 100)? as usize;
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
            || texture_payload_end < payload_offset
            || environment_offset != texture_payload_end
            || environment_end < environment_offset
            || environment_end - environment_offset > MAX_ENVIRONMENT_SECTION_BYTES
            || biome_profile_count > MAX_ENVIRONMENT_PROFILES
            || fog_profile_count > MAX_ENVIRONMENT_PROFILES
            || bytes.len() != checked_add(environment_end, HASH_BYTES, "atmosphere hash")?
        {
            return Err(invalid("noncanonical MCBEATM2 section layout"));
        }
        let digest = Sha256::digest(&bytes[..environment_end]);
        if &bytes[environment_end..] != digest.as_slice() {
            return Err(invalid("MCBEATM2 envelope hash mismatch"));
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
                return Err(invalid("noncanonical MCBEATM2 texture descriptor"));
            }
            let path_end = checked_add(path_offset, path_length, "atmosphere path")?;
            if path_end > payload_offset
                || bytes.get(path_offset..path_end) != Some(expected_path.as_bytes())
            {
                return Err(invalid("unexpected MCBEATM2 source path"));
            }
            let texture_end = checked_add(texture_offset, texture_length, "atmosphere pixels")?;
            if texture_end > texture_payload_end {
                return Err(invalid("MCBEATM2 texture payload is out of range"));
            }
            let rgba8 = bytes[texture_offset..texture_end]
                .to_vec()
                .into_boxed_slice();
            if Sha256::digest(&rgba8).as_slice() != pixels_sha256 {
                return Err(invalid("MCBEATM2 texture pixel hash mismatch"));
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
        if expected_path_offset != payload_offset || expected_payload_offset != texture_payload_end
        {
            return Err(invalid(
                "MCBEATM2 texture sections contain gaps or trailing data",
            ));
        }
        let environment: EnvironmentSection =
            serde_json::from_slice(&bytes[environment_offset..environment_end])
                .map_err(|_| invalid("invalid MCBEATM2 environment section"))?;
        if environment.biome_profiles.len() != biome_profile_count
            || environment.fog_profiles.len() != fog_profile_count
        {
            return Err(invalid("MCBEATM2 environment counts do not match header"));
        }
        validate_environment_profiles(&environment.biome_profiles, &environment.fog_profiles)?;
        Ok(Self {
            source_manifest_sha256,
            textures: textures.into_boxed_slice(),
            biome_profiles: environment.biome_profiles,
            fog_profiles: environment.fog_profiles,
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

    #[must_use]
    pub fn biome_profiles(&self) -> &[BiomeVisualProfile] {
        &self.biome_profiles
    }

    #[must_use]
    pub fn biome_profile(&self, identifier: &str) -> Option<&BiomeVisualProfile> {
        self.biome_profiles
            .binary_search_by(|profile| profile.biome_identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| &self.biome_profiles[index])
    }

    #[must_use]
    pub fn fog_profiles(&self) -> &[FogProfile] {
        &self.fog_profiles
    }

    #[must_use]
    pub fn fog_profile(&self, identifier: &str) -> Option<&FogProfile> {
        self.fog_profiles
            .binary_search_by(|profile| profile.identifier.as_ref().cmp(identifier))
            .ok()
            .map(|index| &self.fog_profiles[index])
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
    let texture_payload_end = checked_add(payload_offset, payload_length, "atmosphere payload")?;
    let environment = serde_json::to_vec(&EnvironmentSection {
        biome_profiles: compiled.biome_profiles.clone(),
        fog_profiles: compiled.fog_profiles.clone(),
    })
    .map_err(|_| invalid("failed to encode MCBEATM2 environment section"))?;
    if environment.len() > MAX_ENVIRONMENT_SECTION_BYTES {
        return Err(invalid("MCBEATM2 environment section exceeds bound"));
    }
    let environment_offset = texture_payload_end;
    let environment_end = checked_add(
        environment_offset,
        environment.len(),
        "atmosphere environment",
    )?;
    let total_length = checked_add(environment_end, HASH_BYTES, "atmosphere hash")?;
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
        texture_payload_end,
        environment_offset,
        environment_end,
    ] {
        push_u64(&mut bytes, offset)?;
    }
    push_u32(
        &mut bytes,
        u32::try_from(compiled.biome_profiles.len())
            .map_err(|_| invalid("biome profile count overflow"))?,
    );
    push_u32(
        &mut bytes,
        u32::try_from(compiled.fog_profiles.len())
            .map_err(|_| invalid("fog profile count overflow"))?,
    );
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
    debug_assert_eq!(bytes.len(), environment_offset);
    bytes.extend_from_slice(&environment);
    debug_assert_eq!(bytes.len(), environment_end);
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
    validate_environment_profiles(&compiled.biome_profiles, &compiled.fog_profiles)?;
    Ok(())
}

fn validate_environment_profiles(
    biomes: &[BiomeVisualProfile],
    fogs: &[FogProfile],
) -> Result<(), AssetError> {
    if biomes.len() > MAX_ENVIRONMENT_PROFILES || fogs.len() > MAX_ENVIRONMENT_PROFILES {
        return Err(invalid("environment profile count exceeds bound"));
    }
    let mut previous_fog: Option<&str> = None;
    for profile in fogs {
        validate_environment_identifier(&profile.identifier)?;
        if previous_fog.is_some_and(|previous| previous >= profile.identifier.as_ref()) {
            return Err(invalid("fog profiles are not strictly ordered"));
        }
        if profile.distances.is_empty() || profile.distances.len() > MAX_FOG_DISTANCES {
            return Err(invalid("fog profile distance count is invalid"));
        }
        let mut previous_medium = None;
        for distance in &profile.distances {
            if previous_medium.is_some_and(|previous| previous >= distance.medium) {
                return Err(invalid("fog distances are not strictly ordered"));
            }
            let start = distance.start();
            let end = distance.end();
            if !start.is_finite()
                || !end.is_finite()
                || start < 0.0
                || end < start
                || distance.rgb8 > 0x00ff_ffff
            {
                return Err(invalid("fog distance is invalid"));
            }
            previous_medium = Some(distance.medium);
        }
        previous_fog = Some(&profile.identifier);
    }
    let mut previous_biome: Option<&str> = None;
    for profile in biomes {
        for identifier in [
            profile.biome_identifier.as_ref(),
            profile.fog_identifier.as_ref(),
            profile.atmosphere_identifier.as_ref(),
            profile.lighting_identifier.as_ref(),
        ] {
            validate_environment_identifier(identifier)?;
        }
        if previous_biome.is_some_and(|previous| previous >= profile.biome_identifier.as_ref()) {
            return Err(invalid("biome profiles are not strictly ordered"));
        }
        if profile.sky_rgb8.is_some_and(|rgb| rgb > 0x00ff_ffff)
            || fogs
                .binary_search_by(|fog| fog.identifier.cmp(&profile.fog_identifier))
                .is_err()
        {
            return Err(invalid("biome visual profile is invalid"));
        }
        previous_biome = Some(&profile.biome_identifier);
    }
    Ok(())
}

fn validate_environment_identifier(identifier: &str) -> Result<(), AssetError> {
    if identifier.is_empty() || identifier.len() > MAX_ENVIRONMENT_IDENTIFIER_BYTES {
        return Err(invalid("environment identifier length is invalid"));
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
        .map_err(|_| invalid("MCBEATM2 offset exceeds platform"))
}

fn array_at<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], AssetError> {
    let end = checked_add(offset, N, "atmosphere field")?;
    bytes
        .get(offset..end)
        .ok_or_else(|| invalid("truncated MCBEATM2 field"))?
        .try_into()
        .map_err(|_| invalid("invalid MCBEATM2 field"))
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{
        AtmosphereTexture, BiomeVisualProfile, CompiledAtmosphereAssets, FogDistance,
        FogDistanceMode, FogMedium, FogProfile, RuntimeAtmosphereAssets, encode_atmosphere_blob,
        source_specs,
    };

    #[test]
    fn environment_profiles_round_trip_through_the_hashed_envelope() {
        let compiled = CompiledAtmosphereAssets {
            source_manifest_sha256: [0x11; 32],
            textures: synthetic_textures(),
            biome_profiles: vec![BiomeVisualProfile {
                biome_identifier: "minecraft:the_end".into(),
                fog_identifier: "minecraft:fog_the_end".into(),
                atmosphere_identifier: "minecraft:end_atmospherics".into(),
                lighting_identifier: "minecraft:end_lighting".into(),
                sky_rgb8: Some(0),
            }]
            .into_boxed_slice(),
            fog_profiles: vec![FogProfile {
                identifier: "minecraft:fog_the_end".into(),
                distances: vec![FogDistance {
                    medium: FogMedium::Air,
                    mode: FogDistanceMode::RenderRelative,
                    start_bits: 0.92_f32.to_bits(),
                    end_bits: 1.0_f32.to_bits(),
                    rgb8: 0x0B_08_0C,
                }]
                .into_boxed_slice(),
            }]
            .into_boxed_slice(),
        };

        let blob = encode_atmosphere_blob(&compiled).unwrap();
        let runtime = RuntimeAtmosphereAssets::decode(&blob).unwrap();
        assert_eq!(
            runtime.biome_profile("minecraft:the_end"),
            compiled.biome_profiles.first()
        );
        assert_eq!(
            runtime.fog_profile("minecraft:fog_the_end"),
            compiled.fog_profiles.first()
        );
        assert!(runtime.biome_profile("minecraft:missing").is_none());
        assert!(runtime.fog_profile("minecraft:missing").is_none());
    }

    #[test]
    fn fog_distance_resolution_honors_fixed_and_render_relative_modes_exactly() {
        let fixed = FogDistance {
            medium: FogMedium::Air,
            mode: FogDistanceMode::Fixed,
            start_bits: 10.0_f32.to_bits(),
            end_bits: 96.0_f32.to_bits(),
            rgb8: 0x33_08_08,
        };
        assert_eq!(fixed.resolve(256.0).unwrap().start, 10.0);
        assert_eq!(fixed.resolve(256.0).unwrap().end, 96.0);
        assert_eq!(fixed.resolve(256.0).unwrap().rgb8, 0x33_08_08);

        let relative = FogDistance {
            medium: FogMedium::Air,
            mode: FogDistanceMode::RenderRelative,
            start_bits: 0.92_f32.to_bits(),
            end_bits: 1.0_f32.to_bits(),
            rgb8: 0xAB_D2_FF,
        };
        let resolved = relative.resolve(256.0).unwrap();
        assert_eq!(resolved.start, 235.52);
        assert_eq!(resolved.end, 256.0);
        assert_eq!(resolved.rgb8, 0xAB_D2_FF);
        assert!(relative.resolve(f32::NAN).is_none());
        assert!(relative.resolve(-1.0).is_none());
    }

    fn synthetic_textures() -> Box<[AtmosphereTexture]> {
        source_specs()
            .into_iter()
            .map(|(role, path, width, height)| {
                let rgba8 = vec![role as u8; (width * height * 4) as usize].into_boxed_slice();
                AtmosphereTexture {
                    role,
                    source_path: path.into(),
                    source_bytes: 1,
                    source_sha256: [role as u8; 32],
                    pixels_sha256: Sha256::digest(&rgba8).into(),
                    width,
                    height,
                    rgba8,
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }
}
