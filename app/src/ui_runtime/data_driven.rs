//! Host state that data-driven UI answers depend on; its state lives in client-ui.

use bevy::prelude::{Res, ResMut};
use client_shell::{menu::MenuRuntime, ui_resources::UiRuntimeResource};
use client_ui::ui_runtime::{presentation::forms::scene_policy::MenuScene, scene_stack::SceneHost};

/// Publishes the host's menu screens, which screen requests arriving from UI events check
/// for a showing menu.
pub(crate) fn publish_host_screens(
    menu: Option<Res<MenuRuntime>>,
    mut runtime: ResMut<UiRuntimeResource>,
) {
    let host = SceneHost::of(menu.as_deref().map(|menu| menu as &dyn MenuScene), &runtime);
    runtime.observe_host_screens(host);
}
