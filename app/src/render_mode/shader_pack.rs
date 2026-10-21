//! The Shader pack renderer's inputs: the packs listed in the Video picker and the
//! [`ShaderPackRequest`] the render host loads from.

use std::path::{Component, Path, PathBuf};

use bevy::prelude::*;
use launcher::{install_layout::InstallLayout, menu::MenuScreen};
use render_model::SHADER_PACKS_ENABLED;
use render_scene::shaderpack::ShaderPackRequest;
use ui::RenderMode;

use super::{MenuRuntime, RenderModeConfig, RuntimeSettings, compiled_render_mode};

#[cfg(feature = "shaderpacks")]
pub(super) mod import;
pub(super) mod reload;
#[cfg(test)]
mod tests;

/// The install's shader-pack folders and the packs last listed from them.
pub(super) struct ShaderPackDirs {
    /// Player-supplied packs.
    packs: PathBuf,
    /// Private translation cache.
    cache: PathBuf,
    /// `(name, path to open)` of each discovered pack, in picker order.
    discovered: Vec<(String, PathBuf)>,
}

impl ShaderPackDirs {
    /// The layout's shader-pack folders, before anything is listed.
    pub(super) fn new(layout: &InstallLayout) -> Self {
        Self {
            packs: layout.shader_packs_dir(),
            cache: layout.shader_pack_cache_dir(),
            discovered: Vec::new(),
        }
    }

    /// Where the pack saved as `name` opens from: its discovered path (which may be a nested
    /// folder), else `name` inside the pack folder. Names that are not one plain path
    /// component, such as `..` from a hand-edited file, resolve to nothing.
    fn pack_path(&self, name: &str) -> Option<PathBuf> {
        if let Some((_, path)) = self.discovered.iter().find(|(listed, _)| listed == name) {
            return Some(path.clone());
        }
        let mut components = Path::new(name).components();
        matches!(
            (components.next(), components.next()),
            (Some(Component::Normal(_)), None)
        )
        .then(|| self.packs.join(name))
    }
}

/// `(name, path to open)` of each shader pack in `dir`.
#[cfg(feature = "shaderpacks")]
fn discover(dir: &Path) -> Vec<(String, PathBuf)> {
    shaderpack::discover_packs(dir)
        .into_iter()
        .map(|pack| (pack.name, pack.path))
        .collect()
}

/// Builds without shader packs list none and never read the disk.
#[cfg(not(feature = "shaderpacks"))]
fn discover(_dir: &Path) -> Vec<(String, PathBuf)> {
    Vec::new()
}

/// Re-lists the shader-pack folder into the menu's picker when this build compiles shader
/// packs and the install layout is known.
pub(super) fn refresh(config: &mut RenderModeConfig, menu: &mut MenuRuntime) {
    if !SHADER_PACKS_ENABLED {
        return;
    }
    let Some(dirs) = config.shader_packs.as_mut() else {
        return;
    };
    let discovered = discover(&dirs.packs);
    if dirs.discovered != discovered {
        menu.sync_shader_packs(discovered.iter().map(|(name, _)| name.clone()).collect());
        dirs.discovered = discovered;
    }
}

/// Re-lists packs each time Settings opens, so packs added while the client runs appear
/// without scanning the disk every frame. Startup listing happens in `seed_render_mode`.
pub(super) fn refresh_shader_packs(
    mut config: ResMut<RenderModeConfig>,
    menu: Option<ResMut<MenuRuntime>>,
    mut settings_open: Local<bool>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    let open = menu.is_visible() && menu.screen() == MenuScreen::Settings;
    if open && !*settings_open {
        refresh(&mut config, &mut menu);
    }
    *settings_open = open;
}

/// The pack the render host should load: the launch `--shader-pack`, else the selected pack
/// in the shader-pack folder, while Shader pack is the requested renderer.
fn desired_request(config: &RenderModeConfig, video: &ui::VideoSettings) -> ShaderPackRequest {
    let pack = if compiled_render_mode(video.render_mode) == RenderMode::ShaderPack {
        config.cli_shader_pack.clone().or_else(|| {
            let name = video.shader_pack.as_deref()?;
            config.shader_packs.as_ref()?.pack_path(name)
        })
    } else {
        None
    };
    ShaderPackRequest {
        pack,
        cache_dir: config.shader_packs.as_ref().map(|dirs| dirs.cache.clone()),
        revision: config.shader_pack_revision,
    }
}

/// Keeps [`ShaderPackRequest`] following the settings, writing it only when it changes.
pub(super) fn sync_shader_pack_request(
    config: Res<RenderModeConfig>,
    settings: Res<RuntimeSettings>,
    mut request: ResMut<ShaderPackRequest>,
) {
    if !config.is_changed() && !settings.is_changed() && !request.is_added() {
        return;
    }
    let next = desired_request(&config, &settings.user_settings_update().1.video);
    request.set_if_neq(next);
}
