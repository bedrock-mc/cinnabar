//! Names-only derivatives of the pinned sample archive.

use std::{collections::BTreeSet, error::Error, fs, io::Read, path::Path};

use pack_compile_input::json::parse_semantic_json;
use serde_json::Value;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

/// Generates sorted vocabulary rows and actor names from an unpacked sample archive.
pub fn generate(samples: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut rows = BTreeSet::new();
    let mut actors = BTreeSet::new();
    let metadata = samples.join("metadata/vanilladata_modules");
    for (file, domain) in [
        ("mojang-blocks.json", "block"),
        ("mojang-items.json", "item"),
        ("mojang-entities.json", "actor"),
    ] {
        let document = read(&metadata.join(file))?;
        for entry in array(&document, "data_items")? {
            let name = string(entry, "name")?;
            rows.insert((domain.to_owned(), name.to_owned()));
            if domain == "actor" {
                actors.insert(name.to_owned());
            }
        }
        if domain == "block" {
            for entry in array(&document, "block_properties")? {
                rows.insert(("property".to_owned(), string(entry, "name")?.to_owned()));
            }
        }
    }
    for (directory, root, domain) in [
        ("behavior_pack/items", "minecraft:item", "behavior_item"),
        (
            "behavior_pack/entities",
            "minecraft:entity",
            "behavior_actor",
        ),
        (
            "resource_pack/entity",
            "minecraft:client_entity",
            "pack_actor",
        ),
        (
            "resource_pack/attachables",
            "minecraft:attachable",
            "attachable",
        ),
        ("resource_pack/particles", "particle_effect", "particle"),
    ] {
        for path in json_files(&samples.join(directory))? {
            let document = read(&path)?;
            let name = document
                .get(root)
                .and_then(|value| value.get("description"))
                .and_then(|value| value.get("identifier"))
                .and_then(Value::as_str)
                .ok_or_else(|| format!("missing {root} identifier in {}", path.display()))?;
            rows.insert((domain.to_owned(), name.to_owned()));
            if domain == "pack_actor"
                && let Some(materials) = document[root]["description"]["materials"].as_object()
            {
                for material in materials.values() {
                    if let Some(name) = material.as_str() {
                        rows.insert(("material".to_owned(), name.to_owned()));
                    }
                }
            }
        }
    }
    for (file, domain) in [
        ("terrain_texture.json", "terrain_texture"),
        ("item_texture.json", "item_texture"),
    ] {
        let document = read(&samples.join("resource_pack/textures").join(file))?;
        let entries = document["texture_data"]
            .as_object()
            .ok_or("missing texture_data")?;
        rows.extend(entries.keys().map(|name| (domain.to_owned(), name.clone())));
    }
    let vocabulary = rows
        .into_iter()
        .map(|(domain, name)| format!("{domain}\t{name}\n"))
        .collect::<String>()
        .into_bytes();
    let actors = actors
        .into_iter()
        .map(|name| format!("{name}\n"))
        .collect::<String>()
        .into_bytes();
    Ok((vocabulary, actors))
}

/// Parses one bounded pack document, including its JSON comments and trailing commas.
pub(crate) fn read(path: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!(
            "vocabulary input exceeds {MAX_FILE_BYTES} bytes: {}",
            path.display()
        )
        .into());
    }
    Ok(parse_semantic_json(path, &bytes)?)
}

/// Requires the metadata array whose entries define the vocabulary.
fn array<'a>(document: &'a Value, key: &str) -> Result<&'a [Value]> {
    document[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing metadata array {key}").into())
}

/// Requires a nonempty identifier field in a metadata entry.
fn string<'a>(document: &'a Value, key: &str) -> Result<&'a str> {
    document[key]
        .as_str()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("missing metadata string {key}").into())
}

/// Collects JSON definitions recursively and rejects links outside the verified archive.
pub(crate) fn json_files(directory: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err("vocabulary inputs must not contain symbolic links".into());
        }
        if kind.is_dir() {
            files.extend(json_files(&entry.path())?);
        } else if entry.path().extension().is_some_and(|ext| ext == "json") {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}
