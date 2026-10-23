//! One hash-indexed installation archive containing the existing runtime artifacts.

use anyhow::{Context, Result, ensure};
use experience_sdk::artifact::{ASSETS_DIR, DATA_SCHEMA, MANIFEST_FILE, SERVER_WASM};
use experience_sdk::project::Project;
use serde::{Deserialize, Serialize};
use server_experience::{bundle::offline, crypto, policy::MAX_EXPANDED_BYTES};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const INDEX: &str = "package.json";
const PACK: &str = "resource.mcpack";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    format: u32,
    id: String,
    files: BTreeMap<String, String>,
}

/// Writes a verified server artifact, builds its ordinary pack and atomically replaces the archive.
pub(super) fn assemble(
    root: &Path,
    project: &Project,
    version: &str,
    server: &[u8],
    assets: &[(String, Vec<u8>)],
    client: &[u8],
    generated: &Path,
) -> Result<PathBuf> {
    let scratch = tempfile::tempdir_in(root)?;
    let artifact = scratch.path().join("server");
    fs::create_dir(&artifact)?;
    let mut files = BTreeMap::from([(SERVER_WASM.to_owned(), server.to_vec())]);
    for (path, data) in assets
        .iter()
        .filter(|(path, _)| !path.starts_with("client/"))
    {
        files.insert(format!("{ASSETS_DIR}/{path}"), data.clone());
    }
    let hashes: BTreeMap<_, _> = files
        .iter()
        .map(|(path, data)| (path.clone(), crypto::digest(data)))
        .collect();
    let manifest = toml::to_string(
        &serde_json::json!({"id": project.id, "version": version, "api": experience_sdk::project::server_api(), "data-schema": DATA_SCHEMA, "files": hashes}),
    )?;
    files.insert(MANIFEST_FILE.into(), manifest.into_bytes());
    for (path, data) in &files {
        let dest = artifact.join(path);
        fs::create_dir_all(dest.parent().context("artifact parent missing")?)?;
        fs::write(dest, data)?;
    }
    let pack = scratch.path().join(PACK);
    super::local::export_pack(&artifact, &pack)?;
    let mut entries: BTreeMap<String, Vec<u8>> = files
        .into_iter()
        .map(|(path, data)| (format!("server/{path}"), data))
        .collect();
    entries.insert(format!("client/{}.cxb", project.id), client.to_vec());
    entries.insert(PACK.into(), fs::read(pack)?);
    for file in ["registry.json", "messages.go"] {
        let path = if file == "messages.go" {
            "messages/messages.go"
        } else {
            file
        };
        entries.insert(path.into(), fs::read(generated.join(file))?);
    }
    entries.insert(
        "messages/go.mod".into(),
        experience_sdk::project::go_module(&project.id).into_bytes(),
    );
    let index = Index {
        format: 1,
        id: project.id.clone(),
        files: entries
            .iter()
            .map(|(path, data)| (path.clone(), crypto::digest(data)))
            .collect(),
    };
    entries.insert(INDEX.into(), serde_json::to_vec_pretty(&index)?);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in entries {
        zip.start_file(
            path,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )?;
        zip.write_all(&data)?;
    }
    let bytes = zip.finish()?.into_inner();
    verify(&bytes)?;
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;
    let destination = dist.join(format!("{}.cmod", project.id));
    let mut staged = tempfile::NamedTempFile::new_in(&dist)?;
    staged.write_all(&bytes)?;
    staged.persist(&destination)?;
    writeln!(std::io::stdout(), "package={}", destination.display())?;
    Ok(destination)
}

