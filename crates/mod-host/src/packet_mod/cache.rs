//! Compiled packet mods kept on disk, so a large guest compiles once per version.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::Path,
};
use wasmtime::{Engine, Module, component::Component};

/// File-name prefix of every cached artifact; older versions are pruned on each store.
const PREFIX: &str = "packet-mod-";
const EXTENSION: &str = "cwasm";

/// Compiled code that wasmtime can serialize for the same engine settings.
pub(super) trait Artifact: Sized {
    fn compile(engine: &Engine, bytes: &[u8]) -> Result<Self>;
    /// # Safety
    /// `path` must hold an artifact this host serialized.
    unsafe fn deserialize_file(engine: &Engine, path: &Path) -> Result<Self>;
    fn serialize(&self) -> Result<Vec<u8>>;
}

impl Artifact for Component {
    fn compile(engine: &Engine, bytes: &[u8]) -> Result<Self> {
        Component::new(engine, bytes)
    }
    unsafe fn deserialize_file(engine: &Engine, path: &Path) -> Result<Self> {
        // SAFETY: forwarded from the caller's contract.
        unsafe { Component::deserialize_file(engine, path) }
    }
    fn serialize(&self) -> Result<Vec<u8>> {
        Component::serialize(self)
    }
}

impl Artifact for Module {
    fn compile(engine: &Engine, bytes: &[u8]) -> Result<Self> {
        Module::new(engine, bytes)
    }
    unsafe fn deserialize_file(engine: &Engine, path: &Path) -> Result<Self> {
        // SAFETY: forwarded from the caller's contract.
        unsafe { Module::deserialize_file(engine, path) }
    }
    fn serialize(&self) -> Result<Vec<u8>> {
        Module::serialize(self)
    }
}

/// Compiles `bytes`, reusing the artifact in `dir` when present. A missing, stale or unreadable
/// artifact falls back to compiling, and a failed store only logs.
pub(super) fn load<A: Artifact>(engine: &Engine, bytes: &[u8], dir: Option<&Path>) -> Result<A> {
    let Some(dir) = dir else {
        return A::compile(engine, bytes).context("compile packet mod");
    };
    let path = dir.join(file_name(engine, bytes));
    if path.is_file() {
        // SAFETY: the cache directory belongs to this user and holds only artifacts this host
        // serialized from the same bytes and engine settings; wasmtime still refuses artifacts
        // whose header does not match the engine.
        match unsafe { A::deserialize_file(engine, &path) } {
            Ok(artifact) => return Ok(artifact),
            Err(error) => tracing::info!(error = format!("{error:#}"), "recompiling packet mod"),
        }
    }
    let artifact = A::compile(engine, bytes).context("compile packet mod")?;
    if let Err(error) = artifact
        .serialize()
        .and_then(|bytes| store(dir, &path, &bytes))
    {
        tracing::warn!(
            error = format!("{error:#}"),
            "packet mod compile cache unavailable"
        );
    }
    Ok(artifact)
}

/// Names an artifact by the component bytes and the engine's compatibility settings.
fn file_name(engine: &Engine, bytes: &[u8]) -> String {
    let mut settings = DefaultHasher::new();
    engine.precompile_compatibility_hash().hash(&mut settings);
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.update(settings.finish().to_le_bytes());
    format!("{PREFIX}{:x}.{EXTENSION}", digest.finalize())
}

/// Writes the artifact atomically and removes artifacts of other versions.
fn store(dir: &Path, path: &Path, serialized: &[u8]) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    std::fs::write(&partial, serialized)?;
    std::fs::rename(&partial, path)?;
    for entry in std::fs::read_dir(dir)?.flatten() {
        let stale = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if stale != path && name.starts_with(PREFIX) && name.ends_with(EXTENSION) {
            let _ = std::fs::remove_file(stale);
        }
    }
    Ok(())
}
