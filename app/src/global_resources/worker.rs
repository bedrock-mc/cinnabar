//! Serial disk work keeps staged selection and import results in submission order.

use super::{Action, Snapshot};
use bevy::prelude::Resource;
use resource_pack::{GlobalPackLibrary, LibraryError, ValidatedPackStack};
use std::{path::PathBuf, sync::Arc};

pub(super) enum Command {
    Action { action: Action, revision: u64 },
    Import(PathBuf),
    Commit(Vec<resource_pack::ActivePack>),
}
pub(super) enum Event {
    Snapshot(Snapshot),
    Apply(Arc<ValidatedPackStack>, Vec<resource_pack::ActivePack>),
}

#[derive(Resource)]
pub(super) struct Worker {
    commands: crossbeam_channel::Sender<Command>,
    pub events: crossbeam_channel::Receiver<Event>,
    pub snapshot: Snapshot,
    pub awaiting: Option<(u64, Vec<resource_pack::ActivePack>)>,
}

impl Worker {
    /// Starts one worker, shared by initial imports, file drops and settings actions.
    pub fn new(root: PathBuf, files: Vec<PathBuf>) -> Self {
        let (commands, incoming) = crossbeam_channel::unbounded();
        let (outgoing, events) = crossbeam_channel::unbounded();
        std::thread::spawn(move || run(root, files, incoming, outgoing));
        Self {
            commands,
            events,
            snapshot: Snapshot::default(),
            awaiting: None,
        }
    }

    /// Queues a command without blocking the frame.
    pub fn send(&mut self, command: Command) {
        let _ = self.commands.send(command);
    }
}

/// Opens storage and processes commands until the app drops the sender.
fn run(
    root: PathBuf,
    files: Vec<PathBuf>,
    incoming: crossbeam_channel::Receiver<Command>,
    outgoing: crossbeam_channel::Sender<Event>,
) {
    let mut snapshot = Snapshot::default();
    let icon_root = root.join("icons");
    let mut library = match GlobalPackLibrary::open(root, engine_version()) {
        Ok(library) => library,
        Err(error) => {
            snapshot.message = error.to_string();
            let _ = outgoing.send(Event::Snapshot(snapshot));
            return;
        }
    };
    library.set_device_memory(super::memory::physical_bytes());
    super::icons::refresh(&library, &mut snapshot, &icon_root);
    match library.preview() {
        Ok(stack) => {
            let _ = outgoing.send(Event::Apply(stack, library.active().to_vec()));
        }
        Err(error) => snapshot.message = error.to_string(),
    }
    refresh(&library, &mut snapshot);
    let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
    let initial = files.into_iter().map(Command::Import);
    for command in initial.chain(incoming) {
        if let Command::Action { action, revision } = &command
            && indexed(*action)
            && *revision != snapshot.revision
        {
            snapshot.message = "Resource pack list changed; select the pack again.".into();
            let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
            continue;
        }
        let changes_lists = matches!(
            &command,
            Command::Import(_)
                | Command::Commit(_)
                | Command::Action {
                    action: Action::Activate(_)
                        | Action::Deactivate(_)
                        | Action::MoveUp(_)
                        | Action::MoveDown(_)
                        | Action::Subpack(_)
                        | Action::Import,
                    ..
                }
        );
        snapshot.busy = true;
        snapshot.message = "Updating resource packs…".into();
        let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
        let result = match command {
            Command::Commit(selection) => library.commit_selection(&selection),
            Command::Import(path) => import(&mut library, path, &mut snapshot, &icon_root),
            Command::Action {
                action: Action::Import,
                ..
            } => match super::picker::pick() {
                Ok(Some(path)) => import(&mut library, path, &mut snapshot, &icon_root),
                Ok(None) => {
                    snapshot.message.clear();
                    Ok(())
                }
                Err(error) => {
                    snapshot.message = error;
                    Ok(())
                }
            },
            Command::Action { action, .. } => act(&mut library, &mut snapshot, action, &outgoing),
        };
        if let Err(error) = result {
            snapshot.message = error.to_string();
        }
        snapshot.busy = false;
        if changes_lists {
            snapshot.revision += 1;
        }
        refresh(&library, &mut snapshot);
        let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
    }
}

/// Only actions that refer to published list indices require that list generation.
fn indexed(action: Action) -> bool {
    !matches!(
        action,
        Action::ToggleAvailable
            | Action::ToggleActive
            | Action::CloseSettings
            | Action::Import
            | Action::Apply
    )
}

