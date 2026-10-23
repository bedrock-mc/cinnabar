//! Global pack management. Live application is a Cinnabar extension to Bedrock.

mod icons;
mod worker;

use bevy::prelude::*;
use client_shell::local_worlds::LocalWorlds;
use launcher::local_worlds::Input;
use launcher_host::local_worlds::is_world_archive;
use std::{path::PathBuf, sync::Arc};
use {crate::runtime::network::PackReload, client_shell::menu::MenuRuntime};

const WORKER_UNAVAILABLE: &str = "The resource pack worker is unavailable. Restart the client after correcting the resource pack storage error.";

/// Creates the import worker; no filesystem or ZIP work runs on a frame.
pub(crate) fn configure(app: &mut App, root: PathBuf, config_root: PathBuf, files: Vec<PathBuf>) {
    app.insert_resource(worker::Worker::new(root, config_root, files))
        .add_systems(
            Update,
            drive.before(crate::runtime::network::reload_resource_packs),
        )
        .add_systems(Last, flush_on_exit);
}

/// Saves exit-frame edits and drains queued disk work before shutdown.
fn flush_on_exit(
    mut worker: ResMut<worker::Worker>,
    mut menu: ResMut<MenuRuntime>,
    mut exits: MessageReader<AppExit>,
) {
    if exits.read().next().is_none() {
        return;
    }
    for action in std::mem::take(&mut menu.global_resource_actions) {
        let revision = worker.snapshot.revision;
        worker.send(worker::Command::Action { action, revision });
    }
    worker.flush_pending();
}

/// Pack-reload status folded into the published snapshot, borrowed for comparison.
#[derive(Clone, Copy)]
enum Overlay<'a> {
    Progress(&'a str),
    Error(&'a str),
    Applied,
    Plain,
}

impl<'a> Overlay<'a> {
    /// Classifies the reload status the published snapshot reflects over `message`.
    fn current(reload: &'a PackReload, message: &str) -> Self {
        if let Some(progress) = reload.progress() {
            Self::Progress(progress)
        } else if let Some(error) = reload.error() {
            Self::Error(error)
        } else if message == "Preparing resource packs…" && reload.last_duration().is_some() {
            Self::Applied
        } else {
            Self::Plain
        }
    }
}

/// The overlay last published, retained so unchanged frames skip cloning the snapshot.
enum PublishedOverlay {
    Progress(String),
    Error(String),
    Applied,
    Plain,
}

impl PublishedOverlay {
    fn matches(&self, overlay: Overlay<'_>) -> bool {
        match (self, overlay) {
            (Self::Progress(published), Overlay::Progress(current))
            | (Self::Error(published), Overlay::Error(current)) => published == current,
            (Self::Applied, Overlay::Applied) | (Self::Plain, Overlay::Plain) => true,
            _ => false,
        }
    }
}

impl From<Overlay<'_>> for PublishedOverlay {
    fn from(overlay: Overlay<'_>) -> Self {
        match overlay {
            Overlay::Progress(progress) => Self::Progress(progress.to_owned()),
            Overlay::Error(error) => Self::Error(error.to_owned()),
            Overlay::Applied => Self::Applied,
            Overlay::Plain => Self::Plain,
        }
    }
}