/// Checks the complete inventory before any archive path reaches the filesystem.
fn verify(bytes: &[u8]) -> Result<(Index, BTreeMap<String, Vec<u8>>)> {
    ensure!(
        bytes.len() as u64 <= MAX_EXPANDED_BYTES,
        "mod package is too large"
    );
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut entries = BTreeMap::new();
    let mut remaining = MAX_EXPANDED_BYTES;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let path = file.name().to_owned();
        ensure!(
            path.split('/').all(|part| !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(['\\', ':'])),
            "invalid mod package path {path}"
        );
        ensure!(
            file.is_file()
                && file
                    .unix_mode()
                    .is_none_or(|mode| mode & 0o170000 == 0 || mode & 0o170000 == 0o100000),
            "mod entries must be regular files"
        );
        ensure!(
            file.size() <= remaining,
            "mod package expands beyond its budget"
        );
        let mut data = Vec::new();
        (&mut file).take(remaining + 1).read_to_end(&mut data)?;
        ensure!(
            data.len() as u64 <= remaining,
            "mod package expands beyond its budget"
        );
        remaining -= data.len() as u64;
        ensure!(
            entries.insert(path, data).is_none(),
            "duplicate package path"
        );
    }
    let index: Index = serde_json::from_slice(entries.get(INDEX).context("missing package.json")?)?;
    ensure!(
        index.format == 1 && experience_sdk::project::valid_name(&index.id),
        "unsupported mod package or invalid ID"
    );
    ensure!(
        index.files.len() + 1 == entries.len() && !index.files.contains_key(INDEX),
        "mod inventory differs from its index"
    );
    for (path, hash) in &index.files {
        ensure!(
            entries
                .get(path)
                .is_some_and(|data| crypto::digest(data) == *hash),
            "mod hash mismatch for {path}"
        );
    }
    ensure!(
        entries.contains_key(&format!("server/{SERVER_WASM}")) && entries.contains_key(PACK),
        "mod package needs server WASM and a resource pack"
    );
    let client = offline::verify(
        entries
            .get(&format!("client/{}.cxb", index.id))
            .context("missing client bundle")?,
    )?;
    ensure!(
        client.manifest.id == index.id,
        "client identity differs from mod identity"
    );
    ensure!(
        client
            .manifest
            .files
            .iter()
            .all(|file| !file.path.starts_with("server/")),
        "client bundle contains server-only assets"
    );
    let server: toml::Value = toml::from_str(std::str::from_utf8(
        entries
            .get(&format!("server/{MANIFEST_FILE}"))
            .context("missing server manifest")?,
    )?)?;
    ensure!(
        server.get("id").and_then(toml::Value::as_str) == Some(index.id.as_str()),
        "server identity differs from mod identity"
    );
    let inventory = server
        .get("files")
        .and_then(toml::Value::as_table)
        .context("server file index missing")?;
    let server_files = entries
        .iter()
        .filter_map(|(path, data)| path.strip_prefix("server/").map(|path| (path, data)))
        .filter(|(path, _)| *path != MANIFEST_FILE)
        .collect::<BTreeMap<_, _>>();
    ensure!(
        inventory.len() == server_files.len(),
        "server inventory differs from artifact"
    );
    for (path, hash) in inventory {
        ensure!(
            server_files
                .get(path.as_str())
                .is_some_and(|data| hash.as_str() == Some(crypto::digest(data).as_str())),
            "server hash mismatch for {path}"
        );
    }
    // A file cannot also be an ancestor of another file.
    for path in entries.keys() {
        let mut parent = Path::new(path).parent();
        while let Some(p) = parent {
            ensure!(
                !p.to_str().is_some_and(|p| entries.contains_key(p)),
                "conflicting package paths"
            );
            parent = p.parent();
        }
    }
    Ok((index, entries))
}

/// Bounds the on-disk archive before reading or expanding it.
fn read_package(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "mod package must be a regular file"
    );
    let mut bytes = Vec::new();
    file.take(MAX_EXPANDED_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_EXPANDED_BYTES,
        "mod package is too large"
    );
    Ok(bytes)
}

/// Verifies and expands a package into a new directory; existing installations are never removed.
pub fn unpack(package: &Path, destination: &Path) -> Result<String> {
    let (index, entries) = verify(&read_package(package)?)?;
    extract(index, entries, destination)
}

/// Publishes a previously checked archive snapshot into a new directory.
fn extract(index: Index, entries: BTreeMap<String, Vec<u8>>, destination: &Path) -> Result<String> {
    ensure!(
        !destination.exists(),
        "{} already exists",
        destination.display()
    );
    fs::create_dir_all(destination.parent().context("destination parent missing")?)?;
    let staged = tempfile::tempdir_in(destination.parent().context("destination parent missing")?)?;
    for (path, bytes) in entries {
        let path = staged.path().join(path);
        fs::create_dir_all(path.parent().context("entry parent missing")?)?;
        fs::write(path, bytes)?;
    }
    fs::rename(staged.path(), destination)?;
    Ok(index.id)
}

/// Installs one verified mod into a server world, using its stable package ID as the directory.
pub fn install(package: &Path, world: &Path) -> Result<PathBuf> {
    install_at(package, world, false)
}

/// Replaces only this project's installation after the replacement has been completely staged.
pub(super) fn replace(package: &Path, world: &Path) -> Result<PathBuf> {
    install_at(package, world, true)
}

/// Stages an installation before moving the previous directory, restoring it if publication fails.
fn install_at(package: &Path, world: &Path, replace: bool) -> Result<PathBuf> {
    let (index, entries) = verify(&read_package(package)?)?;
    let destination = world.join("mods").join(&index.id);
    if !replace || !destination.exists() {
        extract(index, entries, &destination)?;
        return Ok(destination);
    }
    let staged = tempfile::tempdir_in(destination.parent().context("mod directory missing")?)?;
    let next = staged.path().join("next");
    extract(index, entries, &next)?;
    let previous = staged.path().join("previous");
    fs::rename(&destination, &previous)?;
    if let Err(error) = fs::rename(next, &destination) {
        fs::rename(previous, &destination).context("restoring the previous mod installation")?;
        return Err(error.into());
    }
    Ok(destination)
}

#[cfg(test)]
mod tests;
