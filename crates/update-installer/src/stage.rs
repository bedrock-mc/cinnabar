//! Resumable files are published only after their signed size and digest match.

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use time::OffsetDateTime;
use update_manifest::{Artifact, Manifest, TrustedKeys, select::Check};

/// A signed size cannot allocate unbounded disk space.
pub const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const ENVELOPE_FILE: &str = "manifest.json";
pub const ARTIFACT_FILE: &str = "artifact";
const STAGE_PREFIX: &str = "stage-";
const RETIRED_PREFIX: &str = "retired-";
const RETIRED_STAGE: &str = "stage";

/// Atomically replaces an envelope only when its authenticated payload already exists unchanged.
pub fn refresh_envelope(
    directory: &Path,
    lease: &crate::lease::Lease,
    selected: &Selection,
) -> Result<bool> {
    ensure!(
        directory.parent() == Some(lease.root()),
        "stage is outside the leased directory"
    );
    if verify_file(&directory.join(ARTIFACT_FILE), &selected.artifact).is_err() {
        return Ok(false);
    }
    let mut envelope = tempfile::NamedTempFile::new_in(directory)?;
    envelope.write_all(&selected.envelope)?;
    envelope.persist(directory.join(ENVELOPE_FILE))?;
    Ok(true)
}

/// Retains a verified offer independently of later cache promotion or manual desktop handoff.
pub fn retain(
    source: &Path,
    lease: &crate::lease::Lease,
    selected: &Selection,
) -> Result<(PathBuf, File)> {
    ensure!(
        source.parent() == Some(lease.root()),
        "stage is outside the leased directory"
    );
    let (directory, reader) = crate::helper::snapshot_directory(lease.root())?;
    fs::write(directory.path().join(ENVELOPE_FILE), &selected.envelope)?;
    let artifact = directory.path().join(ARTIFACT_FILE);
    // Immutable payload links survive cache replacement without copying gigabytes.
    if fs::hard_link(source.join(ARTIFACT_FILE), &artifact).is_err() {
        fs::copy(source.join(ARTIFACT_FILE), &artifact)?;
    }
    verify_file(&artifact, &selected.artifact)?;
    Ok((directory.keep(), reader))
}

/// The signed release and its selected native artifact.
pub struct Selection {
    pub manifest: Manifest,
    pub artifact: Artifact,
    envelope: Vec<u8>,
}

impl Selection {
    /// Verifies expiry, channel, platform and build ordering before selecting any download.
    pub fn verify(
        envelope: &[u8],
        keys: &TrustedKeys,
        check: Check<'_>,
        now: OffsetDateTime,
    ) -> Result<Self> {
        ensure!(
            envelope.len() <= update_manifest::MAX_ENVELOPE_BYTES,
            "manifest exceeds size limit"
        );
        let manifest = update_manifest::verify(envelope, keys, now)?;
        ensure!(
            manifest.build_number > 0,
            "release has no publication number"
        );
        let verdict = update_manifest::select::evaluate_build(&manifest, check)?;
        let artifact = verdict
            .artifact
            .context("no permitted artifact for this channel and platform")?;
        ensure!(
            artifact.size as u64 <= MAX_ARTIFACT_BYTES,
            "artifact exceeds size limit"
        );
        Ok(Self {
            manifest,
            artifact,
            envelope: envelope.to_vec(),
        })
    }
}

/// A persistent partial download identified by the signed SHA-256 digest.
pub struct Download {
    path: PathBuf,
    file: File,
    digest: Sha256,
    written: u64,
    selection: Selection,
}

