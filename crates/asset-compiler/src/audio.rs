//! Deterministic compiler for the pinned vanilla sound-definition catalog.

use crate::entity::validate_vanilla_source_manifest;
use assets::{
    AudioAlternative, AudioDefinition, MAX_AUDIO_ALTERNATIVES,
    MAX_AUDIO_ALTERNATIVES_PER_DEFINITION, MAX_AUDIO_CATEGORY_BYTES, MAX_AUDIO_DEFINITIONS,
    MAX_AUDIO_IDENTIFIER_BYTES, MAX_AUDIO_PATH_BYTES, MAX_AUDIO_SUBTITLE_BYTES,
    encode_audio_catalog,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
use thiserror::Error;

pub const AUDIO_SOUND_DEFINITIONS_RELATIVE_PATH: &str = "sounds/sound_definitions.json";
pub const MAX_AUDIO_SOURCE_BYTES: usize = 2 * 1024 * 1024;
pub const PINNED_SOUND_DEFINITIONS_SHA256: [u8; 32] = [
    0xae, 0xd4, 0x36, 0xa8, 0x50, 0x92, 0xa9, 0xef, 0x12, 0xca, 0x05, 0xd1, 0x71, 0xca, 0x53, 0xc3,
    0x34, 0xf9, 0xdf, 0x2f, 0x99, 0xff, 0xca, 0xd8, 0x23, 0xdd, 0xab, 0x72, 0x43, 0xa8, 0x85, 0xbb,
];

#[derive(Clone, Debug)]
pub struct CompiledAudioCarrier {
    pub bytes: Vec<u8>,
    pub report: AudioCompileReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub sound_definitions_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub definition_count: usize,
    pub alternative_count: usize,
    pub scalar_alternative_count: usize,
    pub object_alternative_count: usize,
}

pub fn compile_audio_assets(
    root: &Path,
    source_manifest: &[u8],
) -> Result<CompiledAudioCarrier, AudioCompileError> {
    let source_manifest_sha256 =
        validate_vanilla_source_manifest(source_manifest).map_err(AudioCompileError::Manifest)?;
    let root = root
        .canonicalize()
        .map_err(|source| AudioCompileError::Read {
            path: root.display().to_string().into_boxed_str(),
            source,
        })?;
    if !root.is_dir() {
        return invalid("audio pack root is not a directory");
    }
    let path = root.join(AUDIO_SOUND_DEFINITIONS_RELATIVE_PATH);
    let metadata = fs::symlink_metadata(&path).map_err(|source| AudioCompileError::Read {
        path: path.display().to_string().into_boxed_str(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return invalid("sound_definitions.json must be a regular non-symlink file");
    }
    let canonical = path
        .canonicalize()
        .map_err(|source| AudioCompileError::Read {
            path: path.display().to_string().into_boxed_str(),
            source,
        })?;
    if !canonical.starts_with(&root) {
        return invalid("sound_definitions.json resolves outside the pack root");
    }
    let source_len = usize::try_from(metadata.len()).map_err(|_| {
        AudioCompileError::Invalid("sound definitions length exceeds platform".into())
    })?;
    if source_len > MAX_AUDIO_SOURCE_BYTES {
        return invalid("sound definitions source exceeds the 2 MiB bound");
    }
    let source = fs::read(&canonical).map_err(|source| AudioCompileError::Read {
        path: canonical.display().to_string().into_boxed_str(),
        source,
    })?;
    compile_audio_source(
        &source,
        source_manifest_sha256,
        PINNED_SOUND_DEFINITIONS_SHA256,
    )
}

fn compile_audio_source(
    source: &[u8],
    source_manifest_sha256: [u8; 32],
    required_source_sha256: [u8; 32],
) -> Result<CompiledAudioCarrier, AudioCompileError> {
    if source.len() > MAX_AUDIO_SOURCE_BYTES {
        return invalid("sound definitions source exceeds the 2 MiB bound");
    }
    let sound_definitions_sha256: [u8; 32] = Sha256::digest(source).into();
    if sound_definitions_sha256 != required_source_sha256 {
        return Err(AudioCompileError::SourcePin);
    }
    let root: RawRoot = serde_json::from_slice(source).map_err(AudioCompileError::Json)?;
    if root.sound_definitions.len() > MAX_AUDIO_DEFINITIONS {
        return invalid("sound definition count exceeds bound");
    }
    let mut definitions = Vec::with_capacity(root.sound_definitions.len());
    let mut alternative_count = 0usize;
    let mut scalar_alternative_count = 0usize;
    let mut object_alternative_count = 0usize;
    for (identifier, raw) in root.sound_definitions {
        validate_source_string(&identifier, MAX_AUDIO_IDENTIFIER_BYTES, "identifier")?;
        validate_optional_source_string(&raw.category, MAX_AUDIO_CATEGORY_BYTES, "category")?;
        validate_optional_source_string(&raw.subtitle, MAX_AUDIO_SUBTITLE_BYTES, "subtitle")?;
        validate_optional_source_string(
            &raw.use_legacy_max_distance,
            MAX_AUDIO_CATEGORY_BYTES,
            "legacy-distance token",
        )?;
        if raw.sounds.len() > MAX_AUDIO_ALTERNATIVES_PER_DEFINITION {
            return invalid("per-definition alternative count exceeds bound");
        }
        alternative_count = alternative_count
            .checked_add(raw.sounds.len())
            .ok_or_else(|| AudioCompileError::Invalid("alternative count overflow".into()))?;
        if alternative_count > MAX_AUDIO_ALTERNATIVES {
            return invalid("total alternative count exceeds bound");
        }
        let mut alternatives = Vec::with_capacity(raw.sounds.len());
        for sound in raw.sounds {
            let alternative = match sound {
                RawSound::Scalar(name) => {
                    scalar_alternative_count += 1;
                    AudioAlternative {
                        object_form: false,
                        name: name.into_boxed_str(),
                        weight: 1,
                        volume: None,
                        pitch: None,
                        is_3d: None,
                        stream: None,
                        load_on_low_memory: None,
                    }
                }
                RawSound::Object(sound) => {
                    object_alternative_count += 1;
                    AudioAlternative {
                        object_form: true,
                        name: sound.name.into_boxed_str(),
                        weight: sound.weight.unwrap_or(1),
                        volume: sound.volume,
                        pitch: sound.pitch,
                        is_3d: sound.is_3d,
                        stream: sound.stream,
                        load_on_low_memory: sound.load_on_low_memory,
                    }
                }
            };
            validate_source_string(&alternative.name, MAX_AUDIO_PATH_BYTES, "sound path")?;
            if alternative.weight == 0 {
                return invalid("sound alternative weight must be positive");
            }
            validate_finite(alternative.volume, "alternative volume")?;
            validate_finite(alternative.pitch, "alternative pitch")?;
            alternatives.push(alternative);
        }
        validate_finite(raw.min_distance, "definition min_distance")?;
        validate_finite(raw.max_distance, "definition max_distance")?;
        validate_finite(raw.volume, "definition volume")?;
        validate_finite(raw.pitch, "definition pitch")?;
        definitions.push(AudioDefinition {
            identifier: identifier.into_boxed_str(),
            category: raw.category.map(String::into_boxed_str),
            subtitle: raw.subtitle.map(String::into_boxed_str),
            min_distance: raw.min_distance,
            max_distance: raw.max_distance,
            volume: raw.volume,
            pitch: raw.pitch,
            use_legacy_max_distance: raw.use_legacy_max_distance.map(String::into_boxed_str),
            alternatives: alternatives.into_boxed_slice(),
        });
    }
    let bytes = encode_audio_catalog(
        source_manifest_sha256,
        sound_definitions_sha256,
        &definitions,
    )?;
    Ok(CompiledAudioCarrier {
        report: AudioCompileReport {
            source_manifest_sha256,
            sound_definitions_sha256,
            carrier_sha256: Sha256::digest(&bytes).into(),
            definition_count: definitions.len(),
            alternative_count,
            scalar_alternative_count,
            object_alternative_count,
        },
        bytes,
    })
}

fn validate_finite(value: Option<f32>, field: &str) -> Result<(), AudioCompileError> {
    if value.is_some_and(|value| !value.is_finite()) {
        return invalid(format!("{field} must be finite"));
    }
    Ok(())
}

fn validate_optional_source_string(
    value: &Option<String>,
    maximum: usize,
    field: &str,
) -> Result<(), AudioCompileError> {
    if let Some(value) = value {
        validate_source_string(value, maximum, field)?;
    }
    Ok(())
}

fn validate_source_string(
    value: &str,
    maximum: usize,
    field: &str,
) -> Result<(), AudioCompileError> {
    if value.is_empty() || value.len() > maximum {
        return invalid(format!("{field} byte length is outside its bound"));
    }
    Ok(())
}

#[derive(Deserialize)]
struct RawRoot {
    sound_definitions: BTreeMap<String, RawDefinition>,
}

#[derive(Deserialize)]
struct RawDefinition {
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    min_distance: Option<f32>,
    #[serde(default)]
    max_distance: Option<f32>,
    #[serde(default)]
    volume: Option<f32>,
    #[serde(default)]
    pitch: Option<f32>,
    #[serde(default, rename = "__use_legacy_max_distance")]
    use_legacy_max_distance: Option<String>,
    sounds: Vec<RawSound>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawSound {
    Scalar(String),
    Object(RawSoundObject),
}

#[derive(Deserialize)]
struct RawSoundObject {
    name: String,
    #[serde(default)]
    weight: Option<u16>,
    #[serde(default)]
    volume: Option<f32>,
    #[serde(default)]
    pitch: Option<f32>,
    #[serde(default, rename = "is3D")]
    is_3d: Option<bool>,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    load_on_low_memory: Option<bool>,
}

#[derive(Debug, Error)]
pub enum AudioCompileError {
    #[error("audio source manifest is not the reviewed pin: {0}")]
    Manifest(assets::AssetError),
    #[error("sound_definitions.json does not match the reviewed source SHA-256")]
    SourcePin,
    #[error("invalid sound_definitions.json: {0}")]
    Json(serde_json::Error),
    #[error("could not read audio source {path}: {source}")]
    Read {
        path: Box<str>,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid audio source: {0}")]
    Invalid(Box<str>),
    #[error(transparent)]
    Carrier(#[from] assets::AudioCatalogError),
}

fn invalid<T>(detail: impl Into<Box<str>>) -> Result<T, AudioCompileError> {
    Err(AudioCompileError::Invalid(detail.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::RuntimeAudioCatalog;
    use tempfile::tempdir;

    fn compile(source: &[u8]) -> Result<CompiledAudioCarrier, AudioCompileError> {
        compile_audio_source(source, [0x11; 32], Sha256::digest(source).into())
    }

    #[test]
    fn scalar_and_object_forms_round_trip_without_interpreting_unknown_fields() {
        let source = br#"{"sound_definitions":{"z":{"category":"music","subtitle":"sub","min_distance":0,"max_distance":256,"volume":1000,"pitch":0.1,"__use_legacy_max_distance":"true","sounds":["sounds/z",{"name":"sounds/a","weight":90,"volume":0.01,"pitch":4,"is3D":false,"stream":true,"load_on_low_memory":false,"pitch:":999}]},"a":{"sounds":[{"name":"sounds/b"}]}}}"#;
        let compiled = compile(source).unwrap();
        assert_eq!(compiled.report.definition_count, 2);
        assert_eq!(compiled.report.alternative_count, 3);
        assert_eq!(compiled.report.scalar_alternative_count, 1);
        assert_eq!(compiled.report.object_alternative_count, 2);
        let catalog = RuntimeAudioCatalog::decode(&compiled.bytes).unwrap();
        assert_eq!(catalog.definitions()[0].identifier.as_ref(), "a");
        let definition = catalog.lookup("z").unwrap();
        assert_eq!(definition.volume, Some(1000.0));
        assert_eq!(definition.alternatives[0].name.as_ref(), "sounds/a");
        assert_eq!(definition.alternatives[0].pitch, Some(4.0));
        assert!(!definition.alternatives[1].object_form);
        assert!(catalog.lookup("unknown.custom").is_none());
    }

    #[test]
    fn source_order_does_not_change_canonical_carrier() {
        let one = br#"{"sound_definitions":{"b":{"sounds":["z","a"]},"a":{"sounds":["b"]}}}"#;
        let two = br#"{"sound_definitions":{"a":{"sounds":["b"]},"b":{"sounds":["a","z"]}}}"#;
        let source_hash = [9; 32];
        let a = compile_audio_source(one, [1; 32], Sha256::digest(one).into()).unwrap();
        let b = compile_audio_source(two, [1; 32], Sha256::digest(two).into()).unwrap();
        assert_ne!(
            a.bytes, b.bytes,
            "source identity is intentionally embedded"
        );
        let ca = RuntimeAudioCatalog::decode(&a.bytes).unwrap();
        let cb = RuntimeAudioCatalog::decode(&b.bytes).unwrap();
        assert_eq!(ca.definitions(), cb.definitions());
        assert_ne!(source_hash, ca.sound_definitions_sha256());
    }

    #[test]
    fn parser_rejects_counts_strings_weights_nonfinite_and_wrong_pin() {
        let zero_weight = br#"{"sound_definitions":{"a":{"sounds":[{"name":"x","weight":0}]}}}"#;
        assert!(matches!(
            compile(zero_weight),
            Err(AudioCompileError::Invalid(_))
        ));
        let long = format!(
            r#"{{"sound_definitions":{{"{}":{{"sounds":["x"]}}}}}}"#,
            "x".repeat(257)
        );
        assert!(matches!(
            compile(long.as_bytes()),
            Err(AudioCompileError::Invalid(_))
        ));
        let source = br#"{"sound_definitions":{}}"#;
        assert!(matches!(
            compile_audio_source(source, [0; 32], [3; 32]),
            Err(AudioCompileError::SourcePin)
        ));
        let too_many = (0..65).map(|_| "\"x\"").collect::<Vec<_>>().join(",");
        let source = format!(r#"{{"sound_definitions":{{"a":{{"sounds":[{too_many}]}}}}}}"#);
        assert!(matches!(
            compile(source.as_bytes()),
            Err(AudioCompileError::Invalid(_))
        ));
    }

    #[test]
    fn filesystem_entrypoint_requires_the_exact_fixed_regular_source_path() {
        let directory = tempdir().unwrap();
        let manifest = include_bytes!("../../../assets/vanilla-source.json");
        assert!(matches!(
            compile_audio_assets(directory.path(), manifest),
            Err(AudioCompileError::Read { .. })
        ));
        fs::create_dir_all(directory.path().join("sounds/sound_definitions.json")).unwrap();
        assert!(matches!(
            compile_audio_assets(directory.path(), manifest),
            Err(AudioCompileError::Invalid(_))
        ));
    }
}
