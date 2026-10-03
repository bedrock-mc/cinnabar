use std::path::PathBuf;

/// Finds a configured local fixture and explains why its test cannot run when absent.
pub fn env_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os(name).map(PathBuf::from).or_else(|| {
        if name != "PINNED_VANILLA_PACK" {
            return None;
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        Some(root.join(assets::vanilla_source().resource_pack_dir()))
    });
    let Some(path) = path else {
        eprintln!("skipping local fixture test: {name} is not set");
        return None;
    };
    if !path.exists() {
        eprintln!(
            "skipping local fixture test: {name} points at missing {}",
            path.display()
        );
        return None;
    }
    Some(path)
}
