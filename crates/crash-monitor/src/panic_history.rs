//! Selects the first retained report in the client's final panic cascade.

use std::path::{Path, PathBuf};

use crash_report::{
    folder,
    report::{Kind, Report},
};

/// Selects from reports still on disk instead of keeping snapshots that outlive pruning.
/// Only one candidate is held while walking the bounded retained folders backwards.
pub(crate) fn select(crash_dir: &Path, session_id: &str) -> Option<(PathBuf, Report)> {
    let mut selected: Option<(PathBuf, Report)> = None;
    for folder in folder::list(crash_dir).into_iter().rev() {
        let Ok(report) = folder::read(&folder) else {
            continue;
        };
        if report.session_id != session_id || report.kind != Kind::Panic {
            continue;
        }
        if let Some((_, newer)) = &selected
            && newer.uptime_ms.saturating_sub(report.uptime_ms) > crate::outcome::CASCADE_MS
        {
            break;
        }
        selected = Some((folder, report));
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crash_report::report::Identity;

    #[test]
    fn the_selected_panic_starts_the_last_retained_cascade() {
        for (uptimes, expected) in [
            (vec![], None),
            (vec![5_000], Some(0)),
            (vec![1_000, 60_000, 60_200, 60_900], Some(1)),
            (vec![1_000, 60_000], Some(1)),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let identity = Identity {
                session_id: "0123456789abcdef".into(),
                started_at_ms: 0,
                release: "test".into(),
                commit: "test".into(),
                build_profile: "test".into(),
            };
            for (index, uptime) in uptimes.into_iter().enumerate() {
                let mut report = Report::new(Kind::Panic, index.to_string(), &identity, uptime);
                report.uptime_ms = uptime;
                folder::write(
                    directory.path(),
                    &report,
                    &folder::Logs::default(),
                    None,
                    uptime,
                )
                .unwrap();
            }
            let selected = select(directory.path(), &identity.session_id);
            assert_eq!(
                selected.map(|(_, report)| report.message),
                expected.map(|index| index.to_string())
            );
            assert!(select(directory.path(), "other-session").is_none());
        }
    }
}
