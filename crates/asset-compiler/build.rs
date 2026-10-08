//! Embeds `ASSETC_SOURCE_SHA256`: a digest of every workspace crate the compiler links, every file
//! they embed and the locked external packages they reach, so `assetc prepare` rebuilds carriers
//! exactly when compiler code or its dependencies change.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

#[path = "build_support/lockfile.rs"]
mod lockfile;

/// Crate directories that never reach the binary.
const SKIPPED_DIRS: &[&str] = &["tests", "benches", "target"];

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut crates = BTreeSet::new();
    collect_crates(&manifest_dir, &mut crates);
    let mut files = BTreeSet::new();
    for dir in &crates {
        println!("cargo:rerun-if-changed={}", dir.display());
        collect_files(dir, dir, &mut files);
    }
    let embedded: BTreeSet<PathBuf> = files
        .iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .flat_map(|path| embedded_files(path))
        .filter(|path| !crates.iter().any(|dir| path.starts_with(dir)))
        .collect();
    for path in &embedded {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let root = manifest_dir.join("../..").canonicalize().unwrap();
    let mut hasher = Sha256::new();
    for path in files.iter().chain(&embedded) {
        let relative = path.strip_prefix(&root).unwrap_or(path);
        hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update([0]);
        hasher.update(fs::read(path).unwrap_or_default());
        hasher.update([0]);
    }
    let lock = root.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let locked = fs::read_to_string(&lock).unwrap_or_default();
    let package = env::var("CARGO_PKG_NAME").unwrap();
    for line in lockfile::external_closure(&locked, &package) {
        hasher.update(line.as_bytes());
        hasher.update([0]);
    }
    println!(
        "cargo:rustc-env=ASSETC_SOURCE_SHA256={:x}",
        hasher.finalize()
    );
}

/// `dir` and every workspace crate its normal dependencies reach through `path = "..."`.
fn collect_crates(dir: &Path, crates: &mut BTreeSet<PathBuf>) {
    let dir = dir.canonicalize().unwrap();
    if !crates.insert(dir.clone()) {
        return;
    }
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    let mut normal = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            normal = line == "[dependencies]"
                || (line.starts_with("[target.") && line.ends_with(".dependencies]"));
            continue;
        }
        if let Some(path) = normal.then(|| quoted_after(line, "path")).flatten() {
            collect_crates(&dir.join(path), crates);
        }
    }
}

fn collect_files(root: &Path, dir: &Path, files: &mut BTreeSet<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            let top_level_skip = dir == root
                && SKIPPED_DIRS
                    .iter()
                    .any(|skip| name == std::ffi::OsStr::new(skip));
            if !top_level_skip {
                collect_files(root, &path, files);
            }
        } else {
            files.insert(path);
        }
    }
}

/// Files a source embeds through `include_bytes!`/`include_str!` string literals.
fn embedded_files(source: &Path) -> Vec<PathBuf> {
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

fn quoted_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(&format!("{key} = \""))? + key.len() + 4;
    line[at..].split('"').next()
}
