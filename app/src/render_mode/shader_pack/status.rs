//! Hands the render host's shader-pack load state to the menu, which raises its toasts and
//! explains a failure in the Shaders tab.

use std::path::Path;

use bevy::prelude::*;
use client_shell::ui_resources::UiRuntimeResource;
use launcher::menu::shader_pack_status::PackLoad;
use render_scene::shaderpack::{ShaderPackLoadProgress, ShaderPackStatus};

use super::super::{MenuRuntime, RenderModeConfig};

/// The picker's name for the pack at `path`: its listed name, else its file or folder name
/// (a launch `--shader-pack` outside the folder).
fn pack_name<'a>(config: &'a RenderModeConfig, path: &'a Path) -> &'a str {
    config
        .shader_packs
        .as_ref()
        .and_then(|dirs| dirs.discovered.iter().find(|(_, listed)| listed == path))
        .map(|(name, _)| name.as_str())
        .or_else(|| path.file_name()?.to_str())
        .unwrap_or_default()
}

/// The menu's view of `status`, with the running load's progress.
fn pack_load<'a>(
    config: &'a RenderModeConfig,
    status: &'a ShaderPackStatus,
    progress: Option<&ShaderPackLoadProgress>,
) -> PackLoad<'a> {
    match status {
        ShaderPackStatus::Idle => PackLoad::Idle,
        ShaderPackStatus::Loading(path) | ShaderPackStatus::Ready(path) => {
            let pack = pack_name(config, path);
            if matches!(status, ShaderPackStatus::Loading(_))
                || progress.is_some_and(ShaderPackLoadProgress::is_loading)
            {
                PackLoad::Loading {
                    pack,
                    programs: progress.and_then(ShaderPackLoadProgress::programs),
                }
            } else {
                PackLoad::Ready { pack }
            }
        }
        ShaderPackStatus::Failed { pack, reason } => PackLoad::Failed {
            pack: pack_name(config, pack),
            reason,
        },
    }
}

/// Mirrors the load state into the menu each frame; builds without the shader-pack host
/// have no status and stay on Vanilla silently.
pub(in super::super) fn sync_shader_pack_status(
    config: Res<RenderModeConfig>,
    status: Option<Res<ShaderPackStatus>>,
    progress: Option<Res<ShaderPackLoadProgress>>,
    menu: Option<ResMut<MenuRuntime>>,
    runtime: Option<ResMut<UiRuntimeResource>>,
    time: Res<Time<Real>>,
    mut seen: Local<Option<u64>>,
) {
    let (Some(status), Some(mut menu), Some(mut runtime)) = (status, menu, runtime) else {
        return;
    };
    let load = pack_load(&config, &status, progress.as_deref());
    let revision = progress
        .as_deref()
        .map_or(0, ShaderPackLoadProgress::revision);
    if seen
        .replace(revision)
        .is_some_and(|previous| previous != revision)
        && let PackLoad::Ready { pack } | PackLoad::Failed { pack, .. } = load
    {
        menu.sync_shader_pack_status(
            PackLoad::Loading {
                pack,
                programs: None,
            },
            &mut runtime,
            time.elapsed(),
        );
    }
    menu.sync_shader_pack_status(load, &mut runtime, time.elapsed());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A renderer configuration without install paths or launch overrides.
    fn config() -> RenderModeConfig {
        RenderModeConfig {
            cli: None,
            attributable: false,
            path: None,
            cli_shader_pack: None,
            shader_packs: None,
            shader_pack_revision: 0,
        }
    }

    #[test]
    fn ready_pack_reload_reports_loading_until_the_worker_finishes() {
        let config = config();
        let status = ShaderPackStatus::Ready("Dusk.zip".into());
        let mut progress = ShaderPackLoadProgress::default();
        assert_eq!(
            pack_load(&config, &status, Some(&progress)),
            PackLoad::Ready { pack: "Dusk.zip" }
        );
        let worker = progress.start();
        worker.record(3, 7);
        assert_eq!(
            pack_load(&config, &status, Some(&progress)),
            PackLoad::Loading {
                pack: "Dusk.zip",
                programs: Some((3, 7))
            }
        );
        progress.finish();
        assert_eq!(
            pack_load(&config, &status, Some(&progress)),
            PackLoad::Ready { pack: "Dusk.zip" }
        );
    }
    #[test]
    fn completed_reload_reports_its_outcome_without_a_loading_frame() {
        let mut app = App::new();
        app.insert_resource(config())
            .insert_resource(ShaderPackStatus::Ready("Dusk.zip".into()))
            .init_resource::<ShaderPackLoadProgress>()
            .insert_resource(MenuRuntime::new(true, 2, "Test".into()))
            .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
            .init_resource::<Time<Real>>()
            .add_systems(Update, sync_shader_pack_status);
        app.update();
        let before = app
            .world()
            .resource::<UiRuntimeResource>()
            .hud()
            .toasts()
            .len();
        {
            let mut progress = app.world_mut().resource_mut::<ShaderPackLoadProgress>();
            progress.start();
            progress.finish();
        }
        app.update();
        assert_eq!(
            app.world()
                .resource::<UiRuntimeResource>()
                .hud()
                .toasts()
                .len(),
            before + 1
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<UiRuntimeResource>()
                .hud()
                .toasts()
                .len(),
            before + 1
        );
    }
}
