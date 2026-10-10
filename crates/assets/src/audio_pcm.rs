//! A single bounded PCM sample, authenticated against caller-supplied expected
//! identities. Expected hashes must come from reviewed independent inputs, never
//! from the carrier being decoded. Hashes do not themselves prove codec semantics.

use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"MCBEPCM1";
const HEADER_BYTES: usize = 196;
pub const MAX_AUDIO_PCM_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_AUDIO_PCM_CARRIER_BYTES: usize = MAX_AUDIO_PCM_BYTES + 8192;
pub const MAX_AUDIO_PCM_SOURCE_BYTES: u32 = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("invalid bounded PCM carrier: {0}")]
pub struct AudioPcmError(&'static str);
fn invalid(detail: &'static str) -> AudioPcmError {
    AudioPcmError(detail)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPcmMode {
    FinitePredecodedNoLoop,
}

/// Trusted expected identity, supplied separately from untrusted carrier bytes.
/// Construction checks representation/bounds, not the caller's source authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPcmExpectedIdentity {
    identifier: Box<str>,
    source_path: Box<str>,
    hashes: [[u8; 32]; 5],
    source_bytes: u32,
    channels: u8,
    sample_rate: u32,
    frames: u32,
}

impl AudioPcmExpectedIdentity {
    // Every expected scalar is explicit; none may be borrowed from the payload.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identifier: &str,
        source_path: &str,
        catalog_sha: [u8; 32],
        manifest_sha: [u8; 32],
        definitions_sha: [u8; 32],
        source_sha: [u8; 32],
        pcm_sha: [u8; 32],
        source_bytes: u32,
        channels: u8,
        sample_rate: u32,
        frames: u32,
    ) -> Result<Self, AudioPcmError> {
        if identifier.is_empty()
            || identifier.len() > 256
            || identifier.contains('\0')
            || source_path.len() > 256
            || !source_path.starts_with("sounds/")
            || !source_path.ends_with(".fsb")
            || source_path.contains(['\\', '\0', ':'])
            || source_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(invalid("identifier or canonical source path"));
        }
        let hashes = [
            catalog_sha,
            manifest_sha,
            definitions_sha,
            source_sha,
            pcm_sha,
        ];
        if hashes.contains(&[0; 32])
            || source_bytes == 0
            || source_bytes > MAX_AUDIO_PCM_SOURCE_BYTES
            || !matches!(channels, 1 | 2)
            || !(4000..=96000).contains(&sample_rate)
            || frames == 0
        {
            return Err(invalid("expected metadata or hashes"));
        }
        let identity = Self {
            identifier: identifier.into(),
            source_path: source_path.into(),
            hashes,
            source_bytes,
            channels,
            sample_rate,
            frames,
        };
        identity.pcm_bytes()?;
        Ok(identity)
    }
    pub fn identifier(&self) -> &str {
        &self.identifier
    }
    pub fn source_path(&self) -> &str {
        &self.source_path
    }
    pub fn catalog_sha256(&self) -> [u8; 32] {
        self.hashes[0]
    }
    pub fn source_manifest_sha256(&self) -> [u8; 32] {
        self.hashes[1]
    }
    pub fn sound_definitions_sha256(&self) -> [u8; 32] {
        self.hashes[2]
    }
    pub fn source_sha256(&self) -> [u8; 32] {
        self.hashes[3]
    }
    pub fn pcm_sha256(&self) -> [u8; 32] {
        self.hashes[4]
    }
    pub fn source_bytes(&self) -> u32 {
        self.source_bytes
    }
    pub fn channels(&self) -> u8 {
        self.channels
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn frames(&self) -> u32 {
        self.frames
    }
    fn pcm_bytes(&self) -> Result<usize, AudioPcmError> {
        let bytes = u64::from(self.frames)
            .checked_mul(u64::from(self.channels))
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(|| invalid("PCM size overflow"))?;
        if bytes > MAX_AUDIO_PCM_BYTES as u64 {
            return Err(invalid("PCM byte bound"));
        }
        usize::try_from(bytes).map_err(|_| invalid("PCM platform size"))
    }
}

