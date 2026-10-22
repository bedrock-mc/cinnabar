//! Per-folder upload state in `upload.json`: consent, retries and the one successful send.
//!
//! A folder without the file has no consent and is never sent. Consent makes it pending; a
//! send either marks it sent for good, schedules a retry with backoff, or abandons it. Senders
//! hold a claim file while sending, so concurrent processes never send one folder twice.

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::folder;

/// The state file inside a crash folder.
pub const UPLOAD_FILE: &str = "upload.json";
/// A persistent file whose OS lock serializes senders and consent changes.
const CLAIM_FILE: &str = "upload.claim";
/// Delay before each retry after a failed attempt.
pub const BACKOFF_MS: [u64; 5] = [60_000, 600_000, 3_600_000, 21_600_000, 86_400_000];
/// Attempts before a folder is abandoned.
pub const MAX_ATTEMPTS: u32 = BACKOFF_MS.len() as u32 + 1;
/// Largest state file read.
const MAX_STATE_BYTES: usize = 4096;

/// Who agreed to send the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Consent {
    /// The player chose Send report for this crash.
    User,
    /// The player turned on sending crash reports automatically.
    Auto,
}

/// A folder's upload state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Upload {
    Pending {
        consent: Consent,
        report_id: String,
        attempts: u32,
        next_at_ms: u64,
    },
    Sent {
        report_id: String,
        at_ms: u64,
    },
    /// The server refused it or every retry failed; only a new Send report tries again.
    Abandoned {
        report_id: String,
        reason: String,
    },
}

/// What one send attempt achieved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The server stored it, or already had it.
    Sent,
    /// A network failure or a temporary refusal; `after_ms` is the server's requested delay.
    Retry { after_ms: Option<u64> },
    /// The server will never accept it.
    Rejected(String),
}

/// The folder's state; `None` without consent or when the file does not parse.
pub fn read(folder: &Path) -> Option<Upload> {
    let mut bytes = Vec::new();
    fs::File::open(folder.join(UPLOAD_FILE))
        .ok()?
        .take(MAX_STATE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_STATE_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// Records `consent` with a fresh `report_id` and returns the state afterwards.
///
/// A sent folder stays sent. Explicit consent upgrades a pending automatic report and makes its
/// retry due now, keeping its id and attempt count. Only the player's own
/// Send report revives an abandoned folder, keeping its id so the server can drop a duplicate.
pub fn consent(
    folder: &Path,
    consent: Consent,
    report_id: &str,
    now_ms: u64,
) -> io::Result<Upload> {
    let _claim = claim(folder, now_ms)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "another sender owns this crash report",
        )
    })?;
    if !super::valid_id(report_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "malformed upload id",
        ));
    }
    let state = match read(folder) {
        Some(Upload::Pending {
            report_id,
            attempts,
            ..
        }) if consent == Consent::User => Upload::Pending {
            consent,
            report_id,
            attempts,
            next_at_ms: now_ms,
        },
        Some(Upload::Abandoned { report_id, .. }) if consent == Consent::User => Upload::Pending {
            consent,
            report_id,
            attempts: 0,
            next_at_ms: now_ms,
        },
        Some(state) => return Ok(state),
        None => Upload::Pending {
            consent,
            report_id: report_id.to_owned(),
            attempts: 0,
            next_at_ms: now_ms,
        },
    };
    write(folder, &state)?;
    Ok(state)
}

/// Records the outcome of an attempt on a pending folder and returns the state afterwards.
pub fn record(folder: &Path, outcome: &Outcome, now_ms: u64) -> io::Result<Upload> {
    let Some(Upload::Pending {
        consent,
        report_id,
        attempts,
        ..
    }) = read(folder)
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "crash report is not pending",
        ));
    };
    let attempts = attempts.saturating_add(1);
    let state = match outcome {
        Outcome::Sent => Upload::Sent {
            report_id,
            at_ms: now_ms,
        },
        Outcome::Rejected(reason) => Upload::Abandoned {
            report_id,
            reason: reason.chars().take(200).collect(),
        },
        Outcome::Retry { .. } if attempts >= MAX_ATTEMPTS => Upload::Abandoned {
            report_id,
            reason: format!("not delivered after {attempts} attempts"),
        },
        Outcome::Retry { after_ms } => {
            let backoff = BACKOFF_MS[(attempts as usize - 1).min(BACKOFF_MS.len() - 1)];
            let requested = after_ms.unwrap_or(0).min(BACKOFF_MS[BACKOFF_MS.len() - 1]);
            Upload::Pending {
                consent,
                report_id,
                attempts,
                next_at_ms: now_ms.saturating_add(backoff.max(requested)),
            }
        }
    };
    write(folder, &state)?;
    Ok(state)
}

/// Pending folders whose next attempt is due at `now_ms`, newest first.
pub fn due(crash_dir: &Path, now_ms: u64) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = folder::list(crash_dir)
        .into_iter()
        .filter(|folder| {
            matches!(read(folder), Some(Upload::Pending { next_at_ms, .. }) if next_at_ms <= now_ms)
        })
        .collect();
    folders.reverse();
    folders
}

/// When the earliest pending folder next falls due.
pub fn next_due(crash_dir: &Path) -> Option<u64> {
    folder::list(crash_dir)
        .iter()
        .filter_map(|folder| match read(folder) {
            Some(Upload::Pending { next_at_ms, .. }) => Some(next_at_ms),
            _ => None,
        })
        .min()
}

/// An OS lock on a stable file; the OS releases it even if the sender dies.
#[derive(Debug)]
pub struct Claim {
    _file: fs::File,
}

/// Claims `folder` for sending; `None` while another sender holds it. `now_ms` is retained
/// for callers that also schedule attempts, but lock ownership never depends on clock time.
pub fn claim(folder: &Path, _now_ms: u64) -> io::Result<Option<Claim>> {
    let file = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join(CLAIM_FILE))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Claim { _file: file })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

/// Replaces the state file atomically, so a reader never sees half a state.
fn write(folder: &Path, state: &Upload) -> io::Result<()> {
    let temporary = folder.join(format!(".{UPLOAD_FILE}.tmp"));
    fs::write(&temporary, serde_json::to_vec(state)?)?;
    fs::rename(temporary, folder.join(UPLOAD_FILE))
}
