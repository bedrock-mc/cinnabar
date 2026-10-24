//! Render-mode setting: saved choice, session overrides, and the per-camera
//! Enhanced and shader-pack opt-ins. Vanilla stays the default and the evidence-run mode.

use std::{
    ffi::OsStr,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use bevy::{
    camera::{Camera3dDepthTextureUsage, Hdr},
    ecs::system::lifetimeless::{Read, Write},
    post_process::bloom::Bloom,
    prelude::*,
    render::render_resource::TextureUsages,
};
use render_model::{SHADER_PACKS_ENABLED, enhanced_rendering_enabled};
use serde::{Deserialize, Serialize};
use ui::RenderMode;
use {
    render::{EnhancedRenderPlugin, ShaderPackRenderPlugin},
    render_scene::{
        EnhancedRendering,
        shaderpack::{ShaderPackRendering, ShaderPackRequest, ShaderPackStatus},
    },
};

use {
    client_presentation::camera::FlyCamera, client_shell::menu::MenuRuntime,
    client_state::RuntimeSettings,
};

pub(crate) mod authored_textures;
#[cfg(feature = "shaderpacks")]
mod shader_options;
mod shader_pack;

pub(crate) const RENDER_MODE_ENV: &str = "CINNABAR_RENDER_MODE";
const MAX_GRAPHICS_FILE_BYTES: u64 = 4096;

#[derive(Serialize, Deserialize)]
struct GraphicsFile {
    render_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shader_pack: Option<String>,
}

/// The graphics extension choices saved in `graphics.json`.
#[derive(Clone, Debug, Default, PartialEq)]
struct GraphicsSettings {
    mode: RenderMode,
    /// Selected pack name inside the shader-pack directory.
    shader_pack: Option<String>,
}

impl GraphicsSettings {
    /// The choices currently applied in `video`.
    fn of(video: &ui::VideoSettings) -> Self {
        Self {
            mode: video.render_mode,
            shader_pack: video.shader_pack.clone(),
        }
    }
}

/// Saved choices, or `None` when the file is absent, oversized, or unreadable. A missing
/// pack in older files selects none without losing the mode.
fn load_graphics_settings(path: &Path) -> Option<GraphicsSettings> {
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
    Some(GraphicsSettings {
        mode: RenderMode::parse(&file.render_mode)?,
        shader_pack: file.shader_pack.filter(|name| !name.is_empty()),
    })
}

/// Atomically replace the small graphics extension settings file.
fn save_graphics_settings(path: &Path, graphics: &GraphicsSettings) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Context::with_context(fs::create_dir_all(parent), || {
            format!("create {}", parent.display())
        })?;
    }
    let bytes = serde_json::to_vec_pretty(&GraphicsFile {
        render_mode: graphics.mode.as_str().to_owned(),
        shader_pack: graphics.shader_pack.clone(),
    })?;
    let temp = path.with_extension("json.tmp");
    Context::with_context(fs::write(&temp, bytes), || {
        format!("write {}", temp.display())
    })?;
    Context::with_context(fs::rename(&temp, path), || {
        format!("replace {}", path.display())
    })
}

#[cfg(test)]
fn load_render_mode(path: &Path) -> Option<RenderMode> {
    load_graphics_settings(path).map(|saved| saved.mode)
}

#[cfg(test)]
fn save_render_mode(path: &Path, mode: RenderMode) -> Result<()> {
    let saved = load_graphics_settings(path).unwrap_or_default();
    save_graphics_settings(path, &GraphicsSettings { mode, ..saved })
}

/// The mode itself when this build compiles it, else Vanilla.
fn compiled_render_mode(mode: RenderMode) -> RenderMode {
    match mode {
        RenderMode::Enhanced if enhanced_rendering_enabled() => mode,
        RenderMode::ShaderPack if SHADER_PACKS_ENABLED => mode,
        _ => RenderMode::Vanilla,
    }
}

