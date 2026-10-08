use std::path::PathBuf;

/// Load every built-in HUD JSON file, including shared globals used by its controls.
pub fn files() -> Vec<(String, Vec<u8>)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/java-hud");
    let mut files: Vec<_> = std::fs::read_dir(root.join("ui"))
        .expect("built-in HUD directory")
        .map(|entry| entry.expect("built-in HUD entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            let key = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (key, std::fs::read(path).expect("built-in HUD file"))
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}
