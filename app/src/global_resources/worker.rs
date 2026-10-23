//! Serial disk work keeps staged selection and import results in submission order.

use bevy::prelude::Resource;
use launcher::global_resources::{Action, Snapshot};
use launcher_host::desktop::picker::{self, Filter};
use resource_pack::{GlobalPackLibrary, LibraryError, PACK_IMPORT_EXTENSIONS, ValidatedPackStack};
use std::{path::PathBuf, sync::Arc};

const PACKS: Filter = Filter {
    title: "Import resource pack",
    name: "Resource packs",
    extensions: &PACK_IMPORT_EXTENSIONS,
    unavailable_hint: "Drop a pack onto the window to import it.",
    max_bytes: Some(resource_pack::MAX_ARCHIVE_BYTES as u64),
};

pub(super) enum Command {
    MinecraftImport(Box<launcher_host::minecraft_import::Request>),
    Action { action: Action, revision: u64 },
    Import(PathBuf),
    ImportFiles(Result<Vec<PathBuf>, String>),
    Commit(Vec<resource_pack::ActivePack>),
    ApplyFailed(Vec<resource_pack::ActivePack>),
    Leave,
}
pub(super) enum Event {
    PickerClosed,
    MinecraftImportStatus(String),
    MinecraftImport(
        launcher::minecraft_import::Snapshot,
        Option<Box<launcher_host::minecraft_import::UserState>>,
    ),
    Snapshot(Snapshot),
    Apply(Arc<ValidatedPackStack>, Vec<resource_pack::ActivePack>),
}

#[derive(Resource)]
pub(super) struct Worker {
    commands: Option<Arc<crossbeam_channel::Sender<Command>>>,
    disk: Option<std::thread::JoinHandle<()>>,
    picker: crossbeam_channel::Sender<()>,
    pub picker_open: bool,
    pub events: crossbeam_channel::Receiver<Event>,
    pub snapshot: Snapshot,
    pub awaiting: Option<(u64, Vec<resource_pack::ActivePack>)>,
    pub minecraft_import_pending: bool,
    /// The menu admits one request until the worker publishes its result.
    pub minecraft_import_request: Option<launcher::minecraft_import::Action>,
}

impl Worker {
    /// Starts serial disk work and a separate interactive picker.
    pub fn new(root: PathBuf, config_root: PathBuf, files: Vec<PathBuf>) -> Self {
        Self::new_with_picker(root, config_root, files, || picker::pick_files(&PACKS))
    }

    /// Starts storage with an injectable file picker for headless testing.
    fn new_with_picker(
        root: PathBuf,
        config_root: PathBuf,
        files: Vec<PathBuf>,
        pick: impl Fn() -> Result<Vec<PathBuf>, String> + Send + 'static,
    ) -> Self {
        let (commands, incoming) = crossbeam_channel::unbounded();
        let (outgoing, events) = crossbeam_channel::unbounded();
        let disk = std::thread::spawn(move || run(root, config_root, files, incoming, outgoing));
        let commands = Arc::new(commands);
        let (picker, requests) = crossbeam_channel::unbounded();
        let picked = Arc::downgrade(&commands);
        std::thread::spawn(move || {
            for () in requests {
                let paths = pick();
                let Some(commands) = picked.upgrade() else {
                    break;
                };
                if commands.send(Command::ImportFiles(paths)).is_err() {
                    break;
                }
            }
        });
        Self {
            commands: Some(commands),
            disk: Some(disk),
            picker,
            picker_open: false,
            events,
            snapshot: Snapshot::default(),
            awaiting: None,
            minecraft_import_pending: false,
            minecraft_import_request: None,
        }
    }

    /// Queues a command without blocking the frame.
    pub fn send(&mut self, command: Command) {
        if matches!(
            command,
            Command::Action {
                action: Action::Import,
                ..
            }
        ) {
            if !self.picker_open && self.picker.send(()).is_ok() {
                self.picker_open = true;
            }
        } else {
            let _ = self.commands.as_ref().expect("live worker").send(command);
        }
    }

