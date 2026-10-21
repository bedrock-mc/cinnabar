//! Shader-pack reloads, Iris's R key: re-list the pack folder, then load the selected pack
//! again from disk under a new [`ShaderPackRequest::revision`]. The render host keeps
//! drawing the shown pack until the reloaded one is ready.

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
    /// The next menu reload came from the key.
    key_pressed: bool,
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

/// Requests a reload when the Reload Shaders key is pressed in game.
pub(in crate::render_mode) fn reload_on_key(
    key: ReloadKey,
    menu: Option<ResMut<MenuRuntime>>,
    mut report: ResMut<ShaderPackReloadReport>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    if key.pressed(&menu) {
        menu.request_shader_pack_reload();
        report.key_pressed = true;
    }
}

/// Applies a [`MenuRuntime::request_shader_pack_reload`]: re-lists the pack folder and bumps
/// the revision the next [`ShaderPackRequest`] carries.
pub(in crate::render_mode) fn apply_reload(
    mut config: ResMut<RenderModeConfig>,
    menu: Option<ResMut<MenuRuntime>>,
    mut report: ResMut<ShaderPackReloadReport>,
) {
    let Some(mut menu) = menu else {
        return;
    };
    if !menu.take_shader_pack_reload() {
        return;
    }
    refresh(&mut config, &mut menu);
    config.shader_pack_revision += 1;
    if std::mem::take(&mut report.key_pressed) {
        report.pending = Some(config.shader_pack_revision);
    }
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
