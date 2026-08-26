//! Ownership and reclamation of per-session runtime directories.
//!
//! Every client-owned session gets its own directory under the install
//! layout's transient runtime root: `connect-<pid>-<generation>` for
//! launcher sessions and `direct-<pid>` for `--address` starts. The Go
//! core publishes its bridge endpoint artifact inside that directory, so
//! the directory is the unit of per-session runtime state.
//!
//! Before this module existed those directories were created and never
//! removed, so every disconnect, failed start, or crash left another stale
//! directory behind. Now:
//!
//! * [`SessionDirectoryGuard`] is an identity-checked RAII owner. It binds
//!   one exclusive directory, writes an owner marker into it, and removes
//!   the directory on drop across normal disconnect, error teardown, and
//!   orderly shutdown paths. Removal happens only while the on-disk marker
//!   still names this exact binding's random token; any mismatch refuses
//!   deletion loudly instead of guessing.
//! * [`reclaim_stale_session_directories`] runs once per startup and
//!   removes leftovers from earlier crashed incarnations under bounded,
//!   explicitly provisional age and count policy. It only touches entries
//!   whose name matches the exact session-directory grammar and whose
//!   owner marker proves both family membership and a different owning
//!   process; everything else is left untouched.
//!
//! Cleanup failures never abort startup or shutdown; they are logged and
//! retried by the next startup's reclamation pass. A teardown wedged past
//! the shutdown watchdog ends in `process::exit`, which cannot run
//! destructors, so such leaks are expected to be reclaimed at the next
//! launch rather than hidden.

use std::{
    collections::hash_map::RandomState,
    fs,
    hash::{BuildHasher, Hasher},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::install_layout::InstallLayout;

/// Marker file written inside every owned session directory.
const MARKER_FILE_NAME: &str = "session-owner.json";

/// Maximum age of a leftover session directory before startup reclamation
/// may remove it.
///
/// PROVISIONAL POLICY: no operational measurement of real leak ages exists,
/// so this bound is deliberately generous (far beyond any legitimate
/// session) and must be revisited with telemetry before being tightened.
/// A live session younger than this bound is never reclaimable regardless
/// of which process owns it.
const STALE_SESSION_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Maximum number of stale entries reclaimed during one startup pass.
///
/// PROVISIONAL POLICY: bounds worst-case startup work after a pathological
/// backlog of crashed sessions; unclaimed entries stay for the next
/// startup instead of extending one launch's work unboundedly.
const MAX_STALE_RECLAIMS_PER_STARTUP: usize = 64;

/// Which session flavor a directory belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SessionKind {
    Direct,
    Connect,
}

/// Owner marker persisted inside a bound session directory.
///
/// The random [`SessionOwnerMarker::token`] is the removal authority: a
/// guard may delete its directory only while the marker on disk still
/// carries the token this binding wrote.
#[derive(Debug, Serialize, Deserialize)]
struct SessionOwnerMarker {
    token: String,
    pid: u32,
    kind: SessionKind,
    generation: u64,
    created_unix: u64,
}

/// A parsed `<kind>-<pid>[-<generation>]` session directory name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SessionDirId {
    kind: SessionKind,
    pid: u32,
    generation: u64,
}

/// Parses the strict session-directory grammar.
///
/// Anything outside `direct-<pid>` / `connect-<pid>-<generation>` returns
/// `None`, including empty segments, signs, overflow, and extra segments.
fn parse_session_dir_name(name: &str) -> Option<SessionDirId> {
    let (kind, rest) = name
        .strip_prefix("direct-")
        .map(|rest| (SessionKind::Direct, rest))
        .or_else(|| {
            name.strip_prefix("connect-")
                .map(|rest| (SessionKind::Connect, rest))
        })?;
    let (pid_text, generation_text) = match kind {
        SessionKind::Direct => (rest, None),
        SessionKind::Connect => {
            let (pid_text, generation_text) = rest.split_once('-')?;
            (pid_text, Some(generation_text))
        }
    };
    if pid_text.is_empty()
        || pid_text.len() > 10
        || !pid_text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let generation = match generation_text {
        Some(text) => {
            if text.is_empty() || text.len() > 20 || !text.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            text.parse::<u64>().ok()?
        }
        None => 0,
    };
    Some(SessionDirId {
        kind,
        pid: pid_text.parse::<u32>().ok()?,
        generation,
    })
}

