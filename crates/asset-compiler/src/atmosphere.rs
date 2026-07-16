use std::{
    borrow::Cow,
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use assets::{
    AssetError, AtmosphereRole, AtmosphereTexture, BiomeVisualProfile, CompiledAtmosphereAssets,
    FogDistance, FogDistanceMode, FogMedium, FogProfile, MAX_ENVIRONMENT_PROFILES,
    MAX_FOG_DISTANCES,
};

mod environment_source;

use environment_source::{
    ClientBiomeDocument, FogSettingsDocument, parse_environment_rgb, read_environment_json,
    sorted_environment_files, validate_environment_identifier,
};

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_DECODE_ALLOC: u64 = 512 * 1024;
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

#[derive(Clone, Copy)]
struct CloudValidationPolicy {
    source_sha256: [u8; 32],
    pixels_sha256: [u8; 32],
    source_bytes: usize,
    width: u32,
    height: u32,
    occupied_texels: usize,
}

const NATIVE_CLOUD_VALIDATION: CloudValidationPolicy = CloudValidationPolicy {
    source_sha256: NATIVE_CLOUDS_SOURCE_SHA256,
    pixels_sha256: NATIVE_CLOUDS_PIXELS_SHA256,
    source_bytes: NATIVE_CLOUDS_SOURCE_BYTES,
    width: 256,
    height: 256,
    occupied_texels: NATIVE_CLOUDS_OCCUPIED_TEXELS,
};

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

#[derive(Clone, Copy, Debug, Default)]
pub struct AtmosphereCompileOptions<'a> {
    pub clouds_override: Option<&'a Path>,
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
    let textures =
        compile_atmosphere_textures(root, options, NATIVE_CLOUD_VALIDATION, read_texture)?;
    let (biome_profiles, fog_profiles) = compile_environment_profiles(root)?;
    Ok(CompiledAtmosphereAssets {
        source_manifest_sha256,
        textures,
        biome_profiles,
        fog_profiles,
    })
}

