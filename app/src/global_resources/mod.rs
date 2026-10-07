//! Global pack management. Live application is a Cinnabar extension to Bedrock.

mod icons;
mod memory;
mod picker;
mod worker;

use crate::{menu::MenuRuntime, runtime::network::PackReload};
use bevy::prelude::*;
use std::{path::PathBuf, sync::Arc};

pub(crate) use launcher::global_resources::{Action, Snapshot};

/// Creates the import worker; no filesystem or ZIP work runs on a frame.
pub(crate) fn configure(app: &mut App, root: PathBuf, files: Vec<PathBuf>) {
    app.insert_resource(worker::Worker::new(root, files))
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
                .before(crate::app::ClientFrameSet::UiPublication)
                .before(render::ChunkRenderApplySet),
        );
}

/// Delivers UI/file commands and publishes completed immutable snapshots.
fn drive(
    mut worker: ResMut<worker::Worker>,
    mut menu: ResMut<MenuRuntime>,
    mut reload: ResMut<PackReload>,
    mut drops: MessageReader<bevy::window::FileDragAndDrop>,
) {
    for event in drops.read() {
        if let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = event {
            worker.send(worker::Command::Import(path_buf.clone()));
        }
    }
    for action in std::mem::take(&mut menu.global_resource_actions) {
        let revision = worker.snapshot.revision;
        worker.send(worker::Command::Action { action, revision });
    }
    while let Ok(event) = worker.events.try_recv() {
        match event {
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
