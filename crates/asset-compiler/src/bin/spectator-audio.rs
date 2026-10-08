//! Export canonical pinned-pack audio into bounded browser-decodable files.
//! Usage: spectator-audio RESOURCE_PACK OUTPUT_DIRECTORY SOURCE_MANIFEST
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error, fs, path::Path};

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    if fs::metadata(path)?.len() > 8 * 1024 * 1024 {
        return Err("audio metadata exceeds limit".into());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn wav(pcm: pack_compiler::DecodedFadpcm) -> Vec<u8> {
    let length = (pcm.samples().len() * 2) as u32;
    let channels = u16::from(pcm.channels());
    let mut bytes = Vec::with_capacity(length as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(length + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&pcm.sample_rate().to_le_bytes());
    bytes.extend_from_slice(&(pcm.sample_rate() * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&length.to_le_bytes());
    for sample in pcm.samples() {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
fn visit(
    root: &Path,
    dir: &Path,
    output: &Path,
    depth: usize,
    files: &mut BTreeMap<String, Value>,
    skipped: &mut Vec<String>,
) -> Result<(), Box<dyn Error>> {
    if depth > 16 {
        return Err("audio tree is too deep".into());
    }
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            visit(root, &path, output, depth + 1, files, skipped)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");
        if !matches!(extension, "fsb" | "ogg") {
            continue;
        }
        let stem = path
            .strip_prefix(root)?
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        if meta.len() > 2 * 1024 * 1024 {
            skipped.push(stem);
            continue;
        }
        let input = fs::read(&path)?;
        let (bytes, extension) = if extension == "fsb" {
            match pack_compiler::decode_fsb5_fadpcm(&input) {
                Ok(pcm) => (wav(pcm), "wav"),
                Err(error) => {
                    skipped.push(format!("{stem}: {error}"));
                    continue;
                }
            }
        } else {
            (input, "ogg")
        };
        if bytes.len() > 4 * 1024 * 1024 {
            skipped.push(stem);
            continue;
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let file = format!("{hash}.{extension}");
        fs::write(output.join(&file), &bytes)?;
        files.insert(stem, json!({"sha256":hash,"file":file,"size":bytes.len()}));
        if files.len() > 8192 {
            return Err("too many audio assets".into());
        }
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 4 {
        return Err("usage: spectator-audio RESOURCE_PACK OUTPUT_DIRECTORY SOURCE_MANIFEST".into());
    }
    let root = Path::new(&args[1]).canonicalize()?;
    let output = Path::new(&args[2]);
    let source_manifest = fs::read(Path::new(&args[3]))?;
    if source_manifest.len() > 64 * 1024
        || assets::canonical_source_manifest_sha256(&source_manifest)
            != assets::vanilla_source_manifest_sha256()
    {
        return Err("source manifest does not match canonical vanilla pin".into());
    }
    let definition_path = root.join("sounds/sound_definitions.json");
    let definitions = read_json(&definition_path)?;
    let definition_bytes = fs::read(&definition_path)?;
    let expected = assets::reviewed_audio_pcm_identity();
    if Sha256::digest(&definition_bytes)[..] != expected.sound_definitions_sha256() {
        return Err("sound definitions do not match canonical vanilla pin".into());
    }
    let sounds = read_json(&root.join("sounds.json"))?;
    let source_blocks = read_json(&root.join("blocks.json"))?;
    let blocks: BTreeMap<_, _> = source_blocks
        .as_object()
        .ok_or("invalid blocks metadata")?
        .iter()
        .filter_map(|(name, value)| {
            value
                .get("sound")
                .and_then(Value::as_str)
                .map(|material| (name, material))
        })
        .collect();
    fs::create_dir_all(output)?;
    let mut files = BTreeMap::new();
    let mut skipped = Vec::new();
    visit(
        &root,
        &root.join("sounds"),
        output,
        0,
        &mut files,
        &mut skipped,
    )?;
    let manifest = serde_json::to_vec(
        &json!({"version":1,"packVersion":assets::vanilla_source().tag.strip_prefix('v').unwrap_or(&assets::vanilla_source().tag),"sourceManifestSHA256":format!("{:x}",Sha256::digest(&source_manifest)),"sounds":sounds,"blocks":blocks,"definitions":definitions,"files":files,"unsupported":skipped}),
    )?;
    if manifest.len() > 8 * 1024 * 1024 {
        return Err("compiled manifest exceeds limit".into());
    }
    fs::write(output.join("manifest.json"), manifest)?;
    eprintln!(
        "exported {} browser sound paths; {} unsupported paths",
        files.len(),
        skipped.len()
    );
    Ok(())
}