fn compile_atmosphere_textures(
    root: &Path,
    options: AtmosphereCompileOptions<'_>,
    cloud_policy: CloudValidationPolicy,
    mut read_pinned: impl FnMut(
        &Path,
        AtmosphereRole,
        &'static str,
        u32,
        u32,
    ) -> Result<AtmosphereTexture, AssetError>,
) -> Result<Box<[AtmosphereTexture]>, AssetError> {
    let textures = source_specs()
        .into_iter()
        .map(|(role, source_path, width, height)| {
            if role == AtmosphereRole::Clouds
                && let Some(path) = options.clouds_override
            {
                read_cloud_override_with_policy(path, cloud_policy)
            } else {
                read_pinned(root, role, source_path, width, height)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(textures.into_boxed_slice())
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

fn read_cloud_override_with_policy(
    path: &Path,
    policy: CloudValidationPolicy,
) -> Result<AtmosphereTexture, AssetError> {
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
            source: Box::new(source),
        })?;
    if dimensions != (policy.width, policy.height) {
        return Err(AssetError::WrongAtmosphereTextureDimensions {
            role: role.label(),
            path: path.to_path_buf(),
            width: dimensions.0,
            height: dimensions.1,
            expected_width: policy.width,
            expected_height: policy.height,
        });
    }
    let source_sha256: [u8; 32] = Sha256::digest(&bytes).into();
    if bytes.len() != policy.source_bytes || source_sha256 != policy.source_sha256 {
        return Err(AssetError::AtmosphereTextureHashMismatch {
            role: role.label(),
            path: path.to_path_buf(),
        });
    }

    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(policy.width);
    limits.max_image_height = Some(policy.height);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .map_err(|source| AssetError::AtmosphereTextureDecode {
            role: role.label(),
            path: path.to_path_buf(),
            source: Box::new(source),
        })?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    let pixels_sha256: [u8; 32] = Sha256::digest(&rgba8).into();
    let occupied_texels = rgba8
        .chunks_exact(4)
        .filter(|pixel| pixel[3] >= 128)
        .count();
    if rgba8.len() != pixel_length(policy.width, policy.height)?
        || pixels_sha256 != policy.pixels_sha256
        || occupied_texels != policy.occupied_texels
    {
        return Err(AssetError::AtmosphereTextureHashMismatch {
            role: role.label(),
            path: path.to_path_buf(),
        });
    }
    Ok(AtmosphereTexture {
        role,
        source_path: "textures/environment/clouds.png".into(),
        source_bytes: u32::try_from(policy.source_bytes).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "atmosphere source size",
            }
        })?,
        source_sha256,
        pixels_sha256,
        width: policy.width,
        height: policy.height,
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
            source: Box::new(source),
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
            source: Box::new(source),
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

fn pixel_length(width: u32, height: u32) -> Result<usize, AssetError> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(AssetError::BlobSizeOverflow {
            section: "atmosphere pixels",
        })
}

#[cfg(test)]
mod cloud_override_tests {
    use std::{fs, path::Path};

    use image::{Rgba, RgbaImage};
    use sha2::{Digest, Sha256};

    use super::{
        AssetError, AtmosphereCompileOptions, AtmosphereRole, AtmosphereTexture,
        CloudValidationPolicy, MAX_SOURCE_BYTES, compile_atmosphere_textures,
    };

    #[test]
    fn synthetic_policy_exercises_options_acceptance_and_replaces_only_clouds() {
        let directory = tempfile::tempdir().unwrap();
        let override_path = directory.path().join("native-clouds.png");
        write_png(&override_path, 256, 256);
        let policy = policy_for(&override_path, 256, 256, 65_536);

        let baseline = compile_atmosphere_textures(
            directory.path(),
            AtmosphereCompileOptions::default(),
            policy,
            synthetic_pinned_texture,
        )
        .unwrap();
        let overridden = compile_atmosphere_textures(
            directory.path(),
            AtmosphereCompileOptions {
                clouds_override: Some(&override_path),
            },
            policy,
            synthetic_pinned_texture,
        )
        .unwrap();

        assert_eq!(overridden[..2], baseline[..2]);
        assert_ne!(overridden[2], baseline[2]);
        assert_eq!(overridden[2].role, AtmosphereRole::Clouds);
        assert_eq!(
            overridden[2].source_path.as_ref(),
            "textures/environment/clouds.png"
        );
        assert_eq!(overridden[2].source_sha256, policy.source_sha256);
        assert_eq!(overridden[2].pixels_sha256, policy.pixels_sha256);
    }

    #[test]
    fn cloud_override_missing_path_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            compile_override(&directory.path().join("missing.png"), synthetic_policy()),
            Err(AssetError::AtmosphereTextureIo { role: "clouds", .. })
        ));
    }

    #[test]
    fn cloud_override_oversized_input_fails_before_decode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.png");
        fs::write(&path, vec![0; MAX_SOURCE_BYTES + 1]).unwrap();
        assert!(matches!(
            compile_override(&path, synthetic_policy()),
            Err(AssetError::AtmosphereTextureTooLarge { role: "clouds", .. })
        ));
    }

    #[test]
    fn cloud_override_wrong_dimensions_are_reported_before_hash_validation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wrong-dimensions.png");
        write_png(&path, 255, 256);
        assert!(matches!(
            compile_override(&path, synthetic_policy()),
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
        let mut policy = policy_for(&path, 256, 256, 65_536);
        policy.source_sha256[0] ^= 1;
        assert!(matches!(
            compile_override(&path, policy),
            Err(AssetError::AtmosphereTextureHashMismatch { role: "clouds", .. })
        ));
    }

    fn synthetic_policy() -> CloudValidationPolicy {
        CloudValidationPolicy {
            source_sha256: [0x11; 32],
            pixels_sha256: [0x22; 32],
            source_bytes: 7_880,
            width: 256,
            height: 256,
            occupied_texels: 65_536,
        }
    }

    fn compile_override(
        path: &Path,
        policy: CloudValidationPolicy,
    ) -> Result<Box<[AtmosphereTexture]>, AssetError> {
        compile_atmosphere_textures(
            path.parent().unwrap(),
            AtmosphereCompileOptions {
                clouds_override: Some(path),
            },
            policy,
            synthetic_pinned_texture,
        )
    }

    fn policy_for(
        path: &Path,
        width: u32,
        height: u32,
        occupied_texels: usize,
    ) -> CloudValidationPolicy {
        let bytes = fs::read(path).unwrap();
        let rgba8 = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8()
            .into_raw();
        CloudValidationPolicy {
            source_sha256: Sha256::digest(&bytes).into(),
            pixels_sha256: Sha256::digest(&rgba8).into(),
            source_bytes: bytes.len(),
            width,
            height,
            occupied_texels,
        }
    }

    fn synthetic_pinned_texture(
        _root: &Path,
        role: AtmosphereRole,
        source_path: &'static str,
        width: u32,
        height: u32,
    ) -> Result<AtmosphereTexture, AssetError> {
        let rgba8 = vec![role as u8; width as usize * height as usize * 4].into_boxed_slice();
        Ok(AtmosphereTexture {
            role,
            source_path: source_path.into(),
            source_bytes: 1,
            source_sha256: [role as u8; 32],
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8,
        })
    }

    fn write_png(path: &Path, width: u32, height: u32) {
        RgbaImage::from_pixel(width, height, Rgba([0x44, 0x55, 0x66, 0xff]))
            .save(path)
            .unwrap();
    }
}

