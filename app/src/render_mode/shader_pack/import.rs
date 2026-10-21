//! Shader packs dropped onto the window while Video settings lists them are copied into the
//! shader-pack folder off the frame, as Iris's shader pack screen does; the list then
//! refreshes and a toast reports the outcome with Iris's wording. Unlike Iris, an added pack
//! is not selected, since picking a pack here applies it at once.

use std::{path::PathBuf, sync::Arc};

use bevy::{prelude::*, time::Real, window::FileDragAndDrop};
use client_shell::ui_resources::UiRuntimeResource;
use shaderpack::DropOutcome;

use super::{MenuRuntime, RenderModeConfig, refresh};

const TOAST_TITLE: &str = "Shader Packs";

/// Outcomes of import workers, each covering one frame's dropped files.
#[derive(Resource)]
struct ShaderPackImports {
    sender: crossbeam_channel::Sender<DropOutcome>,
    receiver: crossbeam_channel::Receiver<DropOutcome>,
}

/// Adds the drop import to the render-mode plugin.
pub(in crate::render_mode) fn configure(app: &mut App) {
    let (sender, receiver) = crossbeam_channel::unbounded();
    app.insert_resource(ShaderPackImports { sender, receiver })
        .add_systems(Update, (start_imports, finish_imports).chain());
}

/// Hands the files dropped this frame to a worker while the menu takes shader-pack drops.
fn start_imports(
    mut drops: MessageReader<FileDragAndDrop>,
    menu: Option<Res<MenuRuntime>>,
    config: Res<RenderModeConfig>,
    imports: Res<ShaderPackImports>,
) {
    let paths: Vec<PathBuf> = drops
        .read()
        .filter_map(|drop| match drop {
            FileDragAndDrop::DroppedFile { path_buf, .. } => Some(path_buf.clone()),
            _ => None,
        })
        .collect();
    if paths.is_empty() || !menu.is_some_and(|menu| menu.takes_shader_pack_drops()) {
        return;
    }
    let Some(dirs) = config.shader_packs.as_ref() else {
        return;
    };
    let dir = dirs.packs.clone();
    let sender = imports.sender.clone();
    let spawned = std::thread::Builder::new()
        .name("shader pack import".into())
        .spawn(move || {
            let _ = sender.send(shaderpack::add_dropped_packs(&paths, &dir));
        });
    if let Err(error) = spawned {
        warn!(%error, "dropped shader packs could not be added");
    }
}

/// Re-lists the pack folder and toasts each finished import.
fn finish_imports(
    imports: Res<ShaderPackImports>,
    mut config: ResMut<RenderModeConfig>,
    mut menu: Option<ResMut<MenuRuntime>>,
    mut ui: Option<ResMut<UiRuntimeResource>>,
    time: Res<Time<Real>>,
) {
    for outcome in imports.receiver.try_iter() {
        if let DropOutcome::CopyFailed { name, error } = &outcome {
            warn!(pack = %name, %error, "dropped shader pack could not be copied");
        }
        if let Some(menu) = menu.as_deref_mut() {
            refresh(&mut config, menu);
        }
        if let Some(ui) = ui.as_deref_mut() {
            let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
            ui.0.push_client_toast(
                Arc::from(TOAST_TITLE),
                Arc::from(notice(&outcome)),
                now_millis,
            );
        }
    }
}

/// Iris's shader pack screen notification for `outcome`.
fn notice(outcome: &DropOutcome) -> String {
    match outcome {
        DropOutcome::Added(names) => match names.as_slice() {
            [name] => format!("Added Shader Pack \"{name}\""),
            names => format!("Added {} Shader Packs", names.len()),
        },
        DropOutcome::NoPacks(Some(name)) => format!("\"{name}\" is not a valid Shader Pack"),
        DropOutcome::NoPacks(None) => "Files were not valid Shader Packs".to_owned(),
        DropOutcome::AlreadyExists(name) => {
            format!("\"{name}\" is already in your Shader Packs folder!")
        }
        DropOutcome::CopyFailed { name, .. } => format!("Could not add Shader Pack \"{name}\""),
    }
}

#[cfg(test)]
mod tests;