/// CLI beats environment beats the saved file. Attributable evidence runs
/// ignore the environment and saved file so captures stay vanilla. A mode this
/// build does not compile resolves to Vanilla.
pub(crate) fn startup_render_mode(
    cli: Option<RenderMode>,
    env: Option<&OsStr>,
    saved: Option<RenderMode>,
    attributable: bool,
) -> RenderMode {
    let requested = if cli.is_some() {
        cli
    } else if attributable {
        None
    } else {
        env.and_then(OsStr::to_str)
            .and_then(RenderMode::parse)
            .or(saved)
    };
    compiled_render_mode(requested.unwrap_or_default())
}

pub(crate) struct RenderModePlugin {
    cli: Option<RenderMode>,
    attributable: bool,
    shader_pack: Option<PathBuf>,
}

impl RenderModePlugin {
    /// Configure session overrides and deterministic evidence runs.
    pub(crate) const fn new(cli: Option<RenderMode>, attributable: bool) -> Self {
        Self {
            cli,
            attributable,
            shader_pack: None,
        }
    }

    /// Draws this `--shader-pack` path instead of the saved selection while the session's
    /// mode is Shader pack; it is never saved.
    #[must_use]
    pub(crate) fn with_shader_pack(mut self, shader_pack: Option<PathBuf>) -> Self {
        self.shader_pack = shader_pack;
        self
    }
}

#[derive(Resource)]
struct RenderModeConfig {
    cli: Option<RenderMode>,
    attributable: bool,
    path: Option<PathBuf>,
    /// The launch `--shader-pack`, until the player picks a renderer in the menu.
    cli_shader_pack: Option<PathBuf>,
    /// Shader-pack folders, known once the menu supplies the install layout.
    shader_packs: Option<shader_pack::ShaderPackDirs>,
    /// Bumped by each shader-pack reload; see [`ShaderPackRequest::revision`].
    shader_pack_revision: u64,
}

impl Plugin for RenderModePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EnhancedRenderPlugin, ShaderPackRenderPlugin))
            .insert_resource(RenderModeConfig {
                cli: self.cli,
                attributable: self.attributable,
                path: None,
                cli_shader_pack: self.shader_pack.clone(),
                shader_packs: None,
                shader_pack_revision: 0,
            })
            .init_resource::<ShaderPackRequest>()
            .add_message::<bevy::window::WindowFocused>()
            .init_resource::<shader_pack::reload::ShaderPackReloadReport>()
            .add_systems(Startup, seed_render_mode)
            .add_systems(
                Update,
                (
                    shader_pack::refresh_shader_packs,
                    shader_pack::reload::reload_on_key,
                    shader_pack::reload::apply_reload,
                    apply_menu_render_mode,
                    shader_pack::sync_shader_pack_request,
                    shader_pack::status::sync_shader_pack_status,
                    shader_pack::reload::report_reload,
                    authored_textures::update_authored_textures,
                    apply_render_mode_to_cameras,
                    sync_enhanced_bloom,
                )
                    .chain(),
            );
        #[cfg(feature = "shaderpacks")]
        app.init_resource::<shader_options::ShaderOptionsHost>()
            .add_systems(
                Update,
                shader_options::drive_shader_options.after(apply_menu_render_mode),
            );
        #[cfg(feature = "shaderpacks")]
        {
            shader_pack::import::configure(app);
            shader_pack::removal::configure(app);
        }
    }
}

/// Resolve the saved choice and session overrides at startup, and list the shader packs.
fn seed_render_mode(
    mut config: ResMut<RenderModeConfig>,
    mut menu: Option<ResMut<MenuRuntime>>,
    mut settings: ResMut<RuntimeSettings>,
) {
    config.path = menu.as_deref().map(MenuRuntime::graphics_file);
    config.shader_packs = menu
        .as_deref()
        .map(|menu| shader_pack::ShaderPackDirs::new(menu.layout()));
    let saved = config.path.as_deref().and_then(load_graphics_settings);
    let mode = startup_render_mode(
        config.cli,
        std::env::var_os(RENDER_MODE_ENV).as_deref(),
        saved.as_ref().map(|saved| saved.mode),
        config.attributable,
    );
    set_graphics_settings(
        &mut settings,
        &GraphicsSettings {
            mode,
            shader_pack: saved.and_then(|saved| saved.shader_pack),
        },
    );
    if mode == RenderMode::ShaderPack
        && let Some(menu) = menu.as_deref_mut()
    {
        shader_pack::refresh(&mut config, menu);
    }
}