type CompiledEnvironmentProfiles = (Box<[BiomeVisualProfile]>, Box<[FogProfile]>);

fn compile_environment_profiles(root: &Path) -> Result<CompiledEnvironmentProfiles, AssetError> {
    let mut fogs = Vec::new();
    let mut fog_identifiers = BTreeSet::new();
    for source in sorted_environment_files(&root.join("fogs"), ".json")? {
        let document: FogSettingsDocument = read_environment_json(&source)?;
        let identifier = document.settings.description.identifier;
        validate_environment_identifier(&identifier)?;
        if !fog_identifiers.insert(identifier.clone()) {
            return Err(invalid(format!("duplicate fog profile {identifier}")));
        }
        if document.settings.distance.is_empty()
            || document.settings.distance.len() > MAX_FOG_DISTANCES
        {
            return Err(invalid(format!(
                "fog profile {identifier} distance count is outside 1..={MAX_FOG_DISTANCES}"
            )));
        }
        let mut distances = Vec::with_capacity(document.settings.distance.len());
        for (medium, distance) in document.settings.distance {
            let medium = FogMedium::from_source_name(&medium)
                .ok_or_else(|| invalid(format!("unsupported fog medium {medium}")))?;
            let mode = FogDistanceMode::from_source_name(&distance.render_distance_type)
                .ok_or_else(|| {
                    invalid(format!(
                        "unsupported fog distance mode {}",
                        distance.render_distance_type
                    ))
                })?;
            if !distance.fog_start.is_finite()
                || !distance.fog_end.is_finite()
                || distance.fog_start < 0.0
                || distance.fog_end < distance.fog_start
            {
                return Err(invalid(format!(
                    "fog profile {identifier} has invalid {medium:?} distances"
                )));
            }
            distances.push(FogDistance {
                medium,
                mode,
                start_bits: distance.fog_start.to_bits(),
                end_bits: distance.fog_end.to_bits(),
                rgb8: parse_environment_rgb(&distance.fog_color)?,
            });
        }
        distances.sort_unstable_by_key(|distance| distance.medium);
        fogs.push(FogProfile {
            identifier: identifier.into_boxed_str(),
            distances: distances.into_boxed_slice(),
        });
    }
    fogs.sort_unstable_by(|left, right| left.identifier.cmp(&right.identifier));

    let mut biomes = Vec::new();
    let mut biome_identifiers = BTreeSet::new();
    for source in sorted_environment_files(&root.join("biomes"), ".client_biome.json")? {
        let document: ClientBiomeDocument = read_environment_json(&source)?;
        let biome = document.biome;
        validate_environment_identifier(&biome.description.identifier)?;
        validate_environment_identifier(&biome.components.fog.fog_identifier)?;
        validate_environment_identifier(&biome.components.atmosphere.atmosphere_identifier)?;
        validate_environment_identifier(&biome.components.lighting.lighting_identifier)?;
        if !biome_identifiers.insert(biome.description.identifier.clone()) {
            return Err(invalid(format!(
                "duplicate biome visual profile {}",
                biome.description.identifier
            )));
        }
        if !fog_identifiers.contains(&biome.components.fog.fog_identifier) {
            return Err(invalid(format!(
                "biome {} references missing fog profile {}",
                biome.description.identifier, biome.components.fog.fog_identifier
            )));
        }
        biomes.push(BiomeVisualProfile {
            biome_identifier: biome.description.identifier.into_boxed_str(),
            fog_identifier: biome.components.fog.fog_identifier.into_boxed_str(),
            atmosphere_identifier: biome
                .components
                .atmosphere
                .atmosphere_identifier
                .into_boxed_str(),
            lighting_identifier: biome
                .components
                .lighting
                .lighting_identifier
                .into_boxed_str(),
            sky_rgb8: biome
                .components
                .sky
                .map(|sky| parse_environment_rgb(&sky.sky_color))
                .transpose()?,
        });
    }
    if biomes.is_empty() || biomes.len() > MAX_ENVIRONMENT_PROFILES {
        return Err(invalid(format!(
            "biome visual profile count is outside 1..={MAX_ENVIRONMENT_PROFILES}"
        )));
    }
    if fogs.is_empty() || fogs.len() > MAX_ENVIRONMENT_PROFILES {
        return Err(invalid(format!(
            "fog profile count is outside 1..={MAX_ENVIRONMENT_PROFILES}"
        )));
    }
    biomes.sort_unstable_by(|left, right| left.biome_identifier.cmp(&right.biome_identifier));
    Ok((biomes.into_boxed_slice(), fogs.into_boxed_slice()))
}

