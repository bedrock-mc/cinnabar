//! Admission and final identity checks for startup reclamation.

use super::*;

/// One reclaimable stale entry awaiting its budget turn.
#[derive(Debug)]
pub(super) enum ReclaimTarget {
    SessionDirectory {
        path: PathBuf,
        owner: SessionOwnerMarker,
    },
    CatalogFile {
        path: PathBuf,
        pid: u32,
        modified: u64,
    },
}

impl ReclaimTarget {
    /// Revalidates death and the exact identity under the root lock immediately before removal.
    pub(super) fn still_reclaimable(
        &self,
        policy: &StaleReclaimPolicy,
        probe: &impl Fn(u32) -> OwnerState,
    ) -> bool {
        match self {
            Self::SessionDirectory { path, owner } => {
                fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir() && !meta.is_symlink())
                    && owner.pid != policy.current_pid
                    && probe(owner.pid) == OwnerState::Dead
                    && load_marker(path).is_ok_and(|marker| marker.as_ref() == Some(owner))
            }
            Self::CatalogFile {
                path,
                pid,
                modified,
            } => {
                fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
                    && *pid != policy.current_pid
                    && probe(*pid) == OwnerState::Dead
                    && modified_unix(path) == Some(*modified)
            }
        }
    }
}

/// Admits old, proven-dead owners while retaining their exact marker for the removal check.
pub(super) fn classify_session_directory(
    path: PathBuf,
    name: &str,
    file_type: std::io::Result<std::fs::FileType>,
    policy: &StaleReclaimPolicy,
    report: &mut StaleReclaimReport,
    candidates: &mut Vec<(u64, String, ReclaimTarget)>,
    probe: &impl Fn(u32) -> OwnerState,
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
                if probe(marker.pid) == OwnerState::Dead {
                    candidates.push((
                        marker.created_unix,
                        name.to_owned(),
                        ReclaimTarget::SessionDirectory {
                            path,
                            owner: marker,
                        },
                    ));
                } else {
                    report.skipped_protected_identity += 1;
                }
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

/// Protects live and indeterminate catalog publishers independently of artifact age.
pub(super) fn classify_catalog_file(
    path: PathBuf,
    name: &str,
    pid: u32,
    policy: &StaleReclaimPolicy,
    report: &mut StaleReclaimReport,
    candidates: &mut Vec<(u64, String, ReclaimTarget)>,
    probe: &impl Fn(u32) -> OwnerState,
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
            if probe(pid) == OwnerState::Dead {
                candidates.push((
                    modified,
                    name.to_owned(),
                    ReclaimTarget::CatalogFile {
                        path,
                        pid,
                        modified,
                    },
                ));
            } else {
                report.skipped_protected_identity += 1;
            }
        }
        Some(_) => report.skipped_fresh += 1,
        None => report.skipped_unreadable_identity += 1,
    }
}

/// Reads an artifact's age without consulting the clock; policy time belongs to the caller.
fn modified_unix(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|age| age.as_secs())
}
