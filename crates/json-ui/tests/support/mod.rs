use std::path::PathBuf;

/// The extracted pinned vanilla `resource_pack`, located by `assets/vanilla-source.json`.
pub fn vanilla_pack() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("assets/vanilla-source.json")).unwrap())
            .unwrap();
    let pack = root
        .join(manifest["cache_dir"].as_str().unwrap())
        .join("resource_pack");
    if !pack.join("ui").is_dir() {
        eprintln!(
            "skipping local vanilla UI fixture test: missing {}; fetch vanilla-assets",
            pack.join("ui").display()
        );
    }
    pack
}