fn parse_catalog_file_name(name: &str) -> Option<u32> {
    let pid_text = name.strip_prefix("catalog-")?.strip_suffix(".json")?;
    if pid_text.is_empty()
        || pid_text.len() > 10
        || !pid_text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    pid_text.parse::<u32>().ok()
}

/// Errors raised while binding a session directory.
///
/// Identity conflicts are deliberately distinct from filesystem errors so
/// callers surface them loudly instead of guessing about ownership.
#[derive(Debug, Error)]
pub(crate) enum SessionDirectoryError {
    #[error("invalid session directory name `{name}`")]
    InvalidName { name: String },
    #[error("create session directory {directory}: {source}")]
    Create {
        directory: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "refusing to reuse session directory {directory}: its identity marker names another owning process ({pid})"
    )]
    ForeignIdentity { directory: PathBuf, pid: u32 },
    #[error(
        "refusing to reuse session directory {directory}: its identity marker could not be read: {source}"
    )]
    UnreadableIdentity {
        directory: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "refusing to reuse session directory {directory}: its identity marker is unreadable data: {source}"
    )]
    MalformedIdentity {
        directory: PathBuf,
        source: serde_json::Error,
    },
    #[error("write session identity marker in {directory}: {source}")]
    WriteMarker {
        directory: PathBuf,
        source: std::io::Error,
    },
}

/// Outcome of an explicit [`SessionDirectoryGuard::release`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReleaseOutcome {
    Removed,
    AlreadyReleased,
    DirectoryMissing,
    /// The marker on disk no longer proves this binding owns the
    /// directory; nothing was deleted.
    IdentityRefused,
    /// Removal was attempted with proven identity but the filesystem
    /// refused; the next startup's reclamation pass retries.
    RemoveFailed,
}

/// Identity-checked RAII owner of one per-session runtime directory.
///
/// Drop removes the owned directory exactly once; an explicit
/// [`SessionDirectoryGuard::release`] makes later drops no-ops. Removal is
/// gated on the on-disk marker matching this binding's token, so a
/// directory reused or tampered with by someone else is never deleted.
#[derive(Debug)]
pub(crate) struct SessionDirectoryGuard {
    directory: PathBuf,
    token: String,
    bound: bool,
}

impl SessionDirectoryGuard {
    /// Creates (or takes over) the exclusive session directory at
    /// `directory` and writes this binding's owner marker into it.
    ///
    /// An existing directory is reusable only when its marker is absent
    /// (a leftover from an earlier client build whose name already embeds
    /// this process id) or names this same process id, which proves the
    /// previous owner is dead: two live processes cannot share a pid.
    /// Any other on-disk identity refuses the bind loudly rather than
    /// guessing.
    pub(crate) fn bind(directory: PathBuf) -> Result<Self, SessionDirectoryError> {
        let name = directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = parse_session_dir_name(&name)
            .ok_or_else(|| SessionDirectoryError::InvalidName { name: name.clone() })?;
        let parent = match directory.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_owned(),
            _ => {
                return Err(SessionDirectoryError::Create {
                    directory: directory.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "session directory has no parent",
                    ),
                });
            }
        };
        fs::create_dir_all(&parent).map_err(|source| SessionDirectoryError::Create {
            directory: parent,
            source,
        })?;
        let mut freshly_created = false;
        match fs::create_dir(&directory) {
            Ok(()) => freshly_created = true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                take_over_dead_predecessor(&directory)?;
            }
            Err(source) => {
                return Err(SessionDirectoryError::Create {
                    directory: directory.clone(),
                    source,
                });
            }
        }
        let marker = SessionOwnerMarker {
            token: fresh_token(),
            pid: process::id(),
            kind: id.kind,
            generation: id.generation,
            created_unix: unix_now(),
        };
        if let Err(error) = write_marker(&directory, &marker) {
            if freshly_created {
                let _ = fs::remove_dir_all(&directory);
            }
            return Err(error);
        }
        Ok(Self {
            directory,
            token: marker.token,
            bound: true,
        })
    }

    /// Removes the owned directory after proving the on-disk marker still
    /// names this binding. Idempotent: later calls report
    /// [`ReleaseOutcome::AlreadyReleased`] and do nothing.
    pub(crate) fn release(&mut self) -> ReleaseOutcome {
        if !self.bound {
            return ReleaseOutcome::AlreadyReleased;
        }
        if !self.directory.exists() {
            self.bound = false;
            return ReleaseOutcome::DirectoryMissing;
        }
        let marker = match load_marker(&self.directory) {
            Ok(Some(on_disk)) if on_disk.token == self.token && on_disk.pid == process::id() => {
                on_disk
            }
            Ok(_) | Err(_) => {
                // Identity mismatch or unreadable proof: never guess.
                eprintln!(
                    "session-runtime: refusing removal of {}: its identity marker no longer proves this binding owns it",
                    self.directory.display()
                );
                self.bound = false;
                return ReleaseOutcome::IdentityRefused;
            }
        };
        match fs::remove_dir_all(&self.directory) {
            Ok(()) => {
                self.bound = false;
                ReleaseOutcome::Removed
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.bound = false;
                ReleaseOutcome::DirectoryMissing
            }
            Err(_) => {
                // `remove_dir_all` may remove the marker before encountering
                // an undeletable child. Restore the same proven identity so
                // this still-live guard can safely retry the partial cleanup.
                let _ = write_marker(&self.directory, &marker);
                eprintln!(
                    "session-runtime: could not remove session directory {}; a later release or startup reclamation retries",
                    self.directory.display()
                );
                ReleaseOutcome::RemoveFailed
            }
        }
    }
}

