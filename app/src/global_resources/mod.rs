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
        .init_resource::<PackReload>()
        .add_systems(
            Update,
            drive.before(crate::runtime::network::reload_resource_packs),
        )
        .add_systems(
            Update,
            crate::runtime::network::reload_resource_packs
                .after(crate::runtime::network::receive_network_events)
                .before(crate::runtime::world::drive_world_stream)
                .before(client_state::ClientFrameSet::UiPublication)
                .before(render_scene::ChunkRenderApplySet),
        );
}

/// Delivers UI/file commands and publishes completed immutable snapshots. Dropped world files
/// go to the Worlds tab while it shows and every other drop imports as a pack.
fn drive(
    mut worker: ResMut<worker::Worker>,
    mut menu: ResMut<MenuRuntime>,
    mut reload: ResMut<PackReload>,
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
    worlds: Option<ResMut<LocalWorlds>>,
) {
    for request in menu.take_minecraft_import_requests() {
        let action = request.action;
        if !worker.send_minecraft_import(request) {
            menu.fail_minecraft_import(action, WORKER_UNAVAILABLE);
        }
    }
    let mut world_files = Vec::new();
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
                worker.snapshot.busy = false;
                break;
            }
        };
        match event {
            worker::Event::MinecraftImportStatus(message) => {
                worker.minecraft_import_pending = false;
                menu.publish_minecraft_import_pack_status(&message);
            }
            worker::Event::MinecraftImport(snapshot, state) => {
                worker.minecraft_import_request = None;
                worker.minecraft_import_pending |= state.is_some();
                menu.publish_minecraft_import(snapshot, state.map(|state| *state))
            }
            worker::Event::Snapshot(snapshot) => worker.snapshot = snapshot,
            worker::Event::Apply(stack, selection) => {
                let revision = reload.request_globals(stack);
                worker.awaiting = Some((revision, selection));
            }
        }
    }
    if worker.awaiting.as_ref().is_some_and(|(revision, _)| {
        *revision <= reload.completed_revision() && reload.error().is_none()
    }) {
        let (_, selection) = worker.awaiting.take().expect("completed selection");
        worker.send(worker::Command::Commit(selection));
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
    let mut snapshot = worker.snapshot.clone();
    if let Some(progress) = reload.progress() {
        snapshot.busy = true;
        snapshot.message = progress.to_owned();
    } else if let Some(error) = reload.error() {
        snapshot.message = error.to_owned();
    } else if snapshot.message == "Preparing resource packs…" && reload.last_duration().is_some()
    {
        snapshot.message = "Texture packs applied.".into();
    }
    menu.global_resources = Arc::new(snapshot);
}