    /// Reports whether the explicit import request reached the serial worker.
    pub fn send_minecraft_import(
        &mut self,
        request: launcher_host::minecraft_import::Request,
    ) -> bool {
        let action = request.action;
        let sent = self
            .commands
            .as_ref()
            .expect("live worker")
            .send(Command::MinecraftImport(Box::new(request)))
            .is_ok();
        if sent {
            self.minecraft_import_request = Some(action);
        }
        sent
    }
}

impl Drop for Worker {
    /// Closes disk submissions and saves queued edits without waiting for the picker or reload.
    fn drop(&mut self) {
        drop(self.commands.take());
        if let Some(disk) = self.disk.take()
            && disk.join().is_err()
        {
            bevy::log::error!("resource pack disk worker panicked during shutdown");
        }
    }
}

/// Opens storage and processes commands until the app drops the sender.
fn run(
    root: PathBuf,
    config_root: PathBuf,
    files: Vec<PathBuf>,
    incoming: crossbeam_channel::Receiver<Command>,
    outgoing: crossbeam_channel::Sender<Event>,
) {
    let mut minecraft_import = launcher_host::minecraft_import::Service::new(config_root);
    let mut import_commit_pending = None;
    let mut submitted_selection = None;
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
    library.set_device_memory(launcher_host::memory::physical_bytes());
    super::icons::refresh(&library, &mut snapshot, &icon_root);
    match library.preview() {
        Ok(stack) => {
            submitted_selection = Some(library.active().to_vec());
            let _ = outgoing.send(Event::Apply(stack, library.active().to_vec()));
        }
        Err(error) => snapshot.message = error.to_string(),
    }
    refresh(&library, &mut snapshot);
    let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
    let initial = files.into_iter().map(Command::Import);
    for command in initial.chain(incoming) {
        if matches!(&command, Command::ImportFiles(_)) {
            let _ = outgoing.send(Event::PickerClosed);
        }
        let command = match command {
            Command::Leave
                if snapshot.has_pending_changes()
                    && submitted_selection.as_ref() != Some(&snapshot.selection) =>
            {
                Command::Action {
                    action: Action::Apply,
                    revision: snapshot.revision,
                }
            }
            Command::Leave => continue,
            Command::ApplyFailed(selection) => {
                if submitted_selection.as_ref() == Some(&selection) {
                    submitted_selection = None;
                }
                continue;
            }
            command => command,
        };
        if let Command::Action { action, revision } = &command
            && indexed(*action)
            && *revision != snapshot.revision
        {
            snapshot.message = "Resource pack list changed; select the pack again.".into();
            let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
            continue;
        }
        let mut changes_lists = matches!(
            &command,
            Command::MinecraftImport(_)
                | Command::Import(_)
                | Command::ImportFiles(_)
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
            if submitted_selection.as_ref() == Some(selection) {
                submitted_selection = None;
            }
            snapshot.applied_selection = Some(selection.clone());
        }
        let performs_work = matches!(
            &command,
            Command::MinecraftImport(_)
                | Command::Import(_)
                | Command::ImportFiles(_)
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
        let refresh_icons = |library: &GlobalPackLibrary, snapshot: &mut Snapshot| {
            super::icons::refresh(library, snapshot, &icon_root);
        };
        let result = match command {
            Command::MinecraftImport(request) => {
                let (outcome, changed) = import_minecraft_data(
                    &mut minecraft_import,
                    *request,
                    &mut library,
                    &mut snapshot,
                    refresh_icons,
                );
                changes_lists = changed;
                if let Some(stack) = outcome.stack {
                    import_commit_pending = Some(library.active().to_vec());
                    submitted_selection = Some(library.active().to_vec());
                    let _ = outgoing.send(Event::Apply(stack, library.active().to_vec()));
                }
                let _ = outgoing.send(Event::MinecraftImport(
                    outcome.snapshot,
                    outcome.state.map(Box::new),
                ));
                snapshot.message.clear();
                Ok(())
            }
            Command::Commit(selection) => {
                let result = commit_selection(
                    &mut library,
                    &selection,
                    &mut import_commit_pending,
                    &outgoing,
                );
                snapshot.message.clear();
                result
            }
            Command::Import(path) => {
                import_files(&mut library, vec![path], &mut snapshot, refresh_icons)
            }
            Command::ImportFiles(paths) => match paths {
                Ok(paths) => import_files(&mut library, paths, &mut snapshot, refresh_icons),
                Err(error) => {
                    snapshot.message = error;
                    Ok(())
                }
            },
            Command::Leave | Command::ApplyFailed(_) => {
                unreachable!("handled before disk work")
            }
            Command::Action { action, .. } => {
                let result = act(&mut library, &mut snapshot, action, &outgoing);
                if action == Action::Apply && result.is_ok() {
                    submitted_selection = Some(library.active().to_vec());
                }
                result
            }
        };
        if let Err(error) = result {
            snapshot.message = error.to_string();
        }
        snapshot.busy = false;
        if changes_lists {
            snapshot.revision += 1;
            if library.active() != snapshot.selection
                && let Err(error) = library.save_pending_selection()
            {
                snapshot.message = error.to_string();
            }
        }
        refresh(&library, &mut snapshot);
        let _ = outgoing.send(Event::Snapshot(snapshot.clone()));
    }
}

/// Reports an import acknowledgement against its submitted selection even after staging changes.
fn commit_selection(
    library: &mut GlobalPackLibrary,
    selection: &[resource_pack::ActivePack],
    pending: &mut Option<Vec<resource_pack::ActivePack>>,
    outgoing: &crossbeam_channel::Sender<Event>,
) -> Result<(), LibraryError> {
    let result = library.commit_selection(selection);
    if pending.as_deref() == Some(selection) {
        *pending = None;
        let status = match &result {
            Ok(()) => "Resource packs applied and saved.".to_owned(),
            Err(error) => format!("Resource pack preferences could not be saved: {error}"),
        };
        let _ = outgoing.send(Event::MinecraftImportStatus(status));
    }
    result
}

/// Returns the import result and whether the pack library changed, refreshing artwork only then.
fn import_minecraft_data(
    service: &mut launcher_host::minecraft_import::Service,
    request: launcher_host::minecraft_import::Request,
    library: &mut GlobalPackLibrary,
    snapshot: &mut Snapshot,
    refresh_icons: impl FnOnce(&GlobalPackLibrary, &mut Snapshot),
) -> (launcher_host::minecraft_import::Outcome, bool) {
    let catalog = library.available().to_vec();
    let selection = library.active().to_vec();
    let outcome = service.act(request, library);
    let changed = library.available() != catalog || library.active() != selection;
    if changed {
        clear_indexed(snapshot);
        refresh_icons(library, snapshot);
    }
    (outcome, changed)
}

/// Imports each selection and keeps every batch result visible, then refreshes icons once.
/// A single selection preserves its original status and error handling. Cancellation and
/// entirely failed imports leave the icons unchanged.
fn import_files(
    library: &mut GlobalPackLibrary,
    mut paths: Vec<PathBuf>,
    snapshot: &mut Snapshot,
    refresh_icons: impl FnOnce(&GlobalPackLibrary, &mut Snapshot),
) -> Result<(), LibraryError> {
    if paths.len() == 1 {
        import(library, paths.remove(0), snapshot)?;
        refresh_icons(library, snapshot);
        return Ok(());
    }
    let mut messages = Vec::with_capacity(paths.len());
    let mut imported = false;
    snapshot.message.clear();
    for path in paths {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        match import(library, path, snapshot) {
            Ok(()) => imported = true,
            Err(error) => snapshot.message = error.to_string(),
        }
        messages.push(format!("{name}: {}", snapshot.message));
    }
    snapshot.message = messages.join("\n");
    if imported {
        refresh_icons(library, snapshot);
    }
    Ok(())
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
) -> Result<(), LibraryError> {
    let report = library.import(&path)?;
    clear_indexed(snapshot);
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
            let stack = {
                let _zone = bevy::log::info_span!("pack_apply.preview").entered();
                library.preview()?
            };
            let _ = outgoing.send(Event::Apply(stack, library.active().to_vec()));
            snapshot.message = "Preparing resource packs…".into();
        }
        Action::Import => unreachable!("file picker handled by worker"),
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod disconnect_tests;

#[cfg(test)]
mod drop_tests;