impl Drop for SessionDirectoryGuard {
    fn drop(&mut self) {
        self.release();
    }
}

/// Scope-lifetime owner for the `--address` direct session directory.
///
/// Same identity-checked removal semantics as [`SessionDirectoryGuard`],
/// packaged as an opaque holder so `run()` can keep it alive to function
/// exit without reading it: the holder releases the directory when the
/// scope ends, which is strictly after the explicit `drop(app)` statement
/// has stopped the core child, and across any early-return unwind.
#[derive(Debug)]
pub(crate) struct ScopedSessionDirectory(Option<SessionDirectoryGuard>);

impl ScopedSessionDirectory {
    /// An empty holder for sessions that own no directory.
    #[must_use]
    pub(crate) fn none() -> Self {
        Self(None)
    }

    /// Binds the exclusive session directory, failing loudly on identity
    /// conflicts exactly like [`SessionDirectoryGuard::bind`].
    pub(crate) fn bind(directory: PathBuf) -> Result<Self, SessionDirectoryError> {
        Ok(Self(Some(SessionDirectoryGuard::bind(directory)?)))
    }
}

impl Drop for ScopedSessionDirectory {
    fn drop(&mut self) {
        if let Some(mut directory) = self.0.take() {
            directory.release();
        }
    }
}

/// Adjudicates an existing directory found during [`bind`](SessionDirectoryGuard::bind).
fn take_over_dead_predecessor(directory: &Path) -> Result<(), SessionDirectoryError> {
    match load_marker(directory) {
        // Earlier client builds wrote no marker; the name embedding this
        // process id plus single-process pid uniqueness prove the previous
        // owner is gone.
        Ok(None) => replace_directory(directory),
        Ok(Some(existing)) if existing.pid == process::id() => replace_directory(directory),
        Ok(Some(existing)) => Err(SessionDirectoryError::ForeignIdentity {
            directory: directory.to_owned(),
            pid: existing.pid,
        }),
        Err(MarkerProblem::Io(source)) => Err(SessionDirectoryError::UnreadableIdentity {
            directory: directory.to_owned(),
            source,
        }),
        Err(MarkerProblem::Malformed(source)) => Err(SessionDirectoryError::MalformedIdentity {
            directory: directory.to_owned(),
            source,
        }),
    }
}

/// Removes and recreates a dead predecessor's directory so takeover starts
/// from empty, provably-owned state.
fn replace_directory(directory: &Path) -> Result<(), SessionDirectoryError> {
    fs::remove_dir_all(directory).map_err(|source| SessionDirectoryError::Create {
        directory: directory.to_owned(),
        source,
    })?;
    fs::create_dir(directory).map_err(|source| SessionDirectoryError::Create {
        directory: directory.to_owned(),
        source,
    })?;
    Ok(())
}

/// Why an owner marker could not be interpreted.
enum MarkerProblem {
    Io(std::io::Error),
    Malformed(serde_json::Error),
}

