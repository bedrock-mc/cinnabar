//! Packs sound-event routing JSON and every `.fsb` and `.ogg` under `sounds/` into an MCBESND1 bank.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

const MAX_BANK_FILE_BYTES: u64 = assets::MAX_FSB_INPUT_BYTES as u64;
const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_WALK_DEPTH: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum AudioBankCompileError {
    #[error(transparent)]
    Bank(#[from] assets::SoundBankError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid sound source: {0}")]
    Invalid(&'static str),
}

#[derive(Debug)]
pub struct CompiledAudioBank {
    pub bytes: Vec<u8>,
    pub report: AudioBankCompileReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AudioBankCompileReport {
    pub schema: u32,
    pub files: usize,
    pub skipped_files: usize,
    pub data_bytes: u64,
    pub sounds_json_sha256: Box<str>,
    pub prefix_sha256: Box<str>,
    pub carrier_sha256: Box<str>,
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> AudioBankCompileError + '_ {
    move |source| AudioBankCompileError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn read_json(path: &Path, required: bool) -> Result<Vec<u8>, AudioBankCompileError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(source) if !required && source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(b"{}".to_vec());
        }
        Err(source) => return Err(io_error(path)(source)),
    };
    if !meta.is_file() || meta.len() > MAX_JSON_BYTES {
        return Err(AudioBankCompileError::Invalid(
            "JSON source is not a bounded file",
        ));
    }
    fs::read(path).map_err(io_error(path))
}

/// Maps each `blocks.json` block to its sound material, dropping everything else.
fn block_materials(blocks: &[u8]) -> Result<Vec<u8>, AudioBankCompileError> {
    let root: Value = serde_json::from_slice(blocks)?;
    let mut map = Map::new();
    for (name, value) in root.as_object().into_iter().flatten() {
        if let Some(material) = value.get("sound").and_then(Value::as_str) {
            map.insert(name.clone(), Value::String(material.to_owned()));
        }
    }
    Ok(serde_json::to_vec(&Value::Object(map))?)
}

fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    files: &mut Vec<(String, Vec<u8>)>,
    skipped: &mut usize,
) -> Result<(), AudioBankCompileError> {
    if depth > MAX_WALK_DEPTH {
        return Err(AudioBankCompileError::Invalid("sounds tree is too deep"));
    }
    let mut children: Vec<fs::DirEntry> = fs::read_dir(dir)
        .map_err(io_error(dir))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(io_error(dir))?;
    children.sort_by_key(|child| child.file_name());
    for child in children {
        let path = child.path();
        let meta = fs::symlink_metadata(&path).map_err(io_error(&path))?;
        if meta.is_dir() {
            collect(root, &path, depth + 1, files, skipped)?;
            continue;
        }
        let stem = path
            .strip_prefix(root)
            .ok()
            .and_then(|relative| relative.with_extension("").to_str().map(str::to_owned))
            .map(|text| text.replace('\\', "/"));
        let is_fsb = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fsb"));
        match stem {
            Some(stem) if meta.is_file() && is_fsb && meta.len() <= MAX_BANK_FILE_BYTES => {
                files.push((stem, fs::read(&path).map_err(io_error(&path))?));
            }
            _ if is_fsb => *skipped += 1,
            _ => {}
        }
    }
    Ok(())
}

pub fn compile_audio_bank(pack: &Path) -> Result<CompiledAudioBank, AudioBankCompileError> {
    let root = pack.canonicalize().map_err(io_error(pack))?;
    let sounds = read_json(&root.join("sounds.json"), true)?;
    let blocks = read_json(&root.join("blocks.json"), false)?;
    let music = read_json(&root.join("sounds/music_definitions.json"), false)?;
    serde_json::from_slice::<Value>(&sounds)?;
    serde_json::from_slice::<Value>(&music)?;
    let materials = block_materials(&blocks)?;
    let mut files = Vec::new();
    let mut skipped = 0;
    let sounds_dir = root.join("sounds");
    if sounds_dir.is_dir() {
        collect(&root, &sounds_dir, 0, &mut files, &mut skipped)?;
    }
    let bytes = assets::encode_sound_bank(&sounds, &materials, &music, &files)?;
    let prefix_len = assets::sound_bank_prefix_len(&bytes)?;
    let index = assets::SoundBankIndex::decode_prefix(&bytes[..prefix_len])?;
    let hex = |hash: [u8; 32]| -> Box<str> {
        hash.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            .into()
    };
    let report = AudioBankCompileReport {
        schema: 1,
        files: index.len(),
        skipped_files: skipped,
        data_bytes: (bytes.len() - prefix_len) as u64,
        sounds_json_sha256: hex(Sha256::digest(&sounds).into()),
        prefix_sha256: hex(index.prefix_sha256()),
        carrier_sha256: hex(Sha256::digest(&bytes).into()),
    };
    Ok(CompiledAudioBank { bytes, report })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_fsb_files_and_block_materials() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("sounds/random")).unwrap();
        fs::write(root.join("sounds.json"), r#"{"block_sounds":{}}"#).unwrap();
        fs::write(
            root.join("blocks.json"),
            r#"{"stone":{"sound":"stone"},"air":{}}"#,
        )
        .unwrap();
        fs::write(root.join("sounds/random/click.fsb"), [1, 2, 3]).unwrap();
        fs::write(root.join("sounds/random/note.txt"), "x").unwrap();
        let compiled = compile_audio_bank(root).expect("compile");
        assert_eq!(compiled.report.files, 1);
        let prefix = assets::sound_bank_prefix_len(&compiled.bytes).unwrap();
        let index = assets::SoundBankIndex::decode_prefix(&compiled.bytes[..prefix]).unwrap();
        assert!(index.entry("sounds/random/click").is_some());
        let materials: Value = serde_json::from_slice(index.materials_json()).unwrap();
        assert_eq!(materials["stone"], "stone");
        assert!(materials.get("air").is_none());
    }
}
