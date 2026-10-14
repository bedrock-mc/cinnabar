//! "Clear distant terrain cache": removes every store no client holds open on a thread of
//! its own and reports the result in the menu.

use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        mpsc::{self, Receiver, TryRecvError},
    },
};

use bevy::prelude::{App, Res, ResMut, Resource, Update};
use client_shell::menu::MenuRuntime;
use lod_store::root::{ClearReport, StoreRoot};

/// The cache directory and the clear in progress.
#[derive(Resource)]
pub(super) struct DistantTerrainCacheClear {
    dir: PathBuf,
    /// Behind a mutex only so the resource is `Sync`.
    running: Mutex<Option<Receiver<String>>>,
}

/// Registers the clear for stores under `lod_cache_dir`.
pub(super) fn configure(app: &mut App, lod_cache_dir: &Path) {
    app.insert_resource(DistantTerrainCacheClear {
        dir: lod_cache_dir.to_owned(),
        running: Mutex::new(None),
    })
    .add_systems(Update, clear_distant_terrain_cache);
}

/// What the menu shows for a finished clear.
pub(super) fn clear_message(result: std::io::Result<ClearReport>) -> String {
    match result {
        Ok(report) if report.kept.is_empty() => format!(
            "Cleared the distant terrain cache ({} removed).",
            report.removed.len()
        ),
        Ok(report) => format!(
            "Cleared the distant terrain cache ({} removed); {} in use by a running game were kept.",
            report.removed.len(),
            report.kept.len()
        ),
        Err(error) => format!("Could not clear the distant terrain cache: {error}"),
    }
}

/// Starts a clear the menu asked for, and reports one that finished.
fn clear_distant_terrain_cache(
    menu: Option<ResMut<MenuRuntime>>,
    clear: Res<DistantTerrainCacheClear>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    let mut running = clear.running.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(receiver) = running.as_ref() {
        match receiver.try_recv() {
            Ok(message) => {
                menu.report_distant_terrain_clear(message);
                *running = None;
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                menu.report_distant_terrain_clear(clear_message(Err(std::io::Error::other(
                    "the clearing thread stopped",
                ))));
                *running = None;
            }
        }
    }
    if !menu.take_distant_terrain_clear_request() {
        return;
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let root = StoreRoot::new(&clear.dir);
    let spawned = std::thread::Builder::new()
        .name("lod-cache-clear".to_owned())
        .spawn(move || {
            let _ = sender.send(clear_message(root.clear()));
        });
    match spawned {
        Ok(_) => *running = Some(receiver),
        Err(error) => menu.report_distant_terrain_clear(clear_message(Err(error))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_report_what_was_removed_and_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let message = clear_message(StoreRoot::new(dir.path()).clear());
        assert!(message.starts_with("Cleared"), "{message}");
        assert!(clear_message(Err(std::io::Error::other("denied"))).contains("denied"));
    }
}
