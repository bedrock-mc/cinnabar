//! Compile one reviewed finite, predecoded nonspatial sample. No playback or
//! looping is activated; a streaming source flag does not change this mode.

use assets::{AudioPcmExpectedIdentity, MAX_AUDIO_PCM_SOURCE_BYTES, RuntimeAudioCatalog};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

#[derive(Debug, thiserror::Error)]
pub enum AudioPcmCompileError {
    #[error(transparent)]
    Asset(#[from] assets::AssetError),
    #[error(transparent)]
    Catalog(#[from] assets::AudioCatalogError),
    #[error(transparent)]
    Pcm(#[from] assets::AudioPcmError),
    #[error(transparent)]
    Decode(#[from] crate::FadpcmDecodeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid reviewed PCM input: {0}")]
    Invalid(&'static str),
}

#[derive(Debug)]
pub struct CompiledAudioPcmCarrier {
    pub bytes: Vec<u8>,
    pub report: AudioPcmCompileReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AudioPcmCompileReport {
    pub schema: u32,
    pub identifier: Box<str>,
    pub source_relative_path: Box<str>,
    pub source_manifest_sha256: Box<str>,
    pub sound_definitions_sha256: Box<str>,
    pub catalog_sha256: Box<str>,
    pub source_sha256: Box<str>,
    pub pcm_sha256: Box<str>,
    pub carrier_sha256: Box<str>,
    pub source_bytes: u32,
    pub pcm_bytes: usize,
    pub channels: u8,
    pub sample_rate: u32,
    pub frames: u32,
    pub mode: &'static str,
    pub source_streaming: bool,
    pub exact_target_sample_identity: bool,
}

pub fn compile_audio_pcm_assets(
    root: &Path,
    catalog_bytes: &[u8],
    source_manifest: &[u8],
) -> Result<CompiledAudioPcmCarrier, AudioPcmCompileError> {
    let manifest = crate::entity::validate_vanilla_source_manifest(source_manifest)?;
    let expected = assets::reviewed_audio_pcm_identity();
    if manifest != expected.source_manifest_sha256() {
        return Err(AudioPcmCompileError::Invalid("manifest identity"));
    }
    compile_with_expected(root, catalog_bytes, &expected)
}

fn compile_with_expected(
    root: &Path,
    catalog_bytes: &[u8],
    expected: &AudioPcmExpectedIdentity,
) -> Result<CompiledAudioPcmCarrier, AudioPcmCompileError> {
    let catalog = RuntimeAudioCatalog::decode(catalog_bytes)?;
    let actual_catalog_sha = Sha256::digest(catalog_bytes).into();
    assets::validate_audio_pcm_catalog(&catalog, actual_catalog_sha, expected)?;
    let root = root.canonicalize()?;
    let path = root.join(expected.source_path());
    let mut file = crate::entity::open_source_handle(&root, &path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.len() != u64::from(expected.source_bytes())
        || metadata.len() > u64::from(MAX_AUDIO_PCM_SOURCE_BYTES)
    {
        return Err(AudioPcmCompileError::Invalid(
            "compressed source byte bound or identity",
        ));
    }
    let mut source = Vec::new();
    source
        .try_reserve_exact(expected.source_bytes() as usize)
        .map_err(|_| AudioPcmCompileError::Invalid("source allocation"))?;
    source.resize(expected.source_bytes() as usize, 0);
    file.read_exact(&mut source)?;
    let mut extra = [0; 1];
    if file.read(&mut extra)? != 0
        || file.metadata()?.len() != metadata.len()
        || Sha256::digest(&source)[..] != expected.source_sha256()
    {
        return Err(AudioPcmCompileError::Invalid(
            "source changed or source SHA mismatch",
        ));
    }
    let decoded = crate::decode_fsb5_fadpcm(&source)?;
    if decoded.channels() != expected.channels()
        || decoded.sample_rate() != expected.sample_rate()
        || decoded.frames() as u64 != u64::from(expected.frames())
    {
        return Err(AudioPcmCompileError::Invalid(
            "decoded metadata differs from reviewed expectation",
        ));
    }
    let bytes = assets::encode_audio_pcm(expected, decoded.samples())?;
    let report = AudioPcmCompileReport {
        schema: 1,
        identifier: expected.identifier().into(),
        source_relative_path: expected.source_path().into(),
        source_manifest_sha256: hex(expected.source_manifest_sha256()),
        sound_definitions_sha256: hex(expected.sound_definitions_sha256()),
        catalog_sha256: hex(expected.catalog_sha256()),
        source_sha256: hex(expected.source_sha256()),
        pcm_sha256: hex(expected.pcm_sha256()),
        carrier_sha256: hex(Sha256::digest(&bytes).into()),
        source_bytes: expected.source_bytes(),
        pcm_bytes: decoded.samples().len() * 2,
        channels: expected.channels(),
        sample_rate: expected.sample_rate(),
        frames: expected.frames(),
        mode: "finite_predecoded_no_loop",
        source_streaming: true,
        exact_target_sample_identity: false,
    };
    Ok(CompiledAudioPcmCarrier { bytes, report })
}

fn hex(hash: [u8; 32]) -> Box<str> {
    hash.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .into_boxed_str()
}

#[cfg(test)]
mod tests;