/// Update the shared settings authority only when a choice changes; modes this build does
/// not compile fall back to Vanilla.
fn set_graphics_settings(settings: &mut RuntimeSettings, graphics: &GraphicsSettings) {
    let mode = compiled_render_mode(graphics.mode);
    let (_, current) = settings.user_settings_update();
    if current.video.render_mode != mode || current.video.shader_pack != graphics.shader_pack {
        let mut next = current.clone();
        next.video.render_mode = mode;
        next.video.shader_pack.clone_from(&graphics.shader_pack);
        settings.replace_user_settings(next);
    }
}

/// Changes render mode while retaining the selected pack.
#[cfg(test)]
fn set_render_mode(settings: &mut RuntimeSettings, mode: RenderMode) {
    let graphics = GraphicsSettings {
        mode,
        ..GraphicsSettings::of(&settings.user_settings_update().1.video)
    };
    set_graphics_settings(settings, &graphics);
}

/// Persist menu changes and keep the displayed choices synchronized. A renderer pick ends
/// the `--shader-pack` session override.
fn apply_menu_render_mode(
    mut config: ResMut<RenderModeConfig>,
    menu: Option<ResMut<MenuRuntime>>,
    mut settings: ResMut<RuntimeSettings>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    let mode_request = menu.take_render_mode_request();
    let pack_request = menu.take_shader_pack_request();
    if pack_request.is_some() {
        config.shader_pack_revision = config.shader_pack_revision.wrapping_add(1);
    }
    if mode_request.is_some() || pack_request.is_some() {
        if config.cli_shader_pack.is_some() {
            config.cli_shader_pack = None;
        }
        let current = GraphicsSettings::of(&settings.user_settings_update().1.video);
        let next = GraphicsSettings {
            mode: mode_request.unwrap_or(current.mode),
            shader_pack: pack_request.or(current.shader_pack),
        };
        set_graphics_settings(&mut settings, &next);
        let applied = GraphicsSettings::of(&settings.user_settings_update().1.video);
        if let Some(path) = &config.path
            && let Err(error) = save_graphics_settings(path, &applied)
        {
            warn!(?error, "graphics settings could not be saved");
        }
    }
    let video = &settings.user_settings_update().1.video;
    menu.sync_render_mode(video.render_mode);
    let requested = shader_pack::selected_pack(&config, video);
    menu.sync_shader_pack(
        requested
            .as_ref()
            .map(|pack| pack.name.as_str())
            .or(video.shader_pack.as_deref()),
    );
}

/// Original depth usage, restored once neither optional renderer samples the depth texture.
#[derive(Component)]
struct VanillaDepthUsage(Camera3dDepthTextureUsage);

/// Camera state needed to apply or restore the optional modes.
type RenderModeCameraQuery = (
    Entity,
    Write<Camera3d>,
    Has<EnhancedRendering>,
    Has<ShaderPackRendering>,
    Option<Read<VanillaDepthUsage>>,
);

/// The mode the renderer actually draws: an optional mode only when compiled in, and Shader
/// pack only while its pack has not failed, so every optional resource follows the same
/// fallback as the cameras.
fn effective_render_mode(
    requested: RenderMode,
    shader_pack: Option<&ShaderPackStatus>,
) -> RenderMode {
    match compiled_render_mode(requested) {
        RenderMode::ShaderPack if matches!(shader_pack, Some(ShaderPackStatus::Failed { .. })) => {
            RenderMode::Vanilla
        }
        mode => mode,
    }
}