/// Uses the protocol's single game-version constant for compatibility checks.
fn engine_version() -> [u32; 3] {
    let mut parts = protocol::GAME_VERSION
        .split('.')
        .map(|part| part.parse().unwrap_or(0));
    std::array::from_fn(|_| parts.next().unwrap_or(0))
}

/// Rebuilds the UI lists from the library's highest-priority-first selection.
fn refresh(library: &GlobalPackLibrary, snapshot: &mut Snapshot) {
    snapshot.memory_tier = library.device_memory_tier();
    snapshot.selection = library.active().to_vec();
    snapshot.active = library
        .active()
        .iter()
        .filter_map(|active| library.metadata(active).cloned())
        .collect();
    snapshot.available = library
        .available()
        .iter()
        .filter(|pack| !library.active().iter().any(|active| active.id == pack.id))
        .cloned()
        .collect();
}

/// Imports all resource halves and reports skipped behavior or unsupported entries.
fn import(
    library: &mut GlobalPackLibrary,
    path: PathBuf,
    snapshot: &mut Snapshot,
    icon_root: &std::path::Path,
) -> Result<(), LibraryError> {
    let report = library.import(&path)?;
    super::icons::refresh(library, snapshot, icon_root);
    snapshot.message = format!(
        "Imported {} resource packs; skipped {} behavior packs. {}",
        report.imported.len(),
        report.skipped_behavior,
        report.rejected.join("; ")
    );
    Ok(())
}

/// Applies an indexed UI action against the last published lists.
fn act(
    library: &mut GlobalPackLibrary,
    snapshot: &mut Snapshot,
    action: Action,
    outgoing: &crossbeam_channel::Sender<Event>,
) -> Result<(), LibraryError> {
    snapshot.message.clear();
    match action {
        Action::SelectAvailable(index) => snapshot.selected = Some((false, index)),
        Action::SelectActive(index) => snapshot.selected = Some((true, index)),
        Action::ReadMore(active, index) => {
            let next = Some((active, index));
            snapshot.details_expanded = if snapshot.details_expanded == next {
                None
            } else {
                next
            };
        }
        Action::Activate(index) => {
            if let Some(pack) = snapshot.available.get(index) {
                library.activate(pack.id)?;
                snapshot.selected = None;
            }
        }
        Action::Deactivate(index) => {
            if let Some(pack) = snapshot.active.get(index) {
                library.deactivate(pack.id);
                snapshot.selected = None;
            }
        }
        Action::MoveUp(index) | Action::MoveDown(index) => {
            if let Some(pack) = snapshot.active.get(index) {
                let to = if matches!(action, Action::MoveUp(_)) {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(snapshot.active.len() - 1)
                };
                library.move_pack(pack.id, to)?;
                snapshot.selected = Some((true, to));
            }
        }
        Action::Settings(index) => snapshot.settings = Some(index),
        Action::CloseSettings => snapshot.settings = None,
        Action::Subpack(index) => {
            if let Some(pack) = snapshot
                .settings
                .and_then(|index| snapshot.active.get(index))
            {
                let folder = pack.subpacks.get(index).map(|pack| pack.folder.as_str());
                if let Some(folder) = folder {
                    library.select_subpack(pack.id, folder)?;
                }
            }
        }
        Action::ToggleAvailable => snapshot.available_expanded = !snapshot.available_expanded,
        Action::ToggleActive => snapshot.active_expanded = !snapshot.active_expanded,
        Action::Apply => {
            let stack = library.preview()?;
            let _ = outgoing.send(Event::Apply(stack, library.active().to_vec()));
            snapshot.message = "Preparing resource packs…".into();
        }
        Action::Import => unreachable!("file picker handled by worker"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_commands_from_one_snapshot_keep_both_panel_toggles() {
        let root =
            std::env::temp_dir().join(format!("cinnabar-pack-worker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (commands, incoming) = crossbeam_channel::unbounded();
        let (outgoing, events) = crossbeam_channel::unbounded();
        for action in [Action::ToggleAvailable, Action::ToggleActive] {
            commands
                .send(Command::Action {
                    action,
                    revision: 0,
                })
                .unwrap();
        }
        drop(commands);
        run(root.clone(), Vec::new(), incoming, outgoing);
        let last = events
            .try_iter()
            .filter_map(|event| match event {
                Event::Snapshot(snapshot) => Some(snapshot),
                Event::Apply(..) => None,
            })
            .last()
            .unwrap();
        assert!(!last.available_expanded);
        assert!(!last.active_expanded);
        std::fs::remove_dir_all(root).unwrap();
    }
}
