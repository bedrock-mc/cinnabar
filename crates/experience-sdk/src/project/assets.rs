//! A deterministic, compile-checked index of package files.

use std::{
    collections::BTreeSet,
    fmt::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

/// Reads regular package files, rejecting symlinks and ambiguous generated constant names.
pub fn scan(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    ensure!(
        std::fs::symlink_metadata(root)?.is_dir(),
        "assets must be a regular directory"
    );
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut names = BTreeSet::new();
    while let Some(dir) = pending.pop() {
        println!("cargo:rerun-if-changed={}", dir.display());
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("reading assets directory {}", dir.display()))?
        {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
                continue;
            }
            ensure!(
                kind.is_file(),
                "{}: assets must be regular files",
                entry.path().display()
            );
            let path = entry.path();
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .context("asset path is not UTF-8")?
                .replace('\\', "/");
            ensure!(
                relative.split('/').all(|part| !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part.bytes().all(|b| b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || b"._-".contains(&b))),
                "{relative}: asset paths must be portable lowercase names"
            );
            ensure!(
                names.insert(constant(&relative)),
                "{relative}: asset constant name collision"
            );
            files.push((relative, path.canonicalize()?));
        }
    }
    files.sort();
    Ok(files)
}

/// Emits one typed asset constant per file, with an `include_bytes!` existence check.
pub fn generate(files: &[(String, PathBuf)]) -> Result<String> {
    let mut out = String::from(
        "/// Compile-checked package files, named from their complete relative path.\npub mod assets {\n",
    );
    for (path, file) in files {
        writeln!(
            out,
            "/// Package asset `{path}`.\npub const {}: ::experience_sdk::message::Asset = ::experience_sdk::message::Asset {{ path: {path:?}, bytes: ::core::include_bytes!({:?}) }};",
            constant(path),
            file.to_str().context("asset path is not UTF-8")?
        )?;
    }
    out.push_str("}\n");
    Ok(out)
}

/// Uses the whole path so two directories may contain files with the same basename.
fn constant(path: &str) -> String {
    let name: String = path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("ASSET_{name}")
}
