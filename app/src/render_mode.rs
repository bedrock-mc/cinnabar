//! Render-mode setting: saved choice, session overrides, and the per-camera
//! Enhanced opt-in. Vanilla stays the default and the evidence-run mode.

use std::{
    ffi::OsStr,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use bevy::{
    camera::Camera3dDepthTextureUsage,
    ecs::system::lifetimeless::{Read, Write},
    post_process::bloom::Bloom,
    prelude::*,
    render::{render_resource::TextureUsages, view::Hdr},
};
use render::{EnhancedRenderPlugin, EnhancedRendering};
use render_model::enhanced_rendering_enabled;
use serde::{Deserialize, Serialize};
use ui::RenderMode;

use {
    crate::{menu::MenuRuntime, settings_runtime::RuntimeSettings},
    client_presentation::camera::FlyCamera,
};

pub(crate) const RENDER_MODE_ENV: &str = "CINNABAR_RENDER_MODE";
const MAX_GRAPHICS_FILE_BYTES: u64 = 4096;

#[derive(Serialize, Deserialize)]
struct GraphicsFile {
    render_mode: String,
}

/// Saved mode, or `None` when the file is absent, oversized, or unreadable.
pub(crate) fn load_render_mode(path: &Path) -> Option<RenderMode> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_GRAPHICS_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_GRAPHICS_FILE_BYTES {
        return None;
    }
    let file = serde_json::from_slice::<GraphicsFile>(&bytes).ok()?;
    RenderMode::parse(&file.render_mode)
}

/// Atomically replace the small graphics extension settings file.
pub(crate) fn save_render_mode(path: &Path, mode: RenderMode) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&GraphicsFile {
        render_mode: mode.as_str().to_owned(),
    })?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).with_context(|| format!("write {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))
}

/// CLI beats environment beats the saved file. Attributable evidence runs
/// ignore the environment and saved file so captures stay vanilla.
pub(crate) fn startup_render_mode(
    cli: Option<RenderMode>,
    env: Option<&OsStr>,
    saved: Option<RenderMode>,
    attributable: bool,
) -> RenderMode {
    if !enhanced_rendering_enabled() {
        return RenderMode::Vanilla;
    }
    if let Some(mode) = cli {
        return mode;
    }
    if attributable {
        return RenderMode::Vanilla;
    }
    env.and_then(OsStr::to_str)
        .and_then(RenderMode::parse)
        .or(saved)
        .unwrap_or_default()
}

pub(crate) struct RenderModePlugin {
    cli: Option<RenderMode>,
    attributable: bool,
}

impl RenderModePlugin {
    /// Configure session overrides and deterministic evidence runs.
    pub(crate) const fn new(cli: Option<RenderMode>, attributable: bool) -> Self {
        Self { cli, attributable }
    }
}

#[derive(Resource)]
struct RenderModeConfig {
    cli: Option<RenderMode>,
    attributable: bool,
    path: Option<PathBuf>,
}

impl Plugin for RenderModePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EnhancedRenderPlugin)
            .insert_resource(RenderModeConfig {
                cli: self.cli,
                attributable: self.attributable,
                path: None,
            })
            .add_systems(Startup, seed_render_mode)
            .add_systems(
                Update,
                (
                    apply_menu_render_mode,
                    apply_render_mode_to_cameras,
                    sync_enhanced_bloom,
                )
                    .chain(),
            );
    }
}

/// Resolve the saved choice and session overrides at startup.
fn seed_render_mode(
    mut config: ResMut<RenderModeConfig>,
    menu: Option<Res<MenuRuntime>>,
    mut settings: ResMut<RuntimeSettings>,
) {
    config.path = menu.map(|menu| menu.graphics_file());
    let saved = config.path.as_deref().and_then(load_render_mode);
    let mode = startup_render_mode(
        config.cli,
        std::env::var_os(RENDER_MODE_ENV).as_deref(),
        saved,
        config.attributable,
    );
    set_render_mode(&mut settings, mode);
}

/// Update the shared settings authority only when the choice changes.
fn set_render_mode(settings: &mut RuntimeSettings, mode: RenderMode) {
    let mode = if enhanced_rendering_enabled() {
        mode
    } else {
        RenderMode::Vanilla
    };
    let (_, current) = settings.user_settings_update();
    if current.video.render_mode != mode {
        let mut next = current.clone();
        next.video.render_mode = mode;
        settings.replace_user_settings(next);
    }
}

/// Persist menu changes and keep the displayed toggle synchronized.
fn apply_menu_render_mode(
    config: Res<RenderModeConfig>,
    menu: Option<ResMut<MenuRuntime>>,
    mut settings: ResMut<RuntimeSettings>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    if let Some(mode) = menu.take_render_mode_request() {
        set_render_mode(&mut settings, mode);
        if let Some(path) = &config.path
            && let Err(error) = save_render_mode(path, mode)
        {
            warn!(?error, "render mode could not be saved");
        }
    }
    menu.sync_render_mode(settings.user_settings_update().1.video.render_mode);
}

/// Original depth usage, restored when opting out of Enhanced.
#[derive(Component)]
struct VanillaDepthUsage(Camera3dDepthTextureUsage);

/// Camera state needed to apply or restore the optional mode.
type RenderModeCameraQuery = (
    Entity,
    Write<Camera3d>,
    Has<EnhancedRendering>,
    Option<Read<VanillaDepthUsage>>,
);

