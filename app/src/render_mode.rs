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
    post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter},
    prelude::*,
    render::{render_resource::TextureUsages, view::Hdr},
};
use render::{EnhancedRenderPlugin, EnhancedRendering, EnhancedShadowDebug};
use render_model::ENHANCED_RENDERING_ENABLED;
use serde::{Deserialize, Serialize};
use ui::RenderMode;

use crate::{
    camera::FlyCamera, environment::DebugTimeOverride, menu::MenuRuntime,
    settings_runtime::RuntimeSettings,
};

mod authored_textures;
pub(crate) use authored_textures::AuthoredTextureLoading;

pub(crate) const RENDER_MODE_ENV: &str = "CINNABAR_RENDER_MODE";
const MAX_GRAPHICS_FILE_BYTES: u64 = 4096;
// The right bracket is currently unclaimed by the built-in debug, modding, and
// experience controls, so the probe does not change existing shortcuts.
const ENHANCED_TIME_KEY: KeyCode = KeyCode::BracketRight;
const ENHANCED_SHADOW_KEY: KeyCode = KeyCode::BracketLeft;

const ENHANCED_TIME_PRESETS: [(Option<u32>, &str); 8] = [
    (Some(1_000), "Morning"),
    (Some(6_000), "Noon"),
    (Some(9_000), "Afternoon"),
    (Some(12_000), "Sunset"),
    (Some(12_500), "Twilight"),
    (Some(15_000), "Night"),
    (Some(18_000), "Midnight"),
    (None, "Server"),
];

