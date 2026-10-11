//! Serial disk work keeps staged selection and import results in submission order.

use bevy::prelude::Resource;
use launcher::global_resources::{Action, Snapshot};
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
        if let Command::Commit(selection) = &command {
            snapshot.applied_selection = Some(selection.clone());
        }
        let performs_work = matches!(
            &command,
            Command::Import(_)
                | Command::Commit(_)
                | Command::Action {
                    action: Action::Import | Action::Apply,
                    ..
                }
        );
        if performs_work {
            snapshot.busy = true;
            snapshot.message = "Updating resource packs…".into();
            let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
        }
        let result = match command {
            Command::Commit(selection) => {
                let result = library.commit_selection(&selection);
                snapshot.message.clear();
                result
            }
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
    clear_indexed(snapshot);
    super::icons::refresh(library, snapshot, icon_root);
    snapshot.message = format!(
        "Imported {} resource packs; skipped {} behavior packs. {}",
        report.imported.len(),
        report.skipped_behavior,
        report.rejected.join("; ")
    );
    Ok(())
}

fn clear_indexed(snapshot: &mut Snapshot) {
    snapshot.selected = None;
    snapshot.details_expanded = None;
    snapshot.settings = None;
}

/// Applies an indexed UI action against the last published lists.
fn act(
    library: &mut GlobalPackLibrary,
    snapshot: &mut Snapshot,
    action: Action,
    outgoing: &crossbeam_channel::Sender<Event>,
) -> Result<(), LibraryError> {
    if !matches!(
        action,
        Action::SelectAvailable(_)
            | Action::SelectActive(_)
            | Action::ReadMore(..)
            | Action::Settings(_)
            | Action::CloseSettings
            | Action::ToggleAvailable
            | Action::ToggleActive
    ) {
        snapshot.message.clear();
    }
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
                clear_indexed(snapshot);
            }
        }
        Action::Deactivate(index) => {
            if let Some(pack) = snapshot.active.get(index) {
                library.deactivate(pack.id);
                clear_indexed(snapshot);
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
                let moved = |item: usize| {
                    if item == index {
                        to
                    } else if index < to && item > index && item <= to {
                        item - 1
                    } else if to < index && item >= to && item < index {
                        item + 1
                    } else {
                        item
                    }
                };
                snapshot.details_expanded = snapshot
                    .details_expanded
                    .map(|(active, item)| (active, if active { moved(item) } else { item }));
                snapshot.settings = snapshot.settings.map(moved);
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
                    snapshot.settings = None;
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
    use {
        super::*,
        launcher::global_resources::{Action, Snapshot},
    };

    fn events_for(name: &str, queued: Vec<Command>) -> Vec<Event> {
        let root = std::env::temp_dir().join(format!(
            "cinnabar-pack-worker-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let events = collect_events(root.clone(), queued);
        std::fs::remove_dir_all(root).unwrap();
        events
    }

    fn collect_events(root: PathBuf, queued: Vec<Command>) -> Vec<Event> {
        let (commands, incoming) = crossbeam_channel::unbounded();
        let (outgoing, events) = crossbeam_channel::unbounded();
        for command in queued {
            commands.send(command).unwrap();
        }
        drop(commands);
        run(root, Vec::new(), incoming, outgoing);
        events.try_iter().collect()
    }

    fn write_fixture(root: &std::path::Path, id: u16) -> PathBuf {
        use std::io::Write;
        let path = root.join(format!("fixture-{id}.mcpack"));
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        let manifest = format!(
            r#"{{"format_version":2,"header":{{"uuid":"00000000-0000-0000-0000-{id:012x}","name":"Fixture {id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}],"subpacks":[{{"folder_name":"low","name":"Low","memory_tier":0}},{{"folder_name":"high","name":"High","memory_tier":1}}]}}"#
        );
        archive.write_all(manifest.as_bytes()).unwrap();
        std::fs::write(&path, archive.finish().unwrap().into_inner()).unwrap();
        path
    }

    fn import_fixture(
        library: &mut GlobalPackLibrary,
        root: &std::path::Path,
        id: u16,
    ) -> resource_pack::InstalledPack {
        library
            .import(&write_fixture(root, id))
            .unwrap()
            .imported
            .remove(0)
    }

    #[test]
    fn reordered_cards_and_variant_dialogs_follow_the_same_pack() {
        let fixture = tempfile::tempdir().unwrap();
        let mut library =
            GlobalPackLibrary::open(fixture.path().join("installed"), engine_version()).unwrap();
        let first = import_fixture(&mut library, fixture.path(), 1);
        let second = import_fixture(&mut library, fixture.path(), 2);
        library.activate(first.id).unwrap();
        library.activate(second.id).unwrap();
        let mut snapshot = Snapshot::default();
        refresh(&library, &mut snapshot);
        let (outgoing, _) = crossbeam_channel::unbounded();

        snapshot.details_expanded = Some((true, 1));
        snapshot.settings = Some(1);
        act(&mut library, &mut snapshot, Action::MoveUp(1), &outgoing).unwrap();
        refresh(&library, &mut snapshot);
        assert_eq!(snapshot.active[0].id, first.id);
        assert_eq!(snapshot.details_expanded, Some((true, 0)));
        assert_eq!(snapshot.settings, Some(0));
        assert_eq!(snapshot.selected, Some((true, 0)));

        snapshot.details_expanded = Some((true, 1));
        snapshot.settings = Some(1);
        act(&mut library, &mut snapshot, Action::MoveDown(0), &outgoing).unwrap();
        refresh(&library, &mut snapshot);
        assert_eq!(snapshot.active[0].id, second.id);
        assert_eq!(snapshot.details_expanded, Some((true, 0)));
        assert_eq!(snapshot.settings, Some(0));
        assert_eq!(snapshot.selected, Some((true, 1)));

        snapshot.details_expanded = Some((false, 0));
        act(&mut library, &mut snapshot, Action::MoveUp(1), &outgoing).unwrap();
        assert_eq!(snapshot.details_expanded, Some((false, 0)));
    }

    #[test]
    fn changed_pack_lists_clear_indexed_card_and_dialog_state() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("installed");
        let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
        let first = import_fixture(&mut library, fixture.path(), 1);
        import_fixture(&mut library, fixture.path(), 2);
        library.activate(first.id).unwrap();
        let mut snapshot = Snapshot::default();
        refresh(&library, &mut snapshot);
        let (outgoing, _) = crossbeam_channel::unbounded();

        act(
            &mut library,
            &mut snapshot,
            Action::SelectAvailable(0),
            &outgoing,
        )
        .unwrap();
        assert_eq!(snapshot.selected, Some((false, 0)));
        snapshot.details_expanded = Some((false, 0));
        snapshot.settings = Some(0);
        act(&mut library, &mut snapshot, Action::Activate(0), &outgoing).unwrap();
        assert_eq!(snapshot.selected, None);
        assert_eq!(snapshot.details_expanded, None);
        assert_eq!(snapshot.settings, None);
        refresh(&library, &mut snapshot);

        act(
            &mut library,
            &mut snapshot,
            Action::SelectActive(0),
            &outgoing,
        )
        .unwrap();
        assert_eq!(snapshot.selected, Some((true, 0)));
        snapshot.details_expanded = Some((true, 0));
        snapshot.settings = Some(1);
        act(
            &mut library,
            &mut snapshot,
            Action::Deactivate(0),
            &outgoing,
        )
        .unwrap();
        assert_eq!(snapshot.selected, None);
        assert_eq!(snapshot.details_expanded, None);
        assert_eq!(snapshot.settings, None);
        refresh(&library, &mut snapshot);

        snapshot.selected = Some((false, 0));
        snapshot.details_expanded = Some((false, 0));
        snapshot.settings = Some(0);
        import(
            &mut library,
            write_fixture(fixture.path(), 3),
            &mut snapshot,
            &root.join("icons"),
        )
        .unwrap();
        assert_eq!(snapshot.selected, None);
        assert_eq!(snapshot.details_expanded, None);
        assert_eq!(snapshot.settings, None);
    }

    #[test]
    fn variant_dialog_closes_only_after_a_successful_selection() {
        let fixture = tempfile::tempdir().unwrap();
        let mut library =
            GlobalPackLibrary::open(fixture.path().join("installed"), engine_version()).unwrap();
        let pack = import_fixture(&mut library, fixture.path(), 1);
        library.activate(pack.id).unwrap();
        let mut snapshot = Snapshot::default();
        refresh(&library, &mut snapshot);
        let (outgoing, _) = crossbeam_channel::unbounded();
        let high = snapshot.active[0]
            .subpacks
            .iter()
            .position(|subpack| subpack.folder == "high")
            .unwrap();
        snapshot.settings = Some(0);
        snapshot.details_expanded = Some((true, 0));
        act(
            &mut library,
            &mut snapshot,
            Action::Subpack(high),
            &outgoing,
        )
        .unwrap();
        assert_eq!(snapshot.settings, None);
        assert_eq!(snapshot.details_expanded, Some((true, 0)));
        assert_eq!(library.active()[0].subpack, "high");

        snapshot.settings = Some(0);
        snapshot.active[0].subpacks[high].folder = "missing-variant".into();
        assert!(
            act(
                &mut library,
                &mut snapshot,
                Action::Subpack(high),
                &outgoing
            )
            .is_err()
        );
        assert_eq!(snapshot.settings, Some(0));
        assert_eq!(library.active()[0].subpack, "high");
    }

    #[test]
    fn commit_completion_clears_the_temporary_worker_status() {
        let events = events_for("commit-status", vec![Command::Commit(Vec::new())]);
        let completed = events
            .iter()
            .filter_map(|event| match event {
                Event::Snapshot(snapshot) => Some(snapshot),
                Event::Apply(..) => None,
            })
            .next_back()
            .unwrap();
        assert!(!completed.busy);
        assert!(
            completed.message.is_empty(),
            "successful acknowledgement clears its working status"
        );
        assert_eq!(completed.applied_selection, Some(Vec::new()));
        assert!(!completed.has_pending_changes());
        assert_eq!(
            completed.revision, 0,
            "acknowledgement does not replace staged lists"
        );
    }

    #[test]
    fn presentation_only_actions_do_not_publish_a_pack_update() {
        let events = events_for(
            "presentation-status",
            [
                Action::ToggleAvailable,
                Action::ToggleActive,
                Action::CloseSettings,
            ]
            .into_iter()
            .map(|action| Command::Action {
                action,
                revision: 0,
            })
            .collect(),
        );
        for snapshot in events.iter().filter_map(|event| match event {
            Event::Snapshot(snapshot) => Some(snapshot),
            Event::Apply(..) => None,
        }) {
            assert!(
                !snapshot.busy,
                "presentation changes do not mark pack preparation busy"
            );
            assert!(
                snapshot.message.is_empty(),
                "presentation changes do not publish a working status"
            );
        }
    }

    #[test]
    fn apply_keeps_its_pending_runtime_publication() {
        let events = events_for(
            "apply-status",
            vec![Command::Action {
                action: Action::Apply,
                revision: 0,
            }],
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Apply(..)))
                .count(),
            2
        );
        let completed = events
            .iter()
            .filter_map(|event| match event {
                Event::Snapshot(snapshot) => Some(snapshot),
                Event::Apply(..) => None,
            })
            .next_back()
            .unwrap();
        assert!(!completed.busy);
        assert_eq!(completed.message, "Preparing resource packs…");
    }

    #[test]
    fn failed_import_keeps_its_error_after_busy_finishes() {
        let events = events_for(
            "import-status",
            vec![Command::Import(PathBuf::from("unsupported.worker-fixture"))],
        );
        let completed = events
            .iter()
            .filter_map(|event| match event {
                Event::Snapshot(snapshot) => Some(snapshot),
                Event::Apply(..) => None,
            })
            .next_back()
            .unwrap();
        assert!(!completed.busy);
        assert!(!completed.message.is_empty());
        assert_ne!(completed.message, "Updating resource packs…");
    }

    #[test]
    fn presentation_actions_preserve_a_pending_apply_message() {
        let events = events_for(
            "pending-apply",
            vec![
                Command::Action {
                    action: Action::Apply,
                    revision: 0,
                },
                Command::Action {
                    action: Action::ToggleAvailable,
                    revision: 0,
                },
                Command::Action {
                    action: Action::CloseSettings,
                    revision: 0,
                },
            ],
        );
        let completed = events
            .iter()
            .filter_map(|event| match event {
                Event::Snapshot(snapshot) => Some(snapshot),
                Event::Apply(..) => None,
            })
            .next_back()
            .unwrap();
        assert_eq!(completed.message, "Preparing resource packs…");
        assert!(completed.applied_selection.is_none());
        assert!(!completed.has_pending_changes());
    }

    #[test]
    fn an_acknowledgement_preserves_newer_staging_and_queued_list_indices() {
        use std::io::Write;
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("installed");
        let path = fixture.path().join("fixture.mcpack");
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        archive
            .start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(br#"{"format_version":2,"header":{"uuid":"00000000-0000-0000-0000-000000000001","name":"Fixture","version":[1,0,0]},"modules":[{"type":"resources"}]}"#).unwrap();
        std::fs::write(&path, archive.finish().unwrap().into_inner()).unwrap();
        let mut library = GlobalPackLibrary::open(&root, engine_version()).unwrap();
        let imported = library.import(&path).unwrap().imported.remove(0);
        let acknowledged = vec![resource_pack::ActivePack {
            id: imported.id,
            revision: imported.revision,
            subpack: String::new(),
        }];
        drop(library);
        let events = collect_events(
            root.clone(),
            vec![
                Command::Action {
                    action: Action::Activate(0),
                    revision: 0,
                },
                Command::Action {
                    action: Action::Apply,
                    revision: 1,
                },
                Command::Action {
                    action: Action::Deactivate(0),
                    revision: 1,
                },
                Command::Commit(acknowledged.clone()),
                Command::Action {
                    action: Action::SelectAvailable(0),
                    revision: 2,
                },
            ],
        );
        let completed = events
            .iter()
            .filter_map(|event| match event {
                Event::Snapshot(snapshot) => Some(snapshot),
                Event::Apply(..) => None,
            })
            .next_back()
            .unwrap();
        assert_eq!(completed.applied_selection.as_ref(), Some(&acknowledged));
        assert!(
            completed.selection.is_empty(),
            "runtime acknowledgement preserves newer staged removal"
        );
        assert!(completed.active.is_empty());
        assert!(completed.has_pending_changes());
        assert_eq!(completed.selected, Some((false, 0)));
        assert_eq!(completed.revision, 2);
        assert!(completed.message.is_empty());
        assert_eq!(
            GlobalPackLibrary::open(root, engine_version())
                .unwrap()
                .active(),
            acknowledged
        );
    }

    #[test]
    fn a_failed_save_still_records_the_runtime_acknowledgement() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("installed");
        let (commands, incoming) = crossbeam_channel::unbounded();
        let (outgoing, events) = crossbeam_channel::unbounded();
        let worker_root = root.clone();
        let worker = std::thread::spawn(move || run(worker_root, Vec::new(), incoming, outgoing));
        while !matches!(
            events
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            Event::Snapshot(_)
        ) {}
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::write(&root, b"blocked storage").unwrap();
        commands.send(Command::Commit(Vec::new())).unwrap();
        drop(commands);
        worker.join().unwrap();
        let completed = events
            .try_iter()
            .filter_map(|event| match event {
                Event::Snapshot(snapshot) => Some(snapshot),
                Event::Apply(..) => None,
            })
            .last()
            .unwrap();
        assert_eq!(completed.applied_selection, Some(Vec::new()));
        assert!(!completed.busy);
        assert!(!completed.message.is_empty());
        assert_ne!(completed.message, "Updating resource packs…");
    }

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
