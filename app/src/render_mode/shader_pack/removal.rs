//! Confirmed deletion of imported copies, with file work kept off the frame thread.

use super::super::{
    GraphicsSettings, RuntimeSettings, save_graphics_settings, set_graphics_settings,
};
use super::{MenuRuntime, RenderModeConfig, refresh};
use bevy::{prelude::*, time::Real};
use client_shell::ui_resources::UiRuntimeResource;
use std::sync::{Arc, Mutex, mpsc};

type RemovalResult = Result<(), String>;

#[derive(Resource, Default)]
struct RemovalWorker(Option<(String, Mutex<mpsc::Receiver<RemovalResult>>)>);

/// Orders removal after menu choices and before the renderer sees its new request.
pub(in crate::render_mode) fn configure(app: &mut App) {
    app.init_resource::<RemovalWorker>().add_systems(
        Update,
        (start_removal, finish_removal)
            .chain()
            .after(super::super::apply_menu_render_mode)
            .before(super::sync_shader_pack_request),
    );
}

/// Starts one confirmed deletion and forgets any saved selection of that library entry.
fn start_removal(
    mut worker: ResMut<RemovalWorker>,
    mut config: ResMut<RenderModeConfig>,
    menu: Option<ResMut<MenuRuntime>>,
    mut settings: ResMut<RuntimeSettings>,
) {
    if worker.0.is_some() {
        return;
    }
    let Some(mut menu) = menu else {
        return;
    };
    let Some(name) = menu.take_shader_pack_removal() else {
        return;
    };
    let Some(dirs) = config.shader_packs.as_ref() else {
        menu.finish_shader_pack_removal();
        return;
    };
    let directory = dirs.packs.clone();
    let mut graphics = GraphicsSettings::of(&settings.user_settings_update().1.video);
    let active = super::selected_pack(&config, &settings.user_settings_update().1.video)
        .is_some_and(|pack| dirs.pack_path(&name).as_ref() == Some(&pack.path));
    let remembered = graphics.shader_pack.as_deref() == Some(name.as_str());
    if active || remembered {
        if remembered {
            graphics.shader_pack = None;
        }
        if active {
            graphics.mode = ui::RenderMode::Vanilla;
            config.cli_shader_pack = None;
        }
        set_graphics_settings(&mut settings, &graphics);
        if let Some(path) = &config.path
            && let Err(error) = save_graphics_settings(path, &graphics)
        {
            warn!(%error, "removed shader pack selection could not be saved");
        }
    }
    let (send, receive) = mpsc::channel();
    worker.0 = Some((name.clone(), Mutex::new(receive)));
    let spawned = std::thread::Builder::new()
        .name("shader pack removal".into())
        .spawn(move || {
            let result =
                shaderpack::remove_pack(&directory, &name).map_err(|error| error.to_string());
            let _ = send.send(result);
        });
    if let Err(error) = spawned {
        warn!(%error, "shader pack removal worker could not start");
    }
}

/// Re-lists the library and reports success or failure without blocking on file work.
fn finish_removal(
    mut worker: ResMut<RemovalWorker>,
    mut config: ResMut<RenderModeConfig>,
    menu: Option<ResMut<MenuRuntime>>,
    ui: Option<ResMut<UiRuntimeResource>>,
    time: Res<Time<Real>>,
) {
    let Some((name, receive)) = worker.0.as_mut() else {
        return;
    };
    let outcome = match receive
        .get_mut()
        .unwrap_or_else(|e| e.into_inner())
        .try_recv()
    {
        Ok(outcome) => outcome,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => Err("removal worker stopped".into()),
    };
    let message = match outcome {
        Ok(()) => format!("Removed shader pack {name}"),
        Err(error) => format!("Could not remove shader pack {name}: {error}"),
    };
    worker.0 = None;
    if let Some(mut menu) = menu {
        menu.finish_shader_pack_removal();
        refresh(&mut config, &mut menu);
    }
    if let Some(mut ui) = ui {
        let now = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
        ui.0.push_client_toast(Arc::from("Shader Packs"), Arc::from(message), now);
    }
}

#[cfg(test)]
mod tests;
