//! Small shared authority snapshot; frame checks perform no filesystem or network I/O.

use super::verify::{Manifest, RevocationState, verify_bytes};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub const FILE_PREFIX: &str = "marketplace-";

#[derive(Default)]
struct State {
    epoch: u64,
    account: String,
    issued_at: u64,
    expires_at: u64,
    revoked: Arc<HashMap<String, String>>,
    installed: HashMap<PathBuf, (String, Manifest)>,
}

/// Live admission for managed files, independent of browser trust or persisted grants.
#[derive(Default)]
pub struct Guard(Mutex<State>);

impl Guard {
    /// Switches identity immediately; existing receipts cannot grant another account authority.
    pub fn configure(&self, epoch: u64) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.epoch = epoch;
        state.account.clear();
    }

    /// Applies an authenticated identity only if its control request still belongs to this epoch.
    pub fn account(&self, epoch: u64, xuid: String) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.epoch == epoch {
            state.account = xuid;
        }
    }

    /// Rechecks an uninstalled download against live account and withdrawal authority.
    pub fn permits(&self, owner: &str, hash: &str, now: u64) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        !owner.is_empty()
            && state.account == owner
            && state.expires_at > now
            && state.issued_at <= now.saturating_add(super::verify::CLOCK_SKEW_MS)
            && !state.revoked.contains_key(hash)
    }

    /// Retains every known revoked hash while advancing the signed freshness checkpoint.
    pub fn revocations(&self, value: RevocationState) {
        let previous = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .revoked
            .clone();
        let mut revoked = (*previous).clone();
        revoked.extend(value.revoked);
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.issued_at = value.issued_at;
        state.expires_at = value.expires_at;
        state.revoked = Arc::new(revoked);
    }

    /// Returns the last accepted issue time so a replay cannot roll back this session.
    pub fn checkpoint(&self) -> u64 {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).issued_at
    }

    /// Registers a receipt only after its signature, declaration and on-disk bytes are verified.
    pub fn installed(&self, path: PathBuf, owner: String, manifest: Manifest) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .installed
            .insert(path, (owner, manifest));
    }

    /// Supplies signed declarations for comparison on the loader worker, never the frame thread.
    pub fn manifest(&self, path: &Path) -> Option<Manifest> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .installed
            .get(path)
            .map(|(_, manifest)| manifest.clone())
    }

    /// Tests freshness and ownership at installation and immediately before frame admission.
    pub fn allows(&self, path: &Path, now: u64) -> bool {
        if !managed(path) {
            return true;
        }
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        allowed(&state, path, now)
    }

    /// Validates a captured source against its receipt without reading mutable files again.
    pub fn check(&self, path: &Path, bytes: &[u8], now: u64) -> Result<()> {
        if managed(path) {
            let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            ensure!(allowed(&state, path, now), "{}", reason(&state, path, now));
            let (_, manifest) = state
                .installed
                .get(path)
                .ok_or_else(|| anyhow::anyhow!("Missing marketplace receipt."))?;
            let manifest = manifest.clone();
            drop(state);
            verify_bytes(bytes, &manifest)?;
            ensure!(
                self.allows(path, now),
                "Marketplace authority changed while source bytes were checked."
            );
        }
        Ok(())
    }

    /// Blocks a withdrawal hint immediately, pending the next signed revocation refresh.
    pub fn hold(&self, hash: &str) {
        let previous = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .revoked
            .clone();
        if previous.contains_key(hash) {
            return;
        }
        let mut revoked = (*previous).clone();
        revoked.entry(hash.into()).or_default();
        self.0.lock().unwrap_or_else(|e| e.into_inner()).revoked = Arc::new(revoked);
    }

    /// Explains a held file using signed withdrawal text or the current account/freshness state.
    pub fn reason(&self, path: &Path, now: u64) -> String {
        reason(&self.0.lock().unwrap_or_else(|e| e.into_inner()), path, now)
    }
}

/// Evaluates one immutable authority snapshot while its lock is held.
fn allowed(state: &State, path: &Path, now: u64) -> bool {
    state.installed.get(path).is_some_and(|(owner, manifest)| {
        state.account == *owner
            && state.expires_at > now
            && state.issued_at <= now.saturating_add(super::verify::CLOCK_SKEW_MS)
            && !state.revoked.contains_key(&manifest.sha256)
    })
}

/// Produces plain text for the existing bounded Mods status renderer.
fn reason(state: &State, path: &Path, now: u64) -> String {
    if let Some((owner, manifest)) = state.installed.get(path) {
        if let Some(reason) = state.revoked.get(&manifest.sha256) {
            return if reason.is_empty() {
                "Marketplace release withdrawn by staff.".into()
            } else {
                format!("Marketplace release withdrawn: {reason}")
            };
        }
        if owner != &state.account {
            return "Sign into this mod's account before enabling it.".into();
        }
        if state.expires_at <= now {
            return "Marketplace checks expired. Reconnect and reload the mod after checks refresh.".into();
        }
    }
    "Marketplace release held: current signed evidence is unavailable.".into()
}

/// Reserves managed filenames even if a receipt was deleted or corrupted.
pub fn managed(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(FILE_PREFIX))
}