/// Keep the opt-in effects on gameplay cameras only, never both at once. A shader pack
/// opts in only once it is ready; while it loads the camera draws Vanilla.
fn apply_render_mode_to_cameras(
    mut commands: Commands,
    settings: Res<RuntimeSettings>,
    status: Option<Res<ShaderPackStatus>>,
    mut cameras: Query<RenderModeCameraQuery, With<FlyCamera>>,
) {
    let mode = effective_render_mode(
        settings.user_settings_update().1.video.render_mode,
        status.as_deref(),
    );
    let enhanced = mode == RenderMode::Enhanced;
    let shader_pack =
        mode == RenderMode::ShaderPack && status.as_deref().is_some_and(ShaderPackStatus::is_ready);
    for (entity, mut camera, has_enhanced, has_shader_pack, vanilla_depth) in &mut cameras {
        let mut entity_commands = commands.entity(entity);
        if enhanced != has_enhanced {
            if enhanced {
                let enhanced_settings = EnhancedRendering::default();
                #[cfg(feature = "enhanced-diagnostics")]
                let enhanced_settings = if render_model::enhanced_diagnostics_enabled() {
                    EnhancedRendering::bounded_diagnostic()
                } else {
                    enhanced_settings
                };
                entity_commands.insert((enhanced_settings, Hdr));
            } else {
                entity_commands.remove::<(EnhancedRendering, Hdr, Bloom)>();
            }
        }
        if shader_pack && !has_shader_pack {
            entity_commands.insert((ShaderPackRendering, Msaa::Off));
        } else if !shader_pack && has_shader_pack {
            entity_commands.remove::<ShaderPackRendering>();
        }
        // Both optional renderers sample the depth texture.
        match (enhanced || shader_pack, vanilla_depth) {
            (true, None) => {
                let original = camera.depth_texture_usages;
                camera.depth_texture_usages =
                    (TextureUsages::from(original) | TextureUsages::TEXTURE_BINDING).into();
                entity_commands.insert(VanillaDepthUsage(original));
            }
            (false, Some(VanillaDepthUsage(original))) => {
                camera.depth_texture_usages = *original;
                entity_commands.remove::<VanillaDepthUsage>();
            }
            _ => {}
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

    fn depth_usage(app: &App, camera: Entity) -> TextureUsages {
        TextureUsages::from(
            app.world()
                .get::<Camera3d>(camera)
                .unwrap()
                .depth_texture_usages,
        )
    }

    fn shader_pack_status_ready() -> ShaderPackStatus {
        ShaderPackStatus::Ready(PathBuf::from("Dusk.zip"))
    }

    /// A pack opts cameras in only once ready, a failure falls back, and Vanilla restores depth.
    #[cfg(feature = "shaderpacks")]
    #[test]
    fn shader_pack_cameras_wait_for_a_ready_pack_and_restore_vanilla() {
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>()
            .add_systems(Update, apply_render_mode_to_cameras);
        let original = TextureUsages::from(Camera3d::default().depth_texture_usages);
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), FlyCamera::default(), Msaa::Sample4))
            .id();
        let opted_in = |app: &App| app.world().get::<ShaderPackRendering>(camera).is_some();
        set_render_mode(
            &mut app.world_mut().resource_mut::<RuntimeSettings>(),
            RenderMode::ShaderPack,
        );
        app.world_mut()
            .insert_resource(ShaderPackStatus::Loading(PathBuf::from("Dusk.zip")));
        app.update();
        assert!(!opted_in(&app));
        assert_eq!(depth_usage(&app, camera), original);

        app.world_mut().insert_resource(shader_pack_status_ready());
        app.update();
        assert!(opted_in(&app));
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
        assert!(app.world().get::<EnhancedRendering>(camera).is_none());
        assert_eq!(
            depth_usage(&app, camera),
            original | TextureUsages::TEXTURE_BINDING
        );

        app.world_mut().insert_resource(ShaderPackStatus::Failed {
            pack: PathBuf::from("Dusk.zip"),
            reason: "no shaders folder".to_owned(),
        });
        app.update();
        assert!(!opted_in(&app));
        assert_eq!(depth_usage(&app, camera), original);

        app.world_mut().insert_resource(shader_pack_status_ready());
        app.update();
        assert!(opted_in(&app));
        set_render_mode(
            &mut app.world_mut().resource_mut::<RuntimeSettings>(),
            RenderMode::Vanilla,
        );
        app.update();
        assert!(!opted_in(&app));
        assert!(app.world().get::<VanillaDepthUsage>(camera).is_none());
        assert_eq!(depth_usage(&app, camera), original);
    }

    /// Stale settings and a ready pack cannot opt cameras into the disabled renderer.
    #[cfg(not(feature = "shaderpacks"))]
    #[test]
    fn disabled_shader_packs_clear_stale_camera_opt_ins() {
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>()
            .insert_resource(shader_pack_status_ready())
            .add_systems(Update, apply_render_mode_to_cameras);
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
                ShaderPackRendering,
                VanillaDepthUsage(original),
            ))
            .id();
        let mut stale = ui::UserSettings::default();
        stale.video.render_mode = RenderMode::ShaderPack;
        stale.video.shader_pack = Some("Dusk.zip".to_owned());
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(stale);
        app.update();
        assert!(app.world().get::<ShaderPackRendering>(camera).is_none());
        assert_eq!(depth_usage(&app, camera), TextureUsages::from(original));
    }

    #[test]
    fn a_failed_shader_pack_draws_vanilla() {
        let failed = ShaderPackStatus::Failed {
            pack: PathBuf::from("Dusk.zip"),
            reason: "no shaders folder".to_owned(),
        };
        assert_eq!(
            effective_render_mode(RenderMode::ShaderPack, Some(&failed)),
            RenderMode::Vanilla
        );
        let loading = ShaderPackStatus::Loading(PathBuf::from("Dusk.zip"));
        let compiled = if SHADER_PACKS_ENABLED {
            RenderMode::ShaderPack
        } else {
            RenderMode::Vanilla
        };
        assert_eq!(
            effective_render_mode(RenderMode::ShaderPack, Some(&loading)),
            compiled
        );
        assert_eq!(
            effective_render_mode(RenderMode::ShaderPack, None),
            compiled
        );
    }

    /// Startup selects Shader pack only in builds that compile it; evidence runs stay Vanilla.
    #[test]
    fn shader_pack_startup_follows_the_feature_and_evidence_runs() {
        let compiled = if SHADER_PACKS_ENABLED {
            RenderMode::ShaderPack
        } else {
            RenderMode::Vanilla
        };
        assert_eq!(
            startup_render_mode(Some(RenderMode::ShaderPack), None, None, true),
            compiled
        );
        assert_eq!(
            startup_render_mode(None, Some(OsStr::new("shaderpack")), None, false),
            compiled
        );
        assert_eq!(
            startup_render_mode(None, None, Some(RenderMode::ShaderPack), false),
            compiled
        );
        assert_eq!(
            startup_render_mode(
                None,
                Some(OsStr::new("shaderpack")),
                Some(RenderMode::ShaderPack),
                true,
            ),
            RenderMode::Vanilla
        );
    }

    #[test]
    fn saved_shader_pack_round_trips_and_older_files_still_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("graphics.json");
        fs::write(&path, br#"{"render_mode":"enhanced"}"#).unwrap();
        assert_eq!(
            load_graphics_settings(&path),
            Some(GraphicsSettings {
                mode: RenderMode::Enhanced,
                shader_pack: None,
            })
        );
        let saved = GraphicsSettings {
            mode: RenderMode::ShaderPack,
            shader_pack: Some("Dusk.zip".to_owned()),
        };
        save_graphics_settings(&path, &saved).unwrap();
        assert_eq!(load_graphics_settings(&path), Some(saved));
        save_render_mode(&path, RenderMode::Vanilla).unwrap();
        assert_eq!(
            load_graphics_settings(&path).and_then(|saved| saved.shader_pack),
            Some("Dusk.zip".to_owned()),
            "switching renderer keeps the selected pack"
        );
    }
}
