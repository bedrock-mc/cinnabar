//! Shader packs dropped onto the window while the Shaders tab lists them, or picked from its
//! import target, are copied into the shader-pack folder off the frame, as Iris's shader pack
//! screen does; the list then refreshes and a toast reports the outcome with Iris's wording.
//! Unlike Iris, an added pack is not selected, since picking a pack here applies it at once.

use std::{path::PathBuf, sync::Arc};

use bevy::{prelude::*, time::Real, window::FileDragAndDrop};
use client_shell::ui_resources::UiRuntimeResource;
use launcher_host::desktop::picker::{self, Filter};
use shaderpack::DropOutcome;

use super::{MenuRuntime, RenderModeConfig, refresh};

const TOAST_TITLE: &str = "Shader Packs";

/// The import target's file picker. The platform picker selects files, so folder packs are
/// added by dropping them where the window takes drops.
const PACK_FILES: Filter = Filter {
    title: "Add shader packs",
    name: "Shader packs",
    extensions: &shaderpack::PACK_ARCHIVE_EXTENSIONS,
    unavailable_hint: "Drop a pack onto the Shaders tab to add it.",
    max_bytes: None,
};

/// Opens a file picker and returns the chosen files, none on cancel, or a visible error.
type Pick = Arc<dyn Fn(&Filter) -> Result<Vec<PathBuf>, String> + Send + Sync>;

/// What an import worker finished.
enum Finished {
    /// One frame's dropped files.
    Dropped(DropOutcome),
    /// The import target's picker: nothing when cancelled, or the picker's error.
    Picked(Result<Option<DropOutcome>, String>),
}

/// Outcomes of import workers and the picker they open.
#[derive(Resource)]
struct ShaderPackImports {
    sender: crossbeam_channel::Sender<Finished>,
    receiver: crossbeam_channel::Receiver<Finished>,
    pick: Pick,
}

/// Adds the drop and picker imports to the render-mode plugin.
pub(in crate::render_mode) fn configure(app: &mut App) {
    configure_with(app, Arc::new(picker::pick_files));
}

/// [`configure`] with the file picker that `pick` opens.
fn configure_with(app: &mut App, pick: Pick) {
    let (sender, receiver) = crossbeam_channel::unbounded();
    app.insert_resource(ShaderPackImports {
        sender,
        receiver,
        pick,
    })
    .add_systems(
        Update,
        (start_imports, start_picked_import, finish_imports).chain(),
    );
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
            let _ = sender.send(Finished::Dropped(shaderpack::add_dropped_packs(
                &paths, &dir,
            )));
        });
    if let Err(error) = spawned {
        warn!(%error, "dropped shader packs could not be added");
    }
}

/// Opens the file picker on a worker when the import target was pressed, then copies the
/// chosen packs in as a drop would.
fn start_picked_import(
    menu: Option<ResMut<MenuRuntime>>,
    config: Res<RenderModeConfig>,
    imports: Res<ShaderPackImports>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    if !menu.take_shader_pack_import() {
        return;
    }
    let Some(dirs) = config.shader_packs.as_ref() else {
        menu.finish_shader_pack_import();
        return;
    };
    let dir = dirs.packs.clone();
    let sender = imports.sender.clone();
    let pick = Arc::clone(&imports.pick);
    let spawned = std::thread::Builder::new()
        .name("shader pack picker".into())
        .spawn(move || {
            let picked = pick(&PACK_FILES).map(|paths| {
                (!paths.is_empty()).then(|| shaderpack::add_dropped_packs(&paths, &dir))
            });
            let _ = sender.send(Finished::Picked(picked));
        });
    if let Err(error) = spawned {
        warn!(%error, "the shader pack picker could not be opened");
        menu.finish_shader_pack_import();
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
    for finished in imports.receiver.try_iter() {
        let message = match finished {
            Finished::Dropped(outcome) => Some(Ok(outcome)),
            Finished::Picked(picked) => {
                if let Some(menu) = menu.as_deref_mut() {
                    menu.finish_shader_pack_import();
                }
                picked.transpose()
            }
        };
        let Some(message) = message else {
            continue;
        };
        let text = match message {
            Ok(outcome) => {
                if let DropOutcome::CopyFailed { name, error } = &outcome {
                    warn!(pack = %name, %error, "shader pack could not be copied");
                }
                if let Some(menu) = menu.as_deref_mut() {
                    refresh(&mut config, menu);
                }
                notice(&outcome)
            }
            Err(error) => {
                warn!(%error, "the shader pack picker failed");
                error
            }
        };
        if let Some(ui) = ui.as_deref_mut() {
            let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
            ui.0.push_client_toast(Arc::from(TOAST_TITLE), Arc::from(text), now_millis);
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
