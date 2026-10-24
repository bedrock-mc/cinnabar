//! Atomic immutable components and signed receipts. Missing receipts always deny runtime admission.

use super::{Guard, Keys, Manifest, SignedRelease, guard::FILE_PREFIX, verify::verify_bytes};
use anyhow::{Result, ensure};
use bridge::contract::CinnabarModEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Deserialize, Serialize)]
pub(super) struct Receipt {
    pub owner: String,
    pub selected: CinnabarModEntry,
    pub release: SignedRelease,
}

/// Complete signature/hash-verified bytes awaiting the client's own ABI and grant-declaration checks.
pub struct Downloaded {
    pub(super) file: tempfile::NamedTempFile,
    pub(super) manifest: Manifest,
    pub(super) receipt: Receipt,
    pub(super) generation: Arc<AtomicU64>,
    pub(super) epoch: u64,
    pub(super) guard: Arc<Guard>,
    pub(super) mods: PathBuf,
}

impl Downloaded {
    /// Exposes signed permission requests solely for comparison with the actual component declarations.
    pub fn grants(&self) -> &serde_json::Value {
        &self.manifest.grants
    }

    /// Supplies verified metadata for the owning component host's declaration and ABI checks.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Re-reads the bounded staging file and rejects edits before host compatibility validation.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        let bytes = read(self.file.path(), self.manifest.bytes as usize)?;
        verify_bytes(&bytes, &self.manifest)?;
        Ok(bytes)
    }

    /// Publishes a new disabled file and receipt without overwriting another component or registration.
    pub fn install(self, now: u64) -> Result<PathBuf> {
        self.bytes()?;
        ensure!(
            self.generation.load(Ordering::SeqCst) == self.epoch
                && self
                    .guard
                    .permits(&self.receipt.owner, &self.manifest.sha256, now),
            "Marketplace installation cancelled or held."
        );
        let name = filename(&self.receipt.owner, &self.receipt.selected)?;
        let target = self.mods.join(&name);
        if target.exists() {
            verify_bytes(
                &read(&target, self.manifest.bytes as usize)?,
                &self.manifest,
            )?;
        } else {
            self.file.as_file().sync_all()?;
            self.file.persist_noclobber(&target).map_err(|e| {
                anyhow::anyhow!("Could not publish marketplace component: {}", e.error)
            })?;
        }
        save(
            &self.mods.join(".marketplace").join(format!("{name}.json")),
            &serde_json::to_vec(&self.receipt)?,
        )?;
        self.guard
            .installed(target.clone(), self.receipt.owner, self.manifest);
        Ok(target)
    }
}

/// Derives every destination from validated account and hash selectors; project names never select paths.
pub(super) fn filename(owner: &str, selected: &CinnabarModEntry) -> Result<String> {
    ensure!(
        bridge::valid_cinnabar_xuid(owner) && super::verify::digest_valid(&selected.sha256),
        "Invalid marketplace storage selector."
    );
    let identity = serde_json::to_vec(&(&selected.slug, &selected.version, &selected.sha256))?;
    Ok(format!(
        "{FILE_PREFIX}{owner}-{:x}.wasm",
        Sha256::digest(identity)
    ))
}

/// Recognizes a local removal only for the same signed, previously consumed send request.
pub(super) fn removed(
    mods: &Path,
    owner: &str,
    selected: &CinnabarModEntry,
    keys: &Keys,
    limit: usize,
    now: u64,
) -> bool {
    let result = (|| -> Result<bool> {
        let name = filename(owner, selected)?;
        if mods.join(&name).try_exists()? {
            return Ok(false);
        }
        let receipt: Receipt = serde_json::from_slice(&read(
            &mods.join(".marketplace").join(format!("{name}.json")),
            super::worker::MAX_RESPONSE_BYTES,
        )?)?;
        receipt.release.verify(keys, selected, limit, now)?;
        Ok(receipt.owner == owner && receipt.selected.sent_at == selected.sent_at)
    })();
    result.unwrap_or(false)
}

/// Reads regular files with an enforced byte bound, rejecting leaf links on supported native platforms.
pub(super) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit as u64,
        "Invalid marketplace cache file."
    );
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "Marketplace response exceeds its byte limit."
    );
    Ok(bytes)
}

/// Replaces a complete private metadata checkpoint only after synchronizing its temporary file.
pub(super) fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing marketplace cache directory."))?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|error| anyhow::anyhow!("Could not save marketplace evidence: {}", error.error))?;
    Ok(())
}

/// Restores signed receipts and request identities; corrupt or missing evidence never authorizes a file.
pub(super) fn restore(
    mods: &Path,
    keys: &Keys,
    limit: usize,
    guard: &Guard,
    now: u64,
) -> Vec<Receipt> {
    let mut receipts = Vec::new();
    let Ok(entries) = fs::read_dir(mods.join(".marketplace")) else {
        return receipts;
    };
    for entry in entries.flatten() {
        let restored = (|| -> Result<Receipt> {
            let receipt: Receipt =
                serde_json::from_slice(&read(&entry.path(), super::worker::MAX_RESPONSE_BYTES)?)?;
            let manifest = receipt
                .release
                .verify(keys, &receipt.selected, limit, now)?;
            let name = filename(&receipt.owner, &receipt.selected)?;
            ensure!(
                entry.file_name().to_str() == Some(format!("{name}.json").as_str()),
                "Receipt path mismatch."
            );
            let path = mods.join(name);
            if path.exists() {
                verify_bytes(&read(&path, limit)?, &manifest)?;
                guard.installed(path, receipt.owner.clone(), manifest);
            }
            Ok(receipt)
        })();
        if let Ok(receipt) = restored {
            receipts.push(receipt);
        }
    }
    receipts
}