const fn hash(hex: &str) -> [u8; 32] {
    let text = hex.as_bytes();
    assert!(text.len() == 64);
    let mut output = [0; 32];
    let mut index = 0;
    while index < 64 {
        let value = match text[index] {
            b'0'..=b'9' => text[index] - b'0',
            b'a'..=b'f' => text[index] - b'a' + 10,
            _ => panic!("invalid static checksum"),
        };
        output[index / 2] |= value << (if index % 2 == 0 { 4 } else { 0 });
        index += 1;
    }
    output
}

/// Public preview sample identity, independently compared as no-loop PCM16 with
/// vgmstream r2117. This is not an assertion of target 1.26.40 sample identity.
pub fn reviewed_audio_pcm_identity() -> AudioPcmExpectedIdentity {
    AudioPcmExpectedIdentity::new(
        "ambient.underwater.loop",
        "sounds/ambient/underwater/loop/underwater_ambience.fsb",
        hash("6257771e16831f3d96281c5b04b860575777893ebfb815c13e22ff9457b4e9bb"),
        crate::vanilla_source_manifest_sha256(),
        hash("f6ebf5fe07d67355698a4fa2d138217753b3b7da2b94e3162c94da48f9a69fe6"),
        hash("528126e3702b196e6116bcc8ec3f8dfd9fb4fe716842eca854016eb0a57f86ec"),
        hash("91f8716fe45282ee412c4c1964c9cae1a5e6fef3e48458bcb602e711635a300e"),
        869792,
        2,
        29015,
        795136,
    )
    .expect("reviewed PCM identity is well formed")
}

/// Link a sample to the exact independently selected catalog bytes and route.
pub fn validate_audio_pcm_catalog(
    catalog: &crate::RuntimeAudioCatalog,
    actual_catalog_sha: [u8; 32],
    expected: &AudioPcmExpectedIdentity,
) -> Result<(), AudioPcmError> {
    if actual_catalog_sha != expected.catalog_sha256()
        || catalog.source_manifest_sha256() != expected.source_manifest_sha256()
        || catalog.sound_definitions_sha256() != expected.sound_definitions_sha256()
    {
        return Err(invalid("catalog identity"));
    }
    let definition = catalog
        .lookup(expected.identifier())
        .ok_or_else(|| invalid("catalog member"))?;
    if definition.alternatives.len() != 1
        || definition.volume.unwrap_or(1.0) != 1.0
        || definition.pitch.unwrap_or(1.0) != 1.0
    {
        return Err(invalid("definition dynamics or alternatives"));
    }
    let alternative = &definition.alternatives[0];
    if !alternative.object_form
        || alternative.is_3d != Some(false)
        || alternative.stream != Some(true)
        || alternative.name.as_ref()
            != expected
                .source_path()
                .strip_suffix(".fsb")
                .ok_or_else(|| invalid("source extension"))?
        || alternative.volume.unwrap_or(1.0) != 1.0
        || alternative.pitch.unwrap_or(1.0) != 1.0
    {
        return Err(invalid("nonspatial finite-predecode route"));
    }
    Ok(())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("checked header"),
    )
}
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("checked header"),
    )
}