/// Reads and validates the owner marker inside `directory`.
///
/// `Ok(None)` means no marker file exists; anything else that prevents a
/// trusted reading returns a typed problem instead of a guess.
fn load_marker(directory: &Path) -> Result<Option<SessionOwnerMarker>, MarkerProblem> {
    let bytes = match fs::read(directory.join(MARKER_FILE_NAME)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(MarkerProblem::Io(error)),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(MarkerProblem::Malformed)
}

/// Writes this binding's owner marker.
fn write_marker(
    directory: &Path,
    marker: &SessionOwnerMarker,
) -> Result<(), SessionDirectoryError> {
    let path = directory.join(MARKER_FILE_NAME);
    let write = || -> std::io::Result<()> {
        let bytes =
            serde_json::to_vec(marker).map_err(|error| std::io::Error::other(error.to_string()))?;
        fs::write(&path, bytes)
    };
    write().map_err(|source| SessionDirectoryError::WriteMarker {
        directory: directory.to_owned(),
        source,
    })
}

/// Age/count policy for one startup reclamation pass.
///
/// Constructed through [`StaleReclaimPolicy::for_current_process`] in
/// production; tests construct exact values directly.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StaleReclaimPolicy {
    pub(crate) now_unix: u64,
    pub(crate) max_age_secs: u64,
    pub(crate) limit: usize,
    pub(crate) current_pid: u32,
}

impl StaleReclaimPolicy {
    #[must_use]
    pub(crate) fn for_current_process() -> Self {
        Self {
            now_unix: unix_now(),
            max_age_secs: STALE_SESSION_MAX_AGE_SECS,
            limit: MAX_STALE_RECLAIMS_PER_STARTUP,
            current_pid: process::id(),
        }
    }
}

/// Bounded counters describing one reclamation pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StaleReclaimReport {
    /// Stale session directories removed.
    pub(crate) reclaimed_directories: usize,
    /// Stale leaked `catalog-<pid>.json` endpoint artifacts removed.
    pub(crate) reclaimed_catalog_files: usize,
    /// Entries whose proven removal attempt failed; they stay for the
    /// next startup.
    pub(crate) failed: usize,
    /// Grammar-valid entries younger than the maximum age.
    pub(crate) skipped_fresh: usize,
    /// Entries whose marker names a different-or-current live-capable
    /// process identity, so they were protected instead of guessed at.
    pub(crate) skipped_protected_identity: usize,
    /// Grammar-valid directories without any owner marker (earlier
    /// client builds): identity cannot be proven, so they are never
    /// touched.
    pub(crate) skipped_unmarked: usize,
    /// Grammar-valid entries whose marker exists but cannot be read or
    /// parsed: identity cannot be proven, so they are never touched.
    pub(crate) skipped_unreadable_identity: usize,
    /// Entries outside the session grammar (unrelated user data).
    pub(crate) skipped_unrelated: usize,
}

/// Reclaims leftovers from crashed previous sessions once per startup.
///
/// Fire-and-forget production entry point: failures are logged and never
/// abort gameplay startup.
pub(crate) fn reclaim_stale_session_directories(layout: &InstallLayout) -> StaleReclaimReport {
    reclaim_stale_entries(
        layout.transient_runtime_root(),
        &StaleReclaimPolicy::for_current_process(),
    )
}

/// Reclamation pass over one transient runtime root.
///
/// Only entries that prove their identity are ever removed:
///
/// * session directories must match the strict grammar, be real
///   directories (never symlinks), and carry an owner marker naming a
///   different process id plus an age at or beyond the maximum;
/// * `catalog-<pid>.json` artifacts must match the strict grammar, name a
///   different process id, and carry a modification time at or beyond the
///   maximum.
///
/// Everything else is counted and left untouched. Removals run
/// oldest-first under the count budget so a pathological backlog drains
/// deterministically across launches instead of extending one startup
/// unboundedly.
pub(crate) fn reclaim_stale_entries(
    root: &Path,
    policy: &StaleReclaimPolicy,
) -> StaleReclaimReport {
    let mut report = StaleReclaimReport::default();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return report,
        Err(_) => {
            log_line("could not enumerate the transient runtime root; skipping startup reclaim");
            return report;
        }
    };
    let mut candidates: Vec<(u64, String, ReclaimTarget)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(pid) = parse_catalog_file_name(&name) {
            classify_catalog_file(
                entry.path(),
                &name,
                pid,
                policy,
                &mut report,
                &mut candidates,
            );
            continue;
        }
        if parse_session_dir_name(&name).is_none() {
            report.skipped_unrelated += 1;
            continue;
        }
        classify_session_directory(
            entry.path(),
            &name,
            entry.file_type(),
            policy,
            &mut report,
            &mut candidates,
        );
    }
    candidates.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
    candidates.truncate(policy.limit);
    for (_, name, target) in candidates {
        let removal = match &target {
            ReclaimTarget::SessionDirectory(path) => fs::remove_dir_all(path),
            ReclaimTarget::CatalogFile(path) => fs::remove_file(path),
        };
        match removal {
            Ok(()) => match target {
                ReclaimTarget::SessionDirectory(_) => {
                    report.reclaimed_directories += 1;
                    log_line(&format!("reclaimed stale session directory '{name}'"));
                }
                ReclaimTarget::CatalogFile(_) => {
                    report.reclaimed_catalog_files += 1;
                    log_line(&format!("reclaimed stale catalog artifact '{name}'"));
                }
            },
            Err(error) => {
                report.failed += 1;
                log_line(&format!("could not reclaim stale entry '{name}': {error}"));
            }
        }
    }
    let removed = report.reclaimed_directories + report.reclaimed_catalog_files;
    if removed > 0 || report.failed > 0 {
        log_line(&format!(
            "startup reclaim removed={removed} failed={}",
            report.failed
        ));
    }
    report
}

