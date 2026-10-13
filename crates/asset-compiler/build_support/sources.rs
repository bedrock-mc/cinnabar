use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// Directories excluded at each package root.
const SKIPPED_DIRS: &[&str] = &["tests", "benches", "target"];

/// Collects one package without entering nested packages or root test directories.
pub(crate) fn collect_files(root: &Path, dir: &Path, files: &mut BTreeSet<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            let top_level_skip = dir == root
                && SKIPPED_DIRS
                    .iter()
                    .any(|skip| name == std::ffi::OsStr::new(skip));
            if !top_level_skip && !path.join("Cargo.toml").is_file() {
                collect_files(root, &path, files);
            }
        } else {
            files.insert(path);
        }
    }
}

/// Files a source embeds through `include_bytes!`/`include_str!` string literals.
pub(crate) fn embedded_files(source: &Path) -> Vec<PathBuf> {
    let text = fs::read_to_string(source).unwrap_or_default();
    let base = source.parent().unwrap();
    let mut found = Vec::new();
    for macro_name in ["include_bytes!(", "include_str!("] {
        let mut rest = text.as_str();
        while let Some(start) = rest.find(macro_name) {
            rest = &rest[start + macro_name.len()..];
            let trimmed = rest.trim_start();
            if let Some(literal) = trimmed.strip_prefix('"').and_then(|s| s.split('"').next())
                && let Ok(path) = base.join(literal).canonicalize()
            {
                found.push(path);
            }
        }
    }
    found
}