#[derive(Debug, Clone)]
pub struct RuntimeAudioPcm {
    identity: AudioPcmExpectedIdentity,
    samples: Arc<[i16]>,
    carrier_sha256: [u8; 32],
}
impl RuntimeAudioPcm {
    pub fn decode(
        bytes: &[u8],
        catalog: &crate::RuntimeAudioCatalog,
        actual_catalog_sha: [u8; 32],
        expected: &AudioPcmExpectedIdentity,
    ) -> Result<Self, AudioPcmError> {
        validate_audio_pcm_catalog(catalog, actual_catalog_sha, expected)?;
        if !(HEADER_BYTES + 32..=MAX_AUDIO_PCM_CARRIER_BYTES).contains(&bytes.len()) {
            return Err(invalid("carrier byte bound"));
        }
        if bytes.get(..8) != Some(MAGIC) || u32_at(bytes, 8) != 1 || u16_at(bytes, 14) != 1 {
            return Err(invalid("magic, schema or decode mode"));
        }
        if u16_at(bytes, 12) != u16::from(expected.channels())
            || u32_at(bytes, 16) != expected.sample_rate()
            || u32_at(bytes, 20) != expected.frames()
            || u32_at(bytes, 24) != expected.source_bytes()
        {
            return Err(invalid("sample metadata"));
        }
        let pcm_len = expected.pcm_bytes()?;
        if u32_at(bytes, 32) as u64 != pcm_len as u64
            || usize::from(u16_at(bytes, 28)) != expected.identifier().len()
            || usize::from(u16_at(bytes, 30)) != expected.source_path().len()
        {
            return Err(invalid("section lengths"));
        }
        let pcm_start = HEADER_BYTES
            .checked_add(expected.identifier().len())
            .and_then(|n| n.checked_add(expected.source_path().len()))
            .ok_or_else(|| invalid("section overflow"))?;
        let hash_offset = pcm_start
            .checked_add(pcm_len)
            .ok_or_else(|| invalid("PCM section overflow"))?;
        if hash_offset.checked_add(32) != Some(bytes.len()) {
            return Err(invalid("total length"));
        }
        for (index, hash) in expected.hashes.iter().enumerate() {
            if bytes[36 + index * 32..68 + index * 32] != *hash {
                return Err(invalid("expected hash mismatch"));
            }
        }
        if &bytes[HEADER_BYTES..HEADER_BYTES + expected.identifier().len()]
            != expected.identifier().as_bytes()
            || &bytes[HEADER_BYTES + expected.identifier().len()..pcm_start]
                != expected.source_path().as_bytes()
        {
            return Err(invalid("exact binding"));
        }
        let carrier_sha256 = crate::encoding::sealed_identity(bytes, hash_offset)
            .filter(|_| Sha256::digest(&bytes[pcm_start..hash_offset])[..] == expected.pcm_sha256())
            .ok_or_else(|| invalid("envelope or independently expected PCM hash"))?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(pcm_len / 2)
            .map_err(|_| invalid("PCM allocation"))?;
        samples.extend(
            bytes[pcm_start..hash_offset]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| i16::from_le_bytes(*pair)),
        );
        Ok(Self {
            identity: expected.clone(),
            samples: samples.into(),
            carrier_sha256,
        })
    }
    pub fn identifier(&self) -> &str {
        self.identity.identifier()
    }
    pub fn channels(&self) -> u8 {
        self.identity.channels()
    }
    pub fn sample_rate(&self) -> u32 {
        self.identity.sample_rate()
    }
    pub fn frames(&self) -> u32 {
        self.identity.frames()
    }
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
    pub fn shared_samples(&self) -> Arc<[i16]> {
        Arc::clone(&self.samples)
    }
    pub fn carrier_sha256(&self) -> [u8; 32] {
        self.carrier_sha256
    }
    pub fn mode(&self) -> AudioPcmMode {
        AudioPcmMode::FinitePredecodedNoLoop
    }
}

/// Emit exactly one sample. Requires the independently expected PCM hash;
/// arbitrary bytes plus a newly computed self-declared hash are not sufficient.
pub fn encode_audio_pcm(
    expected: &AudioPcmExpectedIdentity,
    samples: &[i16],
) -> Result<Vec<u8>, AudioPcmError> {
    let pcm_len = expected.pcm_bytes()?;
    if samples.len().checked_mul(2) != Some(pcm_len) {
        return Err(invalid("PCM sample count"));
    }
    let mut digest = Sha256::new();
    for sample in samples {
        digest.update(sample.to_le_bytes());
    }
    if digest.finalize()[..] != expected.pcm_sha256() {
        return Err(invalid("expected PCM content"));
    }
    let length =
        HEADER_BYTES + expected.identifier().len() + expected.source_path().len() + pcm_len + 32;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| invalid("carrier allocation"))?;
    bytes.extend(MAGIC);
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(u16::from(expected.channels()).to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    for value in [
        expected.sample_rate(),
        expected.frames(),
        expected.source_bytes(),
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend((expected.identifier().len() as u16).to_le_bytes());
    bytes.extend((expected.source_path().len() as u16).to_le_bytes());
    bytes.extend((pcm_len as u32).to_le_bytes());
    for hash in expected.hashes {
        bytes.extend(hash);
    }
    bytes.extend(expected.identifier().as_bytes());
    bytes.extend(expected.source_path().as_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
    }
    let hash = Sha256::digest(&bytes);
    bytes.extend_from_slice(&hash);
    Ok(bytes)
}

#[cfg(test)]
mod tests;