#[cfg(test)]
mod environment_profile_tests {
    use std::fs;

    use super::compile_environment_profiles;
    use assets::{FogDistanceMode, FogMedium};

    #[test]
    fn pinned_shape_profiles_compile_exact_default_nether_and_end_metadata() {
        let root = tempfile::tempdir().unwrap();
        write_biome(
            root.path(),
            "plains",
            "minecraft:plains",
            "minecraft:fog_plains",
            "minecraft:default_atmospherics",
            "minecraft:default_lighting",
            None,
        );
        write_biome(
            root.path(),
            "hell",
            "minecraft:hell",
            "minecraft:fog_hell",
            "minecraft:hell_atmospherics",
            "minecraft:nether_lighting",
            None,
        );
        write_biome(
            root.path(),
            "the_end",
            "minecraft:the_end",
            "minecraft:fog_the_end",
            "minecraft:end_atmospherics",
            "minecraft:end_lighting",
            Some("#000000"),
        );
        write_fog(
            root.path(),
            "default",
            "minecraft:fog_plains",
            0.92,
            1.0,
            "#ABD2FF",
            "render",
        );
        write_fog(
            root.path(),
            "hell",
            "minecraft:fog_hell",
            10.0,
            96.0,
            "#330808",
            "fixed",
        );
        write_fog(
            root.path(),
            "the_end",
            "minecraft:fog_the_end",
            0.92,
            1.0,
            "#0B080C",
            "render",
        );

        let (biomes, fogs) = compile_environment_profiles(root.path()).unwrap();
        assert_eq!(biomes.len(), 3);
        let plains = biomes
            .iter()
            .find(|profile| profile.biome_identifier.as_ref() == "minecraft:plains")
            .unwrap();
        assert_eq!(plains.fog_identifier.as_ref(), "minecraft:fog_plains");
        assert_eq!(
            plains.atmosphere_identifier.as_ref(),
            "minecraft:default_atmospherics"
        );
        assert_eq!(
            plains.lighting_identifier.as_ref(),
            "minecraft:default_lighting"
        );
        assert_eq!(plains.sky_rgb8, None);

        let hell = biomes
            .iter()
            .find(|profile| profile.biome_identifier.as_ref() == "minecraft:hell")
            .unwrap();
        assert_eq!(hell.fog_identifier.as_ref(), "minecraft:fog_hell");
        assert_eq!(
            hell.atmosphere_identifier.as_ref(),
            "minecraft:hell_atmospherics"
        );
        assert_eq!(
            hell.lighting_identifier.as_ref(),
            "minecraft:nether_lighting"
        );

        let end = biomes
            .iter()
            .find(|profile| profile.biome_identifier.as_ref() == "minecraft:the_end")
            .unwrap();
        assert_eq!(end.sky_rgb8, Some(0x00_0000));
        assert_eq!(end.fog_identifier.as_ref(), "minecraft:fog_the_end");

        let plains_fog = fogs
            .iter()
            .find(|profile| profile.identifier.as_ref() == "minecraft:fog_plains")
            .unwrap()
            .distance(FogMedium::Air)
            .unwrap();
        assert_eq!(plains_fog.mode, FogDistanceMode::RenderRelative);
        assert_eq!((plains_fog.start(), plains_fog.end()), (0.92, 1.0));
        assert_eq!(plains_fog.rgb8, 0xAB_D2_FF);

        let hell_fog = fogs
            .iter()
            .find(|profile| profile.identifier.as_ref() == "minecraft:fog_hell")
            .unwrap()
            .distance(FogMedium::Air)
            .unwrap();
        assert_eq!(hell_fog.mode, FogDistanceMode::Fixed);
        assert_eq!((hell_fog.start(), hell_fog.end()), (10.0, 96.0));
        assert_eq!(hell_fog.rgb8, 0x33_08_08);

        let end_fog = fogs
            .iter()
            .find(|profile| profile.identifier.as_ref() == "minecraft:fog_the_end")
            .unwrap()
            .distance(FogMedium::Air)
            .unwrap();
        assert_eq!(end_fog.rgb8, 0x0B_08_0C);
    }

