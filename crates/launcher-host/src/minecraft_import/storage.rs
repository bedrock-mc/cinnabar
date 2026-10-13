//! Durable consent and restore snapshots belong exclusively to Cinnabar's user configuration.

use super::UserState;
use anyhow::{Context, Result};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

const ANSWER_FILE: &str = "minecraft-import-answer.json";
const BACKUP_FILE: &str = "minecraft-import-backup.json";
const MAX_BACKUP_BYTES: u64 = 512 * 1024;

/// An answer marker survives either choice, preview cancellation and process restart.
pub(super) fn answered(root: &Path) -> bool {
    root.join(ANSWER_FILE).exists()
}

/// Persists consent or refusal before the host is allowed to read Minecraft contents.
pub(super) fn answer(root: &Path, import: bool) -> Result<()> {
    write(root, ANSWER_FILE, &serde_json::to_vec(&import)?)
}

/// Reports restore availability without decoding a backup on the frame.
pub(super) fn has_backup(root: &Path) -> bool {
    root.join(BACKUP_FILE).is_file()
}

/// Saves all replaced authorities before any import mutation can begin.
pub(super) fn backup(root: &Path, state: &UserState) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(state)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_BACKUP_BYTES,
        "Import backup exceeds its size limit"
    );
    write(root, BACKUP_FILE, &bytes)
}

/// Reads a bounded backup and validates its settings before presenting a revert.
pub(super) fn restore(root: &Path) -> Result<UserState> {
    let mut bytes = Vec::new();
    fs::File::open(root.join(BACKUP_FILE))?
        .take(MAX_BACKUP_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_BACKUP_BYTES,
        "Import backup exceeds its size limit"
    );
    let mut state: UserState = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        state.settings.stored_bindings_valid()
            && state.servers.len() <= crate::servers::MAX_SAVED_SERVERS
            && state.active.len() <= resource_pack::MAX_PACKS,
        "Import backup is invalid"
    );
    crate::servers::validate(&state.servers)?;
    state.settings = launcher::menu::settings_options::SettingsOptions::decode(
        &serde_json::to_vec(&state.settings)?,
    )
    .context("Import backup settings are invalid")?;
    Ok(state)
}

/// Atomically publishes a private, flushed document without touching a Minecraft path.
fn write(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(root)?;
    let mut temporary = tempfile::NamedTempFile::new_in(root)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(root.join(name))
        .map_err(|error| error.error)
        .context("Save Minecraft import state")?;
    Ok(())
}
