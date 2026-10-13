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

#[path = "build_support/sources.rs"]
mod sources;
use sources::{collect_files, embedded_files};

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
        .filter(|path| !files.contains(path))
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

/// Reads one quoted manifest value after its key.
fn quoted_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(&format!("{key} = \""))? + key.len() + 4;
    line[at..].split('"').next()
}