    fn write_biome(
        root: &std::path::Path,
        file: &str,
        biome: &str,
        fog: &str,
        atmosphere: &str,
        lighting: &str,
        sky: Option<&str>,
    ) {
        let directory = root.join("biomes");
        fs::create_dir_all(&directory).unwrap();
        let mut components = serde_json::json!({
            "minecraft:fog_appearance": {"fog_identifier": fog},
            "minecraft:atmosphere_identifier": {"atmosphere_identifier": atmosphere},
            "minecraft:lighting_identifier": {"lighting_identifier": lighting},
        });
        if let Some(value) = sky {
            components.as_object_mut().unwrap().insert(
                "minecraft:sky_color".to_owned(),
                serde_json::json!({"sky_color": value}),
            );
        }
        fs::write(
            directory.join(format!("{file}.client_biome.json")),
            serde_json::to_vec(&serde_json::json!({
                "minecraft:client_biome": {
                    "description": {"identifier": biome},
                    "components": components,
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn write_fog(
        root: &std::path::Path,
        file: &str,
        identifier: &str,
        start: f32,
        end: f32,
        rgb: &str,
        mode: &str,
    ) {
        let directory = root.join("fogs");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{file}_fog_setting.json")),
            serde_json::to_vec(&serde_json::json!({
                "minecraft:fog_settings": {
                    "description": {"identifier": identifier},
                    "distance": {
                        "air": {
                            "fog_start": start,
                            "fog_end": end,
                            "fog_color": rgb,
                            "render_distance_type": mode,
                        }
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