/// Delivers UI/file commands and publishes completed immutable snapshots.
///
/// The published snapshot is a function of the worker's snapshot and the reload overlay, so
/// it is rebuilt only when either changes instead of being cloned every frame. Dropped world
/// files go to the Worlds tab while it shows, the Shaders tab hands drops to the shader-pack
/// import, and every other drop imports as a pack.
fn drive(
    mut worker: ResMut<worker::Worker>,
    mut menu: ResMut<MenuRuntime>,
    mut reload: ResMut<PackReload>,
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
    mut published: Local<Option<PublishedOverlay>>,
    mut was_open: Local<bool>,
    worlds: Option<ResMut<LocalWorlds>>,
) {
    let open = menu.scene() == Some(launcher::menu::MenuScreen::Settings)
        && menu.settings_section()
            == Some(launcher::menu::settings_sections::SettingSection::GlobalTexturePack);
    let mut snapshot_changed = false;
    let picker_was_open = worker.picker_open;
    for request in menu.take_minecraft_import_requests() {
        let action = request.action;
        if !worker.send_minecraft_import(request) {
            menu.fail_minecraft_import(action, WORKER_UNAVAILABLE);
        }
    }
    let mut world_files = Vec::new();
    if menu.takes_shader_pack_drops() {
        drops.clear();
    }
    for event in drops.read() {
        if let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = event {
            if is_world_archive(path_buf) {
                world_files.push(path_buf.clone());
            } else {
                worker.send(worker::Command::Import(path_buf.clone()));
            }
        }
    }
    if !world_files.is_empty() {
        match worlds {
            Some(mut worlds) if menu.showing_worlds() => {
                worlds.input(Input::ImportFiles(world_files));
            }
            _ => info!("dropped world files import from the Worlds tab only"),
        }
    }
    for action in std::mem::take(&mut menu.global_resource_actions) {
        let revision = worker.snapshot.revision;
        worker.send(worker::Command::Action { action, revision });
    }
    if !open && *was_open {
        worker.send(worker::Command::Leave);
    }
    *was_open = open;
    loop {
        let event = match worker.events.try_recv() {
            Ok(event) => event,
            Err(crossbeam_channel::TryRecvError::Empty) => break,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                if let Some(action) = worker.minecraft_import_request.take() {
                    menu.fail_minecraft_import(action, WORKER_UNAVAILABLE);
                }
                if worker.minecraft_import_pending {
                    menu.publish_minecraft_import_pack_status(WORKER_UNAVAILABLE);
                    worker.minecraft_import_pending = false;
                }
                worker.awaiting = None;
                worker.picker_open = false;
                snapshot_changed |= std::mem::take(&mut worker.snapshot.busy);
                break;
            }
        };
        match event {
            worker::Event::PickerClosed => worker.picker_open = false,
            worker::Event::MinecraftImportStatus(message) => {
                worker.minecraft_import_pending = false;
                menu.publish_minecraft_import_pack_status(&message);
            }
            worker::Event::MinecraftImport(snapshot, state) => {
                worker.minecraft_import_request = None;
                worker.minecraft_import_pending |= state.is_some();
                menu.publish_minecraft_import(snapshot, state.map(|state| *state))
            }
            worker::Event::Snapshot(snapshot) => {
                worker.snapshot = snapshot;
                snapshot_changed = true;
            }
            worker::Event::Apply(stack, selection) => {
                let revision = reload.request_globals(stack);
                worker.awaiting = Some((revision, selection));
            }
        }
    }
    if worker
        .awaiting
        .as_ref()
        .is_some_and(|(revision, _)| *revision <= reload.completed_revision())
    {
        let (_, selection) = worker.awaiting.take().expect("completed selection");
        worker.send(if reload.error().is_none() {
            worker::Command::Commit(selection)
        } else {
            worker::Command::ApplyFailed
        });
    }
    if worker.minecraft_import_pending {
        if let Some(error) = reload.error() {
            menu.publish_minecraft_import_pack_status(&format!(
                "Resource packs could not be applied: {error}"
            ));
        } else if let Some(progress) = reload.progress() {
            menu.publish_minecraft_import_pack_status(progress);
        }
    }
    let overlay = Overlay::current(&reload, &worker.snapshot.message);
    snapshot_changed |= picker_was_open != worker.picker_open;
    if !snapshot_changed
        && published
            .as_ref()
            .is_some_and(|published| published.matches(overlay))
    {
        return;
    }
    let mut snapshot = worker.snapshot.clone();
    snapshot.busy |= worker.picker_open;
    match overlay {
        Overlay::Progress(progress) => {
            snapshot.busy = true;
            snapshot.message = progress.to_owned();
        }
        Overlay::Error(error) => snapshot.message = error.to_owned(),
        Overlay::Applied => snapshot.message = "Texture packs applied.".into(),
        Overlay::Plain => {}
    }
    *published = Some(overlay.into());
    menu.global_resources = Arc::new(snapshot);
}
