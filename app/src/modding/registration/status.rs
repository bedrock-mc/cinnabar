//! Bounded extension status messages and atomic file publication.

use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) const MAX_MESSAGE_BYTES: usize = 512;
pub(super) static WRITE_ID: AtomicU64 = AtomicU64::new(0);

/// Removes controls and bounds a status message on UTF-8 character boundaries.
pub(super) fn bounded_message(message: &str) -> String {
    let mut output = String::new();
    for character in message.chars().filter(|character| !character.is_control()) {
        if output.len() + character.len_utf8() > MAX_MESSAGE_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

/// Replaces a JSON file atomically after flushing its complete serialized contents.
pub(super) fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("status directory unavailable"))?;
    fs::create_dir_all(parent)?;
    let id = WRITE_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("pending-{}-{id}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    let _ = fs::remove_file(temporary);
    result
}

/// Applies a trap to its actual instance without discarding a newer admission result.
pub(super) fn record_trap(
    statuses: &mut Vec<super::Status>,
    component: std::path::PathBuf,
    error: String,
    reload: u64,
) {
    if let Some(newer) = statuses.iter_mut().find(|status| {
        status.component == component && status.reload.is_some_and(|known| known > reload)
    }) {
        if newer.state == "reload-error" {
            newer.state = "error".into();
        }
        return;
    }
    statuses.retain(|status| status.component != component);
    statuses.push(super::Status::new(
        component,
        "error",
        Some(error),
        Some(reload),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retained_instance_trap_preserves_the_newer_reload_result() {
        let component = std::path::PathBuf::from("fixture.wasm");
        let mut statuses = vec![super::super::Status::new(
            component.clone(),
            "reload-error",
            Some("new component init failed".into()),
            Some(2),
        )];
        record_trap(
            &mut statuses,
            component,
            "old instance frame trapped".into(),
            1,
        );
        assert_eq!(statuses[0].reload, Some(2));
        assert_eq!(statuses[0].state, "error");
        assert_eq!(
            statuses[0].message.as_deref(),
            Some("new component init failed")
        );
    }

    #[test]
    fn a_late_old_instance_trap_cannot_replace_an_admitted_new_instance() {
        let component = std::path::PathBuf::from("fixture.wasm");
        let mut statuses = vec![super::super::Status::new(
            component.clone(),
            "loaded",
            None,
            Some(2),
        )];
        record_trap(
            &mut statuses,
            component.clone(),
            "old instance frame trapped".into(),
            1,
        );
        assert_eq!(statuses[0].reload, Some(2));
        assert_eq!(statuses[0].state, "loaded");
        record_trap(
            &mut statuses,
            component,
            "current instance trapped".into(),
            2,
        );
        assert_eq!(statuses[0].reload, Some(2));
        assert_eq!(statuses[0].state, "error");
        assert_eq!(
            statuses[0].message.as_deref(),
            Some("current instance trapped")
        );
    }
}
