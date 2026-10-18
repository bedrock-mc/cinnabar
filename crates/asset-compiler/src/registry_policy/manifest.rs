use super::{Result, digest};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BlockManifest {
    pub schema: String,
    pub game_version: String,
    pub protocol: u32,
    pub source: Source,
    pub allowlist: Allowlist,
    pub projection: Projection,
    pub output: Output,
    pub light: Output,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Source {
    pub module: String,
    pub source_lock_sha256: String,
    pub cloudburst_ref: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Allowlist {
    pub path: String,
    pub sha256: String,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Projection {
    pub states: usize,
    pub legacy_exact_states: usize,
    #[serde(default)]
    pub authored_states: usize,
    pub legacy_reduced_states: usize,
    pub twin_states: usize,
    pub twin_names: usize,
    pub twin_fingerprint: String,
    pub unresolved_retail: Vec<String>,
    pub denied_count: usize,
    pub denied_fingerprint: String,
    pub education_states: usize,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Output {
    pub format: String,
    pub path: String,
    pub sha256: String,
}

/// Writes a completed artifact after creating only its own parent directory.
pub(super) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Writes compactly ordered JSON with the historical indentation and final newline.
pub(super) fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write(path, &bytes)
}

/// Writes a carrier and the checksum sidecar used by its existing callers.
pub(super) fn write_blob(path: &Path, bytes: &[u8], filename: bool) -> Result<()> {
    write(path, bytes)?;
    let sha = if filename {
        format!(
            "{}  {}\n",
            digest(bytes),
            path.file_name()
                .ok_or("carrier filename missing")?
                .to_string_lossy()
        )
    } else {
        format!("{}\n", digest(bytes))
    };
    let sidecar = if filename {
        path.with_file_name(format!(
            "{}.sha256",
            path.file_name()
                .ok_or("carrier filename missing")?
                .to_string_lossy()
        ))
    } else {
        path.with_extension("sha256")
    };
    write(&sidecar, sha.as_bytes())
}
