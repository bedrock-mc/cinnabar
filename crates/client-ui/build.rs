//! Embeds original OreUI art using the manifest as the only key-to-file mapping.

use std::{collections::HashSet, env, fmt::Write, fs, path::PathBuf};

/// Validates the art manifest and emits immutable descriptors and embedded image bytes.
fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/oreui");
    let manifest = root.join("manifest.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let value: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 1);
    let mut output = String::new();
    let mut names = Vec::new();
    let mut keys = HashSet::new();
    let mut files = HashSet::new();
    for entry in value["assets"].as_array().unwrap() {
        let id = entry["id"].as_str().unwrap();
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        );
        let name = id.replace('-', "_").to_ascii_uppercase();
        let key = entry["game_key"].as_str().unwrap();
        let file = entry["file"].as_str().unwrap();
        assert_eq!(std::path::Path::new(file).components().count(), 1);
        assert!(keys.insert(key) && files.insert(file) && !names.contains(&name));
        let path = root.join(file).canonicalize().unwrap();
        println!("cargo:rerun-if-changed={}", path.display());
        let frames = entry["frames"].as_u64().unwrap();
        let size = entry["size"].as_array().unwrap();
        let width = size[0].as_u64().unwrap();
        let height = size[1].as_u64().unwrap();
        let steps = entry["playback"]["steps"].as_u64().unwrap_or(1);
        let duration = entry["playback"]["duration_ms"].as_u64().unwrap_or(0);
        writeln!(output, "pub(crate) const {name}: EmbeddedImage = EmbeddedImage {{ key: {key:?}, bytes: include_bytes!({path:?}), size: [{width}, {height}], frames: {frames}, steps: {steps}, duration_ms: {duration} }};").unwrap();
        names.push(name);
    }
    writeln!(
        output,
        "pub(super) const ALL: &[EmbeddedImage] = &[{}];",
        names.join(",")
    )
    .unwrap();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("oreui_art.rs"),
        output,
    )
    .unwrap();
}
