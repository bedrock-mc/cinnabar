//! Reloads the selected pack from disk under a new [`ShaderPackRequest::revision`]. Menu
//! reloads also refresh the pack list; the gameplay key leaves discovery off the frame.
//! The render host keeps drawing the shown pack until the reloaded one is ready.

use std::sync::Arc;

use bevy::{prelude::*, time::Real};
use client_shell::{
    menu::settings_options::control_bindings::binding_pressed,
    screen_policy::absorbs_input,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_state::PlayerRuntime;
use launcher::menu::settings_options::BindingId;
use render_scene::shaderpack::{ShaderPackRequest, ShaderPackSettledRevision, ShaderPackStatus};

use super::{MenuRuntime, RenderModeConfig, refresh};

/// Iris's chat line after its reload key reloads.
const RELOADED: &str = "Shaders Reloaded!";

/// The reload whose outcome the player asked for with the reload key, reported in chat as
/// Iris does; reloads asked for by menus report through their own screens.
#[derive(Resource, Default)]
pub(in crate::render_mode) struct ShaderPackReloadReport {
    /// The revision being reported once its load settles.
    pending: Option<u64>,
}

/// The gameplay input the reload key needs; absent resources mean no world to reload in.
#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::render_mode) struct ReloadKey<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    player: Option<Res<'w, PlayerRuntime>>,
    ui: Option<Res<'w, UiRuntimeResource>>,
    presentation: Option<Res<'w, UiPresentationResource>>,
}

impl ReloadKey<'_> {
    /// Whether the Reload Shaders binding was pressed during gameplay with no screen open.
    fn pressed(&self, menu: &MenuRuntime) -> bool {
        let Some(player) = self.player.as_deref() else {
            return false;
        };
        binding_pressed(
            Some(menu),
            BindingId::ReloadShaders,
            &self.keys,
            &self.mouse,
        ) && !absorbs_input(
            player,
            self.ui.as_deref().map(|ui| &ui.0),
            Some(menu),
            self.presentation
                .as_deref()
                .map(|presentation| &presentation.0),
        )
    }
}

/// Reloads the selected pack when the Reload Shaders key is pressed in game. Without a
/// selected pack the key does nothing. The pack is re-read on the loader's worker, so the
/// frame never lists the pack folder.
pub(in crate::render_mode) fn reload_on_key(
    key: ReloadKey,
    menu: Option<Res<MenuRuntime>>,
    request: Res<ShaderPackRequest>,
    mut config: ResMut<RenderModeConfig>,
    mut report: ResMut<ShaderPackReloadReport>,
) {
    if request.pack.is_none() {
        return;
    }
    if menu.is_some_and(|menu| key.pressed(&menu)) {
        config.shader_pack_revision += 1;
        report.pending = Some(config.shader_pack_revision);
    }
}

/// Applies a [`MenuRuntime::request_shader_pack_reload`]: re-lists the pack folder and bumps
/// the revision the next [`ShaderPackRequest`] carries.
pub(in crate::render_mode) fn apply_reload(
    mut config: ResMut<RenderModeConfig>,
    menu: Option<ResMut<MenuRuntime>>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    if !menu.take_shader_pack_reload() {
        return;
    }
    refresh(&mut config, &mut menu);
    config.shader_pack_revision += 1;
}

/// Reports a key reload in chat once its load settles.
pub(in crate::render_mode) fn report_reload(
    mut report: ResMut<ShaderPackReloadReport>,
    request: Res<ShaderPackRequest>,
    settled: Option<Res<ShaderPackSettledRevision>>,
    status: Option<Res<ShaderPackStatus>>,
    ui: Option<ResMut<UiRuntimeResource>>,
    time: Res<Time<Real>>,
) {
    let Some(revision) = report.pending else {
        return;
    };
    let Some(line) = report_line(
        revision,
        &request,
        settled.map_or(0, |settled| settled.0),
        status.as_deref(),
    ) else {
        return;
    };
    report.pending = None;
    if let Some(mut ui) = ui {
        let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
        ui.0.push_local_chat_line(Arc::from(line), now_millis);
    }
}

/// The chat line for the reload to `revision` once its load has settled: Iris's success
/// line, or its failure line with the reason. With no pack requested there is nothing to
/// load, so it reports at once, as Iris does with shaders off.
fn report_line(
    revision: u64,
    request: &ShaderPackRequest,
    settled: u64,
    status: Option<&ShaderPackStatus>,
) -> Option<String> {
    if request.pack.is_some() && settled < revision {
        return None;
    }
    Some(match status {
        Some(ShaderPackStatus::Failed { reason, .. }) if request.pack.is_some() => {
            format!("§cFailed to reload shaders! Reason: {reason}")
        }
        _ => RELOADED.to_owned(),
    })
}

#[cfg(test)]
mod tests;