impl Download {
    /// Reopens a bounded regular partial file and hashes its retained prefix for a range request.
    pub fn begin(root: &Path, selection: Selection) -> Result<Self> {
        fs::create_dir_all(root)?;
        let path = root.join(format!(
            "{}.part",
            selection.artifact.sha256.to_ascii_lowercase()
        ));
        if path.exists() {
            ensure!(
                fs::symlink_metadata(&path)?.file_type().is_file(),
                "partial download is not a regular file"
            );
        }
        let mut file = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        let written = file.metadata()?.len();
        if written > selection.artifact.size as u64 {
            file.set_len(0)?;
        }
        let written = file.metadata()?.len();
        let mut digest = Sha256::new();
        let mut reader = (&mut file).take(written);
        ensure!(
            hash(&mut reader, &mut digest)? == written,
            "partial file changed while reopening"
        );
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            path,
            file,
            digest,
            written,
            selection,
        })
    }

    /// The byte offset to request, including a complete retained download awaiting verification.
    pub fn offset(&self) -> u64 {
        self.written
    }

    /// Safely restarts when the server does not support the requested byte range.
    pub fn restart(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.written = 0;
        self.digest = Sha256::new();
        Ok(())
    }

    /// Appends only bytes within the signed size; interrupted transfers retain their prefix.
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() as u64 <= self.selection.artifact.size as u64 - self.written,
            "artifact exceeds signed size"
        );
        self.file.write_all(bytes)?;
        self.digest.update(bytes);
        self.written += bytes.len() as u64;
        Ok(())
    }

    /// Publishes a verified artifact and its signed envelope together, replacing an older stage only
    /// once this one is complete; corrupt partials are discarded.
    pub fn finish(self, destination: &Path) -> Result<PathBuf> {
        self.finish_with(destination, |from, to| fs::rename(from, to))
    }

    /// Publishes with caller-supplied renames so failures and restoration can be tested.
    fn finish_with(
        self,
        destination: &Path,
        mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<PathBuf> {
        ensure!(
            self.written == self.selection.artifact.size as u64
                && self.file.metadata()?.len() == self.written,
            "artifact is truncated"
        );
        let valid = format!("{:x}", self.digest.finalize())
            .eq_ignore_ascii_case(&self.selection.artifact.sha256);
        if !valid {
            drop(self.file);
            fs::remove_file(&self.path)?;
            anyhow::bail!("artifact hash mismatch");
        }
        self.file.sync_all()?;
        drop(self.file);
        let parent = destination.parent().context("missing update directory")?;
        let directory = tempfile::Builder::new()
            .prefix(STAGE_PREFIX)
            .tempdir_in(parent)?;
        let mut manifest = File::create(directory.path().join(ENVELOPE_FILE))?;
        manifest.write_all(&self.selection.envelope)?;
        manifest.sync_all()?;
        drop(manifest);
        rename(&self.path, &directory.path().join(ARTIFACT_FILE))?;
        // Dropping the retired directory deletes the superseded stage.
        let retired = tempfile::Builder::new()
            .prefix(RETIRED_PREFIX)
            .tempdir_in(parent)?;
        let previous = retired.path().join(RETIRED_STAGE);
        let replacing = destination.exists();
        if replacing {
            rename(destination, &previous)?;
        }
        if let Err(error) = rename(directory.path(), destination) {
            if replacing && let Err(restore) = rename(&previous, destination) {
                let retained = retired.keep();
                return Err(error).context(format!(
                    "publish verified stage; restoration failed: {restore}; previous stage retained in {}",
                    retained.display()
                ));
            }
            return Err(error).context("publish verified stage");
        }
        Ok(destination.to_owned())
    }
}

/// Recovers the newest valid stage after an interrupted promotion, then prunes orphan workspaces.
pub fn recover(
    destination: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
) -> Result<()> {
    let parent = destination.parent().context("missing update directory")?;
    let mut workspaces = Vec::new();
    let mut recovered: Option<(PathBuf, Manifest)> = None;
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let candidate = if name.starts_with(RETIRED_PREFIX) {
            entry.path().join(RETIRED_STAGE)
        } else if name.starts_with(STAGE_PREFIX) {
            entry.path()
        } else {
            continue;
        };
        workspaces.push(entry.path());
        if !destination.exists() {
            if let Ok(selected) = verify(&candidate, keys, check, now) {
                let newer = recovered.as_ref().is_none_or(|(_, previous)| {
                    update_manifest::select::newer_build(
                        &selected.manifest.version,
                        selected.manifest.build_number,
                        &previous.version,
                        previous.build_number,
                    )
                    .unwrap_or(false)
                });
                if newer {
                    recovered = Some((candidate, selected.manifest));
                }
            }
        }
    }
    if let Some((candidate, _)) = recovered {
        fs::rename(candidate, destination).context("recover interrupted stage promotion")?;
    }
    for workspace in workspaces {
        // Leftover workspace cleanup cannot hide an otherwise usable ready stage.
        let _ = fs::remove_dir_all(workspace);
    }
    Ok(())
}

/// Revalidates the staged envelope and payload before a native installer or package tool runs.
pub fn verify(
    directory: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
) -> Result<Selection> {
    let mut bytes = Vec::new();
    File::open(directory.join(ENVELOPE_FILE))?
        .take(update_manifest::MAX_ENVELOPE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let selected = Selection::verify(&bytes, keys, check, now)?;
    verify_file(&directory.join(ARTIFACT_FILE), &selected.artifact)?;
    Ok(selected)
}

/// Checks regular files with bounded memory and refuses any size change during hashing.
pub fn verify_file(path: &Path, artifact: &Artifact) -> Result<()> {
    update_manifest::validate_artifact(artifact)?;
    ensure!(
        artifact.size as u64 <= MAX_ARTIFACT_BYTES,
        "artifact exceeds size limit"
    );
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "artifact must be a regular file"
    );
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() == artifact.size as u64,
        "artifact size mismatch"
    );
    let mut digest = Sha256::new();
    let count = hash(&mut file.take(artifact.size as u64 + 1), &mut digest)?;
    ensure!(count == artifact.size as u64, "artifact size changed");
    ensure!(
        format!("{:x}", digest.finalize()).eq_ignore_ascii_case(&artifact.sha256),
        "artifact hash mismatch"
    );
    Ok(())
}

/// Hashes a caller-bounded reader using one fixed buffer.
fn hash(reader: &mut impl Read, digest: &mut Sha256) -> Result<u64> {
    let mut count = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count += read as u64;
        digest.update(&buffer[..read]);
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