/// Keep the opt-in effects on gameplay cameras only.
fn apply_render_mode_to_cameras(
    mut commands: Commands,
    settings: Res<RuntimeSettings>,
    mut cameras: Query<RenderModeCameraQuery, With<FlyCamera>>,
) {
    let enhanced = enhanced_rendering_enabled()
        && settings.user_settings_update().1.video.render_mode == RenderMode::Enhanced;
    for (entity, mut camera, has_enhanced, vanilla_depth) in &mut cameras {
        if enhanced != has_enhanced {
            if enhanced {
                let original = camera.depth_texture_usages;
                camera.depth_texture_usages =
                    (TextureUsages::from(original) | TextureUsages::TEXTURE_BINDING).into();
                let enhanced_settings = EnhancedRendering::default();
                #[cfg(feature = "enhanced-diagnostics")]
                let enhanced_settings = if render_model::enhanced_diagnostics_enabled() {
                    EnhancedRendering::bounded_diagnostic()
                } else {
                    enhanced_settings
                };
                commands.entity(entity).insert((
                    enhanced_settings,
                    Hdr,
                    VanillaDepthUsage(original),
                ));
            } else {
                if let Some(VanillaDepthUsage(original)) = vanilla_depth {
                    camera.depth_texture_usages = *original;
                }
                commands
                    .entity(entity)
                    .remove::<(EnhancedRendering, Hdr, Bloom, VanillaDepthUsage)>();
            }
        }
    }
}

/// Apply the per-camera bloom quality switch without changing other effects.
fn sync_enhanced_bloom(
    mut commands: Commands,
    cameras: Query<(Entity, &EnhancedRendering, Has<Bloom>), With<FlyCamera>>,
) {
    for (entity, enhanced, has_bloom) in &cameras {
        let bloom = enhanced_rendering_enabled() && enhanced.bloom;
        if bloom == has_bloom {
            continue;
        }
        if bloom {
            commands.entity(entity).insert(Bloom {
                intensity: 0.12,
                ..default()
            });
        } else {
            commands.entity(entity).remove::<Bloom>();
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, client_presentation::camera::FlyCamera};

    /// Every startup source is forced to Vanilla while Enhanced is disabled.
    #[test]
    fn disabled_enhanced_ignores_cli_environment_and_saved_settings() {
        for cli in [None, Some(RenderMode::Vanilla), Some(RenderMode::Enhanced)] {
            for env in [
                None,
                Some(OsStr::new("enhanced")),
                Some(OsStr::new("vanilla")),
            ] {
                for saved in [None, Some(RenderMode::Vanilla), Some(RenderMode::Enhanced)] {
                    for attributable in [false, true] {
                        assert_eq!(
                            startup_render_mode(cli, env, saved, attributable),
                            RenderMode::Vanilla,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn saved_render_mode_round_trips_and_rejects_malformed_files() {
        let directory = std::env::temp_dir().join(format!(
            "cinnabar-render-mode-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = directory.join("nested/graphics.json");
        assert_eq!(load_render_mode(&path), None);
        save_render_mode(&path, RenderMode::Enhanced).expect("save");
        assert_eq!(load_render_mode(&path), Some(RenderMode::Enhanced));
        save_render_mode(&path, RenderMode::Vanilla).expect("save");
        assert_eq!(load_render_mode(&path), Some(RenderMode::Vanilla));
        fs::write(&path, br#"{"render_mode":"ultra"}"#).expect("write");
        assert_eq!(load_render_mode(&path), None);
        fs::write(&path, vec![b' '; MAX_GRAPHICS_FILE_BYTES as usize + 1]).expect("write");
        assert_eq!(load_render_mode(&path), None);
        let _ = fs::remove_dir_all(directory);
    }

    /// Stale settings and camera components cannot enable the disabled renderer.
    #[test]
    fn disabled_enhanced_clears_camera_effects_and_rejects_runtime_requests() {
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>().add_systems(
            Update,
            (apply_render_mode_to_cameras, sync_enhanced_bloom).chain(),
        );
        let original = Camera3d::default().depth_texture_usages;
        let camera = app
            .world_mut()
            .spawn((
                Camera3d {
                    depth_texture_usages: (TextureUsages::from(original)
                        | TextureUsages::TEXTURE_BINDING)
                        .into(),
                    ..default()
                },
                FlyCamera::default(),
                EnhancedRendering::default(),
                Hdr,
                Bloom::default(),
                VanillaDepthUsage(original),
            ))
            .id();
        // Bypass the normal setting setter to simulate stale in-memory state.
        let mut stale = ui::UserSettings::default();
        stale.video.render_mode = RenderMode::Enhanced;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(stale);
        app.update();
        assert!(app.world().get::<EnhancedRendering>(camera).is_none());
        assert!(app.world().get::<Hdr>(camera).is_none());
        assert!(app.world().get::<Bloom>(camera).is_none());
        assert!(app.world().get::<VanillaDepthUsage>(camera).is_none());
        assert_eq!(
            TextureUsages::from(
                app.world()
                    .get::<Camera3d>(camera)
                    .unwrap()
                    .depth_texture_usages
            ),
            TextureUsages::from(original),
        );
        set_render_mode(
            &mut app.world_mut().resource_mut::<RuntimeSettings>(),
            RenderMode::Enhanced,
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<RuntimeSettings>()
                .user_settings_update()
                .1
                .video
                .render_mode,
            RenderMode::Vanilla
        );
        assert!(app.world().get::<EnhancedRendering>(camera).is_none());
        assert!(app.world().get::<Hdr>(camera).is_none());
        assert!(app.world().get::<Bloom>(camera).is_none());
    }
}