/// One reclaimable stale entry awaiting its budget turn.
#[derive(Debug)]
enum ReclaimTarget {
    SessionDirectory(PathBuf),
    CatalogFile(PathBuf),
}

fn classify_session_directory(
    path: PathBuf,
    name: &str,
    file_type: std::io::Result<std::fs::FileType>,
    policy: &StaleReclaimPolicy,
    report: &mut StaleReclaimReport,
    candidates: &mut Vec<(u64, String, ReclaimTarget)>,
) {
    match file_type {
        Ok(file_type) if file_type.is_dir() && !file_type.is_symlink() => {}
        _ => {
            report.skipped_unrelated += 1;
            return;
        }
    }
    match load_marker(&path) {
        Ok(Some(marker)) => {
            // A marker naming this very process belongs to a binding of
            // the current incarnation (or will be taken over by one);
            // reclamation never races it.
            if marker.pid == policy.current_pid {
                report.skipped_protected_identity += 1;
                return;
            }
            let age = policy.now_unix.saturating_sub(marker.created_unix);
            if age >= policy.max_age_secs {
                candidates.push((
                    marker.created_unix,
                    name.to_owned(),
                    ReclaimTarget::SessionDirectory(path),
                ));
            } else {
                report.skipped_fresh += 1;
            }
        }
        // No marker means identity is unprovable (an earlier client build
        // or foreign data); it is never clearly stale, so it stays.
        Ok(None) => report.skipped_unmarked += 1,
        Err(_) => report.skipped_unreadable_identity += 1,
    }
}

fn classify_catalog_file(
    path: PathBuf,
    name: &str,
    pid: u32,
    policy: &StaleReclaimPolicy,
    report: &mut StaleReclaimReport,
    candidates: &mut Vec<(u64, String, ReclaimTarget)>,
) {
    let is_plain_file = fs::symlink_metadata(&path)
        .map(|metadata| metadata.is_file())
        .unwrap_or_default();
    if !is_plain_file {
        report.skipped_unrelated += 1;
        return;
    }
    // The live menu runtime owns this incarnation's catalog file.
    if pid == policy.current_pid {
        report.skipped_protected_identity += 1;
        return;
    }
    match modified_unix(&path) {
        Some(modified) if policy.now_unix.saturating_sub(modified) >= policy.max_age_secs => {
            candidates.push((modified, name.to_owned(), ReclaimTarget::CatalogFile(path)));
        }
        Some(_) => report.skipped_fresh += 1,
        None => report.skipped_unreadable_identity += 1,
    }
}

fn modified_unix(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|age| age.as_secs())
}

/// One bounded diagnostic line per notable cleanup event.
fn log_line(message: &str) {
    eprintln!("session-runtime: {message}");
}

/// Generates a fresh 128-bit binding token.
///
/// `RandomState` seeds itself from operating-system entropy on every
/// construction, so two independent hasher states yield unpredictable bits
/// without adding a randomness dependency.
fn fresh_token() -> String {
    let mut high_hasher = RandomState::new().build_hasher();
    high_hasher.write_u128(u128::from(process::id()));
    let high = high_hasher.finish();
    let mut low_hasher = RandomState::new().build_hasher();
    low_hasher.write_u64(high);
    low_hasher.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    format!("{high:016x}{:016x}", low_hasher.finish())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
#[path = "session_cleanup_tests.rs"]
mod session_cleanup_tests;