/// Systems that update render-mode state before the shared atmosphere frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RenderModeUpdateSet;

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
    if !ENHANCED_RENDERING_ENABLED {
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
            .init_resource::<DebugTimeOverride>()
            .add_systems(Startup, seed_render_mode)
            .add_systems(
                Update,
                (
                    apply_menu_render_mode,
                    authored_textures::update_authored_textures,
                    apply_render_mode_to_cameras,
                    cycle_enhanced_shadow_debug,
                    sync_enhanced_bloom,
                    cycle_enhanced_time,
                )
                    .chain()
                    .in_set(RenderModeUpdateSet),
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
    let mode = if ENHANCED_RENDERING_ENABLED {
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
    let enhanced = ENHANCED_RENDERING_ENABLED
        && settings.user_settings_update().1.video.render_mode == RenderMode::Enhanced;
    for (entity, mut camera, has_enhanced, vanilla_depth) in &mut cameras {
        if enhanced != has_enhanced {
            if enhanced {
                let original = camera.depth_texture_usages;
                camera.depth_texture_usages = (TextureUsages::from(original)
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_SRC)
                    .into();
                commands.entity(entity).insert((
                    EnhancedRendering::default(),
                    Hdr,
                    VanillaDepthUsage(original),
                ));
            } else {
                if let Some(VanillaDepthUsage(original)) = vanilla_depth {
                    camera.depth_texture_usages = *original;
                }
                commands.entity(entity).remove::<(
                    EnhancedRendering,
                    bevy::render::camera::TemporalJitter,
                    Hdr,
                    Bloom,
                    VanillaDepthUsage,
                )>();
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
        let bloom = ENHANCED_RENDERING_ENABLED
            && enhanced.bloom
            && enhanced.shadow_debug == EnhancedShadowDebug::Off;
        if bloom == has_bloom {
            continue;
        }
        if bloom {
            commands.entity(entity).insert(Bloom {
                intensity: 0.045,
                low_frequency_boost: 0.0,
                prefilter: BloomPrefilter {
                    threshold: 1.5,
                    threshold_softness: 0.25,
                },
                composite_mode: BloomCompositeMode::Additive,
                ..default()
            });
        } else {
            commands.entity(entity).remove::<Bloom>();
        }
    }
}

/// Cycles local lighting presets without changing server time or gameplay.
fn cycle_enhanced_time(
    settings: Res<RuntimeSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    menu: Option<Res<MenuRuntime>>,
    mut debug_time: ResMut<DebugTimeOverride>,
    mut next_preset: Local<usize>,
) {
    let enhanced = ENHANCED_RENDERING_ENABLED
        && settings.user_settings_update().1.video.render_mode == RenderMode::Enhanced;
    if !enhanced {
        *next_preset = 0;
        debug_time.ticks = None;
        return;
    }
    let focused = windows.iter().next().is_none_or(|window| window.focused);
    if !focused || menu.as_ref().is_some_and(|menu| menu.is_visible()) {
        return;
    }
    if !keys.just_pressed(ENHANCED_TIME_KEY) {
        return;
    }
    let (ticks, label) = ENHANCED_TIME_PRESETS[*next_preset];
    *next_preset = (*next_preset + 1) % ENHANCED_TIME_PRESETS.len();
    debug_time.ticks = ticks;
    debug!(target: "cinnabar::enhanced", ?ticks, preset = label, "changed debug time preset");
}

fn shadow_debug_input_allowed(mode: RenderMode, focused: bool, menu_visible: bool) -> bool {
    ENHANCED_RENDERING_ENABLED && mode == RenderMode::Enhanced && focused && !menu_visible
}

fn next_shadow_debug(mode: EnhancedShadowDebug) -> (EnhancedShadowDebug, &'static str) {
    use EnhancedShadowDebug::{Cascades, DepthFar, DepthMiddle, DepthNear, Off, Visibility};
    match mode {
        Off => (Cascades, "Cascade coverage"),
        Cascades => (Visibility, "Shadow visibility"),
        Visibility => (DepthNear, "Near shadow depth"),
        DepthNear => (DepthMiddle, "Middle shadow depth"),
        DepthMiddle => (DepthFar, "Far shadow depth"),
        DepthFar => (Off, "Off"),
    }
}

fn cycle_enhanced_shadow_debug(
    settings: Res<RuntimeSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    menu: Option<Res<MenuRuntime>>,
    mut cameras: Query<&mut EnhancedRendering, With<FlyCamera>>,
) {
    let focused = windows.iter().next().is_none_or(|window| window.focused);
    let menu_visible = menu.as_ref().is_some_and(|menu| menu.is_visible());
    if !shadow_debug_input_allowed(
        settings.user_settings_update().1.video.render_mode,
        focused,
        menu_visible,
    ) || !keys.just_pressed(ENHANCED_SHADOW_KEY)
    {
        return;
    }
    for mut enhanced in &mut cameras {
        let (next, label) = next_shadow_debug(enhanced.shadow_debug);
        enhanced.shadow_debug = next;
        info!(target: "cinnabar::enhanced", mode = label, "changed shadow debug view");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_debug_cycles_all_views_and_returns_to_normal_lighting() {
        let mut mode = EnhancedShadowDebug::Off;
        let mut observed = Vec::new();
        for _ in 0..6 {
            mode = next_shadow_debug(mode).0;
            observed.push(mode);
        }
        assert_eq!(
            observed,
            [
                EnhancedShadowDebug::Cascades,
                EnhancedShadowDebug::Visibility,
                EnhancedShadowDebug::DepthNear,
                EnhancedShadowDebug::DepthMiddle,
                EnhancedShadowDebug::DepthFar,
                EnhancedShadowDebug::Off,
            ]
        );
    }

    #[test]
    fn shadow_debug_input_respects_render_mode_focus_and_menu() {
        assert!(!shadow_debug_input_allowed(
            RenderMode::Vanilla,
            true,
            false
        ));
        assert!(!shadow_debug_input_allowed(
            RenderMode::Enhanced,
            false,
            false
        ));
        assert!(!shadow_debug_input_allowed(
            RenderMode::Enhanced,
            true,
            true
        ));
        assert_eq!(
            shadow_debug_input_allowed(RenderMode::Enhanced, true, false),
            ENHANCED_RENDERING_ENABLED,
        );
    }

    #[cfg(feature = "enhanced")]
    #[test]
    fn shadow_key_changes_only_focused_enhanced_gameplay_cameras() {
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, cycle_enhanced_shadow_debug);
        let camera = app
            .world_mut()
            .spawn((FlyCamera::default(), EnhancedRendering::default()))
            .id();
        let window = app
            .world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(ENHANCED_SHADOW_KEY);
        app.update();
        assert_eq!(
            app.world()
                .get::<EnhancedRendering>(camera)
                .unwrap()
                .shadow_debug,
            EnhancedShadowDebug::Off,
        );
        set_render_mode(
            &mut app.world_mut().resource_mut::<RuntimeSettings>(),
            RenderMode::Enhanced,
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert_eq!(
            app.world()
                .get::<EnhancedRendering>(camera)
                .unwrap()
                .shadow_debug,
            EnhancedShadowDebug::Off,
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_eq!(
            app.world()
                .get::<EnhancedRendering>(camera)
                .unwrap()
                .shadow_debug,
            EnhancedShadowDebug::Cascades,
        );
    }

    #[test]
    fn enhanced_time_presets_cycle_through_daylight_and_server_clock() {
        let mut index = 0;
        let mut values = Vec::new();
        for _ in 0..ENHANCED_TIME_PRESETS.len() {
            values.push(ENHANCED_TIME_PRESETS[index]);
            index = (index + 1) % ENHANCED_TIME_PRESETS.len();
        }
        assert_eq!(
            values,
            [
                (Some(1_000), "Morning"),
                (Some(6_000), "Noon"),
                (Some(9_000), "Afternoon"),
                (Some(12_000), "Sunset"),
                (Some(12_500), "Twilight"),
                (Some(15_000), "Night"),
                (Some(18_000), "Midnight"),
                (None, "Server"),
            ]
        );
    }

    /// Startup requests cannot enable Enhanced in a default build.
    #[cfg(not(feature = "enhanced"))]
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

    /// The opt-in feature permits explicit Enhanced requests while evidence runs stay vanilla.
    #[cfg(feature = "enhanced")]
    #[test]
    fn enhanced_feature_respects_startup_precedence() {
        assert_eq!(
            startup_render_mode(
                Some(RenderMode::Enhanced),
                Some(OsStr::new("vanilla")),
                Some(RenderMode::Vanilla),
                true,
            ),
            RenderMode::Enhanced,
        );
        assert_eq!(
            startup_render_mode(
                None,
                Some(OsStr::new("enhanced")),
                Some(RenderMode::Vanilla),
                false,
            ),
            RenderMode::Enhanced,
        );
        assert_eq!(
            startup_render_mode(None, None, Some(RenderMode::Enhanced), false),
            RenderMode::Enhanced,
        );
        assert_eq!(
            startup_render_mode(
                None,
                Some(OsStr::new("enhanced")),
                Some(RenderMode::Enhanced),
                true,
            ),
            RenderMode::Vanilla,
        );
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
    #[cfg(not(feature = "enhanced"))]
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
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_SRC)
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

    /// Explicit Enhanced mode adds camera effects and Vanilla restores the original camera.
    #[cfg(feature = "enhanced")]
    #[test]
    fn enhanced_mode_applies_and_clears_camera_effects() {
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>().add_systems(
            Update,
            (apply_render_mode_to_cameras, sync_enhanced_bloom).chain(),
        );
        let original = Camera3d::default().depth_texture_usages;
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), FlyCamera::default()))
            .id();

        set_render_mode(
            &mut app.world_mut().resource_mut::<RuntimeSettings>(),
            RenderMode::Enhanced,
        );
        app.update();
        assert!(app.world().get::<EnhancedRendering>(camera).is_some());
        assert!(app.world().get::<Hdr>(camera).is_some());
        assert!(app.world().get::<Bloom>(camera).is_some());
        let enhanced_depth = TextureUsages::from(
            app.world()
                .get::<Camera3d>(camera)
                .unwrap()
                .depth_texture_usages,
        );
        assert!(enhanced_depth.contains(TextureUsages::TEXTURE_BINDING));
        assert!(enhanced_depth.contains(TextureUsages::COPY_SRC));

        set_render_mode(
            &mut app.world_mut().resource_mut::<RuntimeSettings>(),
            RenderMode::Vanilla,
        );
        app.update();
        assert!(app.world().get::<EnhancedRendering>(camera).is_none());
        assert!(app.world().get::<Hdr>(camera).is_none());
        assert!(app.world().get::<Bloom>(camera).is_none());
        assert_eq!(
            TextureUsages::from(
                app.world()
                    .get::<Camera3d>(camera)
                    .unwrap()
                    .depth_texture_usages
            ),
            TextureUsages::from(original),
        );
    }
}
